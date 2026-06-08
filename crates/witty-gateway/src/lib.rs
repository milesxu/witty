//! Native WebSocket gateway between browser sessions and local PTYs.

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::result::Result as StdResult;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tungstenite::{
    accept_hdr_with_config,
    error::Error as WebSocketError,
    handshake::server::{Callback, ErrorResponse, Request, Response},
    protocol::WebSocketConfig,
    Message, WebSocket,
};
use witty_core::GridSize;
use witty_transport::{
    BrowserGatewayClientMessage, BrowserGatewayServerMessage, LocalPtyConfig, LocalPtyTransport,
    TerminalTransport, TransportEvent, BROWSER_GATEWAY_PROTOCOL_VERSION,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_CLIENT_FRAME_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_SERVER_FRAME_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_OUTPUT_BURST_BYTES: usize = 512 * 1024;
const DEFAULT_MAX_WS_WRITE_BUFFER_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayConfig {
    pub bind: String,
    pub once: bool,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub local_pty_config: Option<LocalPtyConfig>,
    pub default_size: GridSize,
    pub allow_non_loopback: bool,
    pub token: Option<String>,
    pub allowed_origins: Vec<String>,
    pub write_timeout: Duration,
    pub max_client_frame_bytes: usize,
    pub max_server_frame_bytes: usize,
    pub max_output_burst_bytes: usize,
    pub max_ws_write_buffer_bytes: usize,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8788".to_owned(),
            once: false,
            program: None,
            args: Vec::new(),
            local_pty_config: None,
            default_size: GridSize::new(24, 80),
            allow_non_loopback: false,
            token: None,
            allowed_origins: Vec::new(),
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            max_client_frame_bytes: DEFAULT_MAX_CLIENT_FRAME_BYTES,
            max_server_frame_bytes: DEFAULT_MAX_SERVER_FRAME_BYTES,
            max_output_burst_bytes: DEFAULT_MAX_OUTPUT_BURST_BYTES,
            max_ws_write_buffer_bytes: DEFAULT_MAX_WS_WRITE_BUFFER_BYTES,
        }
    }
}

pub fn run_cli(args: impl IntoIterator<Item = String>) -> Result<()> {
    let config = parse_config(args)?;
    if !config.once {
        bail!("witty-gateway currently supports only --once mode");
    }
    run_once(config)
}

pub fn parse_config(args: impl IntoIterator<Item = String>) -> Result<GatewayConfig> {
    let mut config = GatewayConfig::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                config.bind = args
                    .next()
                    .context("--bind requires an address like 127.0.0.1:8788")?;
            }
            "--once" => config.once = true,
            "--allow-non-loopback" => config.allow_non_loopback = true,
            "--token" => {
                config.token = Some(args.next().context("--token requires a value")?);
            }
            "--allow-origin" => {
                config.allowed_origins.push(
                    args.next()
                        .context("--allow-origin requires an origin like http://127.0.0.1:8787")?,
                );
            }
            "--write-timeout-ms" => {
                let timeout_ms = parse_u64_arg("--write-timeout-ms", args.next())?;
                config.write_timeout = Duration::from_millis(timeout_ms);
            }
            "--max-client-frame-bytes" => {
                config.max_client_frame_bytes =
                    parse_usize_arg("--max-client-frame-bytes", args.next())?;
            }
            "--max-server-frame-bytes" => {
                config.max_server_frame_bytes =
                    parse_usize_arg("--max-server-frame-bytes", args.next())?;
            }
            "--max-output-burst-bytes" => {
                config.max_output_burst_bytes =
                    parse_usize_arg("--max-output-burst-bytes", args.next())?;
            }
            "--max-ws-write-buffer-bytes" => {
                config.max_ws_write_buffer_bytes =
                    parse_usize_arg("--max-ws-write-buffer-bytes", args.next())?;
            }
            "--program" => {
                config.program = Some(args.next().context("--program requires a path")?);
            }
            "--arg" => {
                config
                    .args
                    .push(args.next().context("--arg requires a value")?);
            }
            "--rows" => {
                config.default_size.rows = parse_u16_arg("--rows", args.next())?;
            }
            "--cols" => {
                config.default_size.cols = parse_u16_arg("--cols", args.next())?;
            }
            _ => bail!("unknown argument {arg}"),
        }
    }

    validate_config(&config)?;
    Ok(config)
}

pub fn run_once(config: GatewayConfig) -> Result<()> {
    validate_config(&config)?;
    let bind_addr = parse_bind_addr(&config)?;
    let listener = TcpListener::bind(bind_addr)
        .with_context(|| format!("bind term gateway at {bind_addr}"))?;
    run_once_on_listener(listener, config)
}

pub fn run_once_on_listener(listener: TcpListener, config: GatewayConfig) -> Result<()> {
    validate_config(&config)?;
    validate_listener_addr(&listener, &config)?;
    eprintln!(
        "Witty gateway listening on {}",
        listener
            .local_addr()
            .context("read gateway listener address")?
    );
    let (stream, _) = listener.accept().context("accept gateway websocket")?;
    stream
        .set_read_timeout(Some(POLL_INTERVAL))
        .context("set gateway socket read timeout")?;
    stream
        .set_write_timeout(Some(config.write_timeout))
        .context("set gateway socket write timeout")?;
    let websocket = accept_gateway_websocket(stream, &config)?;
    run_connection(websocket, config)
}

fn accept_gateway_websocket(
    stream: TcpStream,
    config: &GatewayConfig,
) -> Result<WebSocket<TcpStream>> {
    let policy = GatewayHandshakePolicy {
        token: config.token.clone(),
        allowed_origins: config.allowed_origins.clone(),
    };
    accept_hdr_with_config(
        stream,
        GatewayCallback { policy },
        Some(gateway_websocket_config(config)),
    )
    .map_err(|error| anyhow!("accept websocket upgrade: {error:?}"))
}

fn parse_bind_addr(config: &GatewayConfig) -> Result<SocketAddr> {
    let addr = config
        .bind
        .parse::<SocketAddr>()
        .with_context(|| format!("--bind must be an IP socket address, got {}", config.bind))?;
    if !addr.ip().is_loopback() && !config.allow_non_loopback {
        bail!("refusing non-loopback gateway bind {addr}; pass --allow-non-loopback explicitly")
    }
    Ok(addr)
}

fn validate_listener_addr(listener: &TcpListener, config: &GatewayConfig) -> Result<()> {
    let addr = listener
        .local_addr()
        .context("read gateway listener address")?;
    if !addr.ip().is_loopback() && !config.allow_non_loopback {
        bail!("refusing non-loopback gateway listener {addr}; pass --allow-non-loopback explicitly")
    }
    Ok(())
}

fn validate_config(config: &GatewayConfig) -> Result<()> {
    if config.write_timeout.as_millis() == 0 {
        bail!("--write-timeout-ms must be greater than zero");
    }
    if config.max_client_frame_bytes == 0 {
        bail!("--max-client-frame-bytes must be greater than zero");
    }
    if config.max_server_frame_bytes == 0 {
        bail!("--max-server-frame-bytes must be greater than zero");
    }
    if config.max_output_burst_bytes < config.max_server_frame_bytes {
        bail!("--max-output-burst-bytes must be at least --max-server-frame-bytes");
    }
    if config.max_ws_write_buffer_bytes == 0 {
        bail!("--max-ws-write-buffer-bytes must be greater than zero");
    }
    if config.local_pty_config.is_some() && (config.program.is_some() || !config.args.is_empty()) {
        bail!("gateway local PTY config cannot be combined with --program or --arg");
    }
    Ok(())
}

fn gateway_websocket_config(config: &GatewayConfig) -> WebSocketConfig {
    WebSocketConfig::default()
        .write_buffer_size(0)
        .max_write_buffer_size(config.max_ws_write_buffer_bytes)
        .max_message_size(Some(config.max_client_frame_bytes))
        .max_frame_size(Some(config.max_client_frame_bytes))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GatewayHandshakePolicy {
    token: Option<String>,
    allowed_origins: Vec<String>,
}

struct GatewayCallback {
    policy: GatewayHandshakePolicy,
}

impl Callback for GatewayCallback {
    #[allow(clippy::result_large_err)]
    fn on_request(
        self,
        request: &Request,
        response: Response,
    ) -> StdResult<Response, ErrorResponse> {
        validate_handshake(request, response, &self.policy)
    }
}

#[allow(clippy::result_large_err)]
fn validate_handshake(
    request: &Request,
    response: Response,
    policy: &GatewayHandshakePolicy,
) -> StdResult<Response, ErrorResponse> {
    if let Some(rejection) = origin_rejection(request, policy) {
        return Err(rejection);
    }
    if let Some(rejection) = token_rejection(request, policy) {
        return Err(rejection);
    }
    Ok(response)
}

fn origin_rejection(request: &Request, policy: &GatewayHandshakePolicy) -> Option<ErrorResponse> {
    let Some(origin) = request_header(request, "origin") else {
        return Some(gateway_error_response(403, "missing Origin header"));
    };

    if policy.allowed_origins.is_empty() {
        if is_loopback_origin(origin) {
            return None;
        }
    } else if policy
        .allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return None;
    }

    Some(gateway_error_response(403, "Origin is not allowed"))
}

fn token_rejection(request: &Request, policy: &GatewayHandshakePolicy) -> Option<ErrorResponse> {
    let Some(expected) = &policy.token else {
        return None;
    };

    if request_query_value(request, "token").as_deref() == Some(expected.as_str()) {
        return None;
    }

    Some(gateway_error_response(401, "invalid gateway token"))
}

fn request_header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers().get(name)?.to_str().ok()
}

fn request_query_value(request: &Request, key: &str) -> Option<String> {
    request.uri().query()?.split('&').find_map(|part| {
        let (part_key, part_value) = part.split_once('=')?;
        (part_key == key).then(|| part_value.to_owned())
    })
}

fn is_loopback_origin(origin: &str) -> bool {
    origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("https://127.0.0.1:")
        || origin.starts_with("http://localhost:")
        || origin.starts_with("https://localhost:")
        || origin.starts_with("http://[::1]:")
        || origin.starts_with("https://[::1]:")
}

fn gateway_error_response(status: u16, message: &str) -> ErrorResponse {
    let response = Response::builder()
        .status(status)
        .body(())
        .expect("valid gateway error response");
    let (parts, ()) = response.into_parts();
    ErrorResponse::from_parts(parts, Some(message.to_owned()))
}

pub fn run_connection(mut websocket: WebSocket<TcpStream>, config: GatewayConfig) -> Result<()> {
    let mut session = GatewaySession::new(config.clone());
    let idle_deadline = Instant::now() + IDLE_TIMEOUT;

    loop {
        match websocket.read() {
            Ok(message) => {
                if message.is_close() {
                    return Ok(());
                }
                if message.is_ping() {
                    send_websocket_message(
                        &mut websocket,
                        Message::Pong(message.into_data()),
                        &config,
                        "send gateway websocket pong",
                    )?;
                } else if message.is_text() {
                    let text = message.to_text().context("read gateway text frame")?;
                    if text.len() > config.max_client_frame_bytes {
                        bail!(
                            "gateway client frame exceeded {} bytes",
                            config.max_client_frame_bytes
                        );
                    }
                    let client = BrowserGatewayClientMessage::from_json(text)
                        .context("parse browser gateway client frame")?;
                    for response in session
                        .handle_client_message(client)
                        .context("handle browser gateway client frame")?
                    {
                        send_server_message(&mut websocket, response, &config)?;
                    }
                }
            }
            Err(error) if is_timeout_or_would_block(&error) => {}
            Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => return Ok(()),
            Err(error) => return Err(error).context("read gateway websocket frame"),
        }

        for response in session.poll_server_messages()? {
            let should_exit = matches!(response, BrowserGatewayServerMessage::Exit { .. });
            send_server_message(&mut websocket, response, &config)?;
            if should_exit {
                return Ok(());
            }
        }

        if !session.is_spawned() && Instant::now() > idle_deadline {
            bail!("gateway timed out before PTY session started");
        }
    }
}

pub struct GatewaySession {
    config: GatewayConfig,
    size: GridSize,
    greeted: bool,
    transport: Option<LocalPtyTransport>,
    pending_server_messages: VecDeque<BrowserGatewayServerMessage>,
}

impl GatewaySession {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            size: config.default_size,
            config,
            greeted: false,
            transport: None,
            pending_server_messages: VecDeque::new(),
        }
    }

    pub fn is_spawned(&self) -> bool {
        self.transport.is_some()
    }

    pub fn handle_client_message(
        &mut self,
        message: BrowserGatewayClientMessage,
    ) -> Result<Vec<BrowserGatewayServerMessage>> {
        match message {
            BrowserGatewayClientMessage::Hello { protocol } => {
                if protocol != BROWSER_GATEWAY_PROTOCOL_VERSION {
                    bail!(
                        "unsupported gateway protocol {protocol}; expected {BROWSER_GATEWAY_PROTOCOL_VERSION}"
                    );
                }
                self.greeted = true;
                Ok(vec![BrowserGatewayServerMessage::Ready {
                    protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
                }])
            }
            BrowserGatewayClientMessage::Resize { rows, cols } => {
                self.require_hello()?;
                self.size = GridSize::new(rows, cols);
                if let Some(transport) = &mut self.transport {
                    transport.resize(self.size).context("resize gateway pty")?;
                } else {
                    self.spawn_transport()?;
                }
                Ok(Vec::new())
            }
            BrowserGatewayClientMessage::Input { bytes } => {
                self.require_hello()?;
                if bytes.len() > self.config.max_client_frame_bytes {
                    bail!(
                        "gateway input payload exceeded {} bytes",
                        self.config.max_client_frame_bytes
                    );
                }
                self.spawn_transport()?
                    .write(&bytes)
                    .context("write browser input to gateway pty")?;
                Ok(Vec::new())
            }
            BrowserGatewayClientMessage::Pong { .. } => Ok(Vec::new()),
        }
    }

    pub fn poll_server_messages(&mut self) -> Result<Vec<BrowserGatewayServerMessage>> {
        let mut messages = Vec::new();
        let mut burst_bytes = 0;

        while let Some(message) = self.pending_server_messages.pop_front() {
            if !self.push_server_message_for_burst(
                message,
                &mut messages,
                &mut burst_bytes,
                true,
            )? {
                return Ok(messages);
            }
        }

        loop {
            let event = match self.transport.as_mut() {
                Some(transport) => transport.poll_event().context("poll gateway pty")?,
                None => None,
            };
            let Some(event) = event else {
                break;
            };
            let message = match event {
                TransportEvent::Output(bytes) => BrowserGatewayServerMessage::Output { bytes },
                TransportEvent::Exit { code } => BrowserGatewayServerMessage::Exit { code },
                TransportEvent::Error(message) => BrowserGatewayServerMessage::Error { message },
            };
            if !self.push_server_message_for_burst(
                message,
                &mut messages,
                &mut burst_bytes,
                false,
            )? {
                break;
            }
        }
        Ok(messages)
    }

    fn require_hello(&self) -> Result<()> {
        if self.greeted {
            Ok(())
        } else {
            bail!("gateway client must send hello before terminal frames")
        }
    }

    fn spawn_transport(&mut self) -> Result<&mut LocalPtyTransport> {
        if self.transport.is_none() {
            let config = self.transport_config_for_spawn();
            self.transport = Some(LocalPtyTransport::spawn(config).context("spawn gateway pty")?);
        }

        Ok(self
            .transport
            .as_mut()
            .expect("transport was initialized above"))
    }

    fn transport_config_for_spawn(&self) -> LocalPtyConfig {
        if let Some(config) = &self.config.local_pty_config {
            let mut config = config.clone();
            config.size = self.size;
            return config;
        }

        let mut config = match &self.config.program {
            Some(program) => LocalPtyConfig::command(self.size, program),
            None => LocalPtyConfig::new(self.size),
        };
        config.args(self.config.args.clone());
        config
    }

    fn push_server_message_for_burst(
        &mut self,
        message: BrowserGatewayServerMessage,
        messages: &mut Vec<BrowserGatewayServerMessage>,
        burst_bytes: &mut usize,
        defer_to_front: bool,
    ) -> Result<bool> {
        let frame_bytes = serialized_server_frame_len(&message)?;
        if frame_bytes > self.config.max_server_frame_bytes {
            bail!(
                "gateway server frame exceeded {} bytes",
                self.config.max_server_frame_bytes
            );
        }

        if !messages.is_empty() && *burst_bytes + frame_bytes > self.config.max_output_burst_bytes {
            if defer_to_front {
                self.pending_server_messages.push_front(message);
            } else {
                self.pending_server_messages.push_back(message);
            }
            return Ok(false);
        }

        *burst_bytes += frame_bytes;
        messages.push(message);
        Ok(true)
    }
}

fn send_server_message(
    websocket: &mut WebSocket<TcpStream>,
    message: BrowserGatewayServerMessage,
    config: &GatewayConfig,
) -> Result<()> {
    let json = message
        .to_json()
        .context("serialize gateway server frame")?;
    if json.len() > config.max_server_frame_bytes {
        bail!(
            "gateway server frame exceeded {} bytes",
            config.max_server_frame_bytes
        );
    }
    send_websocket_message(
        websocket,
        Message::Text(json.into()),
        config,
        "send gateway server frame",
    )
}

fn send_websocket_message(
    websocket: &mut WebSocket<TcpStream>,
    message: Message,
    config: &GatewayConfig,
    context: &'static str,
) -> Result<()> {
    websocket
        .send(message)
        .map_err(|error| map_websocket_write_error(error, config))
        .with_context(|| context)
}

fn map_websocket_write_error(error: WebSocketError, config: &GatewayConfig) -> anyhow::Error {
    match error {
        WebSocketError::WriteBufferFull(_) => anyhow!(
            "gateway websocket write buffer exceeded {} bytes; closing session",
            config.max_ws_write_buffer_bytes
        ),
        WebSocketError::Io(error) if is_timeout_io_kind(error.kind()) => anyhow!(
            "gateway websocket write timed out after {} ms; closing session",
            config.write_timeout.as_millis()
        ),
        error => anyhow!(error),
    }
}

fn is_timeout_or_would_block(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::Io(err)
            if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

fn is_timeout_io_kind(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

fn serialized_server_frame_len(message: &BrowserGatewayServerMessage) -> Result<usize> {
    Ok(message
        .to_json()
        .context("serialize gateway server frame for sizing")?
        .len())
}

fn parse_u16_arg(name: &str, value: Option<String>) -> Result<u16> {
    value
        .with_context(|| format!("{name} requires a value"))?
        .parse()
        .with_context(|| format!("{name} must be an unsigned 16-bit integer"))
}

fn parse_usize_arg(name: &str, value: Option<String>) -> Result<usize> {
    value
        .with_context(|| format!("{name} requires a value"))?
        .parse()
        .with_context(|| format!("{name} must be a positive integer"))
}

fn parse_u64_arg(name: &str, value: Option<String>) -> Result<u64> {
    value
        .with_context(|| format!("{name} requires a value"))?
        .parse()
        .with_context(|| format!("{name} must be a positive integer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_accepts_once_bind_and_command() {
        let config = parse_config([
            "--once".to_owned(),
            "--bind".to_owned(),
            "127.0.0.1:9999".to_owned(),
            "--program".to_owned(),
            "/bin/sh".to_owned(),
            "--arg".to_owned(),
            "-lc".to_owned(),
            "--arg".to_owned(),
            "cat".to_owned(),
            "--token".to_owned(),
            "secret".to_owned(),
            "--allow-origin".to_owned(),
            "http://127.0.0.1:8787".to_owned(),
            "--write-timeout-ms".to_owned(),
            "2500".to_owned(),
            "--max-client-frame-bytes".to_owned(),
            "4096".to_owned(),
            "--max-server-frame-bytes".to_owned(),
            "8192".to_owned(),
            "--max-output-burst-bytes".to_owned(),
            "16384".to_owned(),
            "--max-ws-write-buffer-bytes".to_owned(),
            "32768".to_owned(),
            "--rows".to_owned(),
            "30".to_owned(),
            "--cols".to_owned(),
            "100".to_owned(),
        ])
        .unwrap();

        assert_eq!(config.bind, "127.0.0.1:9999");
        assert!(config.once);
        assert_eq!(config.program.as_deref(), Some("/bin/sh"));
        assert_eq!(config.args, ["-lc", "cat"]);
        assert_eq!(config.token.as_deref(), Some("secret"));
        assert_eq!(config.allowed_origins, ["http://127.0.0.1:8787"]);
        assert_eq!(config.write_timeout, Duration::from_millis(2500));
        assert_eq!(config.max_client_frame_bytes, 4096);
        assert_eq!(config.max_server_frame_bytes, 8192);
        assert_eq!(config.max_output_burst_bytes, 16384);
        assert_eq!(config.max_ws_write_buffer_bytes, 32768);
        assert_eq!(config.default_size, GridSize::new(30, 100));
    }

    #[test]
    fn parse_config_rejects_invalid_protocol_limits() {
        assert!(parse_config([
            "--once".to_owned(),
            "--write-timeout-ms".to_owned(),
            "0".to_owned()
        ])
        .is_err());

        assert!(parse_config([
            "--once".to_owned(),
            "--max-server-frame-bytes".to_owned(),
            "2048".to_owned(),
            "--max-output-burst-bytes".to_owned(),
            "1024".to_owned()
        ])
        .is_err());
    }

    #[test]
    fn validate_config_rejects_local_pty_config_with_raw_program_args() {
        let mut local_pty_config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
        local_pty_config.args(["example.com"]);
        let config = GatewayConfig {
            program: Some("/bin/sh".to_owned()),
            local_pty_config: Some(local_pty_config),
            ..GatewayConfig::default()
        };

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn websocket_config_uses_gateway_frame_and_buffer_limits() {
        let config = GatewayConfig {
            max_client_frame_bytes: 4096,
            max_ws_write_buffer_bytes: 8192,
            ..GatewayConfig::default()
        };

        let websocket_config = gateway_websocket_config(&config);

        assert_eq!(websocket_config.max_message_size, Some(4096));
        assert_eq!(websocket_config.max_frame_size, Some(4096));
        assert_eq!(websocket_config.write_buffer_size, 0);
        assert_eq!(websocket_config.max_write_buffer_size, 8192);
    }

    #[test]
    fn bind_validation_rejects_non_loopback_by_default() {
        let config = GatewayConfig {
            bind: "0.0.0.0:8788".to_owned(),
            ..GatewayConfig::default()
        };

        assert!(parse_bind_addr(&config).is_err());
    }

    #[test]
    fn bind_validation_allows_non_loopback_with_explicit_flag() {
        let config = GatewayConfig {
            bind: "0.0.0.0:8788".to_owned(),
            allow_non_loopback: true,
            ..GatewayConfig::default()
        };

        assert_eq!(
            parse_bind_addr(&config).unwrap(),
            "0.0.0.0:8788".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn listener_validation_allows_ephemeral_loopback_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();

        validate_listener_addr(&listener, &GatewayConfig::default()).unwrap();
    }

    #[test]
    fn handshake_accepts_loopback_origin_and_matching_token() {
        let request = Request::builder()
            .uri("/witty?token=secret")
            .header("Origin", "http://127.0.0.1:8787")
            .body(())
            .unwrap();
        let policy = GatewayHandshakePolicy {
            token: Some("secret".to_owned()),
            allowed_origins: Vec::new(),
        };

        assert!(validate_handshake(&request, Response::new(()), &policy).is_ok());
    }

    #[test]
    fn handshake_rejects_missing_token() {
        let request = Request::builder()
            .uri("/witty")
            .header("Origin", "http://127.0.0.1:8787")
            .body(())
            .unwrap();
        let policy = GatewayHandshakePolicy {
            token: Some("secret".to_owned()),
            allowed_origins: Vec::new(),
        };

        assert!(validate_handshake(&request, Response::new(()), &policy).is_err());
    }

    #[test]
    fn handshake_rejects_wrong_token() {
        let request = Request::builder()
            .uri("/witty?token=wrong")
            .header("Origin", "http://127.0.0.1:8787")
            .body(())
            .unwrap();
        let policy = GatewayHandshakePolicy {
            token: Some("secret".to_owned()),
            allowed_origins: Vec::new(),
        };

        assert!(validate_handshake(&request, Response::new(()), &policy).is_err());
    }

    #[test]
    fn handshake_rejects_non_loopback_origin_by_default() {
        let request = Request::builder()
            .uri("/witty")
            .header("Origin", "http://example.invalid")
            .body(())
            .unwrap();
        let policy = GatewayHandshakePolicy {
            token: None,
            allowed_origins: Vec::new(),
        };

        assert!(validate_handshake(&request, Response::new(()), &policy).is_err());
    }

    #[test]
    fn handshake_requires_exact_origin_when_policy_is_explicit() {
        let allowed_request = Request::builder()
            .uri("/witty")
            .header("Origin", "http://127.0.0.1:3000")
            .body(())
            .unwrap();
        let rejected_request = Request::builder()
            .uri("/witty")
            .header("Origin", "http://127.0.0.1:3001")
            .body(())
            .unwrap();
        let policy = GatewayHandshakePolicy {
            token: None,
            allowed_origins: vec!["http://127.0.0.1:3000".to_owned()],
        };

        assert!(validate_handshake(&allowed_request, Response::new(()), &policy).is_ok());
        assert!(validate_handshake(&rejected_request, Response::new(()), &policy).is_err());
    }

    #[test]
    fn session_rejects_input_before_hello() {
        let mut session = GatewaySession::new(GatewayConfig::default());

        assert!(session
            .handle_client_message(BrowserGatewayClientMessage::input(b"x".to_vec()))
            .is_err());
    }

    #[test]
    fn session_accepts_hello_without_spawning_pty() {
        let mut session = GatewaySession::new(GatewayConfig::default());

        let messages = session
            .handle_client_message(BrowserGatewayClientMessage::Hello {
                protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
            })
            .unwrap();

        assert_eq!(
            messages,
            [BrowserGatewayServerMessage::Ready {
                protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
            }]
        );
        assert!(!session.is_spawned());
    }

    #[test]
    fn session_uses_trusted_local_pty_config_template_for_spawn() {
        let mut local_pty_config = LocalPtyConfig::command(GridSize::new(1, 1), "ssh");
        local_pty_config
            .args(["-tt", "alice@example.com"])
            .env("TERM", "xterm-256color");
        let mut session = GatewaySession::new(GatewayConfig {
            local_pty_config: Some(local_pty_config),
            ..GatewayConfig::default()
        });
        session.size = GridSize::new(40, 120);

        let spawn_config = session.transport_config_for_spawn();

        assert_eq!(spawn_config.size, GridSize::new(40, 120));
        assert_eq!(spawn_config.program.as_deref(), Some("ssh"));
        assert_eq!(spawn_config.args, ["-tt", "alice@example.com"]);
        assert_eq!(
            spawn_config.env,
            [("TERM".to_owned(), "xterm-256color".to_owned())]
        );
    }

    #[test]
    fn session_rejects_oversized_input_payload() {
        let mut session = GatewaySession::new(GatewayConfig {
            max_client_frame_bytes: 1,
            ..GatewayConfig::default()
        });
        session
            .handle_client_message(BrowserGatewayClientMessage::Hello {
                protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
            })
            .unwrap();

        assert!(session
            .handle_client_message(BrowserGatewayClientMessage::input(b"xy".to_vec()))
            .is_err());
    }

    #[test]
    fn server_frame_limit_rejects_oversized_output() {
        let message = BrowserGatewayServerMessage::Output {
            bytes: b"abcdef".to_vec(),
        };
        let frame_len = serialized_server_frame_len(&message).unwrap();
        let mut session = GatewaySession::new(GatewayConfig {
            max_server_frame_bytes: frame_len - 1,
            max_output_burst_bytes: frame_len,
            ..GatewayConfig::default()
        });

        assert!(session
            .push_server_message_for_burst(message, &mut Vec::new(), &mut 0, false)
            .is_err());
    }

    #[test]
    fn output_burst_budget_defers_pending_messages() {
        let first = BrowserGatewayServerMessage::Output {
            bytes: b"a".to_vec(),
        };
        let second = BrowserGatewayServerMessage::Output {
            bytes: b"b".to_vec(),
        };
        let first_len = serialized_server_frame_len(&first).unwrap();
        let second_len = serialized_server_frame_len(&second).unwrap();
        let mut session = GatewaySession::new(GatewayConfig {
            max_server_frame_bytes: first_len.max(second_len),
            max_output_burst_bytes: first_len,
            ..GatewayConfig::default()
        });
        session.pending_server_messages.push_back(first.clone());
        session.pending_server_messages.push_back(second.clone());

        assert_eq!(session.poll_server_messages().unwrap(), [first]);
        assert_eq!(session.pending_server_messages.len(), 1);
        assert_eq!(session.poll_server_messages().unwrap(), [second]);
        assert!(session.pending_server_messages.is_empty());
    }
}
