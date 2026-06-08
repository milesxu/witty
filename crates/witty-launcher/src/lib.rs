//! Native launcher for loopback browser terminal sessions.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use witty_core::DEFAULT_MAX_SCROLLBACK_LINES;
pub use witty_core::{validate_external_url, ExternalUrlError, MAX_EXTERNAL_URL_BYTES};
use witty_gateway::{run_once_on_listener, GatewayConfig};
use witty_transport::{
    apply_openssh_import_preview, default_profile_store_path, edit_profile_store,
    parse_openssh_import_preview, read_profile_store, OpenSshImportApplyReport,
    OpenSshImportConflictPolicy, OpenSshImportReview, OpenSshImportSelection,
    ProfileStoreEditOpenMode, ProfileStoreEditReport, ProfileStoreSummary, ProfileStoreV1,
    SshProfile, SshProfileLaunchability, BROWSER_GATEWAY_PROTOCOL_VERSION,
};

const SESSION_ID_BYTES: usize = 16;
const SESSION_TOKEN_BYTES: usize = 32;
const REQUEST_HEADER_LIMIT: usize = 16 * 1024;
const REQUEST_BODY_LIMIT: usize = 16 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SESSION_CONFIG_TTL_MS: u64 = 180_000;
const SESSION_CONFIG_TTL: Duration = Duration::from_millis(SESSION_CONFIG_TTL_MS);
const WEB_ASSET_MANIFEST_FILE: &str = "asset-manifest.json";
const WEB_ASSET_MANIFEST_SCHEMA: u16 = 1;
const WEB_ASSET_MANIFEST_APP: &str = "witty-web";
const WITTY_WEB_ROOT_ENV: &str = "WITTY_WEB_ROOT";
const DEVELOPMENT_WEB_ROOT: &str = "target/witty-web-dist";
const PROFILE_PICKER_OPENSSH_IMPORT_ACTION_ID: &str = "openssh-config";
const PROFILE_PICKER_OPENSSH_IMPORT_ACTION_LABEL: &str = "OpenSSH Import";
const PROFILE_PICKER_OPENSSH_IMPORT_ACTION_KIND: &str = "openssh_config";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LauncherConfig {
    pub web_root: PathBuf,
    pub ui_bind: String,
    pub gateway_bind: String,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub ssh_profile: Option<SshProfile>,
    pub profile_picker_store_path: Option<PathBuf>,
    pub profile_picker_import_sources: Vec<ProfilePickerImportSource>,
    pub profile_import_review: Option<ProfileImportReviewConfig>,
    pub open_browser: bool,
    pub mouse_selection_override: MouseSelectionOverridePolicy,
    pub max_scrollback_lines: usize,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            web_root: default_web_root(),
            ui_bind: "127.0.0.1:0".to_owned(),
            gateway_bind: "127.0.0.1:0".to_owned(),
            program: None,
            args: Vec::new(),
            ssh_profile: None,
            profile_picker_store_path: None,
            profile_picker_import_sources: Vec::new(),
            profile_import_review: None,
            open_browser: false,
            mouse_selection_override: MouseSelectionOverridePolicy::default(),
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseSelectionOverridePolicy {
    #[default]
    ShiftSelect,
    Disabled,
}

impl MouseSelectionOverridePolicy {
    pub fn parse_config_value(value: &str) -> Result<Self> {
        match value {
            "shift-select" => Ok(Self::ShiftSelect),
            "disabled" => Ok(Self::Disabled),
            _ => bail!(
                "unknown mouse selection override policy {value:?}; expected shift-select or disabled"
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchSession {
    pub id: String,
    pub token: String,
    pub ui_origin: String,
    pub ui_url: String,
    pub gateway_url: String,
    pub mouse_selection_override: MouseSelectionOverridePolicy,
    pub max_scrollback_lines: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSessionConfig {
    pub protocol: u16,
    pub gateway_url: String,
    pub token: String,
    pub mouse_selection_override: MouseSelectionOverridePolicy,
    pub scrollback_lines: usize,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePickerSession {
    pub id: String,
    pub ui_token: String,
    pub ui_origin: String,
    pub ui_url: String,
    ui_addr: SocketAddr,
    store_path: PathBuf,
    pub summary: ProfileStoreSummary,
    import_sources: Vec<ProfilePickerImportSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePickerImportSource {
    pub id: String,
    pub config_path: PathBuf,
}

impl ProfilePickerImportSource {
    pub fn openssh_config(config_path: impl Into<PathBuf>) -> Self {
        Self {
            id: PROFILE_PICKER_OPENSSH_IMPORT_ACTION_ID.to_owned(),
            config_path: config_path.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileImportReviewConfig {
    pub config_path: PathBuf,
    pub store_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileImportReviewSession {
    pub id: String,
    pub ui_token: String,
    pub ui_origin: String,
    pub ui_url: String,
    ui_addr: SocketAddr,
    config_path: PathBuf,
    store_path: PathBuf,
    pub review: OpenSshImportReview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePickerBootstrap {
    pub kind: String,
    pub protocol: u16,
    pub ui_token: String,
    pub selection_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_url: Option<String>,
    pub expires_at_ms: u64,
    pub summary: ProfileStoreSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import_actions: Vec<ProfilePickerImportAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePickerImportAction {
    pub id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileImportReviewBootstrap {
    pub kind: String,
    pub protocol: u16,
    pub ui_token: String,
    pub confirm_url: String,
    pub expires_at_ms: u64,
    pub review: OpenSshImportReview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ProfilePickerSelectionRequest {
    ui_token: String,
    profile_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ProfilePickerImportRequest {
    ui_token: String,
    action_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfilePickerImportEntry {
    kind: String,
    protocol: u16,
    import_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ProfileImportConfirmRequest {
    ui_token: String,
    profile_ids: Vec<String>,
    conflict: OpenSshImportConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileImportConfirmReport {
    pub changed: bool,
    pub profiles: usize,
    pub default_changed: bool,
    pub bytes: usize,
    pub created_parent_dir: bool,
    pub selected: usize,
    pub added: usize,
    pub replaced: usize,
    pub warning_count: usize,
    pub global_warning_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_picker_url: Option<String>,
}

impl ProfilePickerSession {
    pub fn ui_addr(&self) -> SocketAddr {
        self.ui_addr
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn import_sources(&self) -> &[ProfilePickerImportSource] {
        &self.import_sources
    }
}

impl ProfileImportReviewSession {
    pub fn ui_addr(&self) -> SocketAddr {
        self.ui_addr
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }
}

pub fn run_cli(args: impl IntoIterator<Item = String>) -> Result<()> {
    let config = parse_config(args)?;
    run(config)
}

pub fn parse_config(args: impl IntoIterator<Item = String>) -> Result<LauncherConfig> {
    parse_config_with_default_profile_store_path(args, default_profile_store_path)
}

fn parse_config_with_default_profile_store_path(
    args: impl IntoIterator<Item = String>,
    default_profile_store: impl Fn() -> Result<PathBuf>,
) -> Result<LauncherConfig> {
    let mut config = LauncherConfig::default();
    let mut profile_store_path = None;
    let mut ssh_profile_id = None;
    let mut profile_picker = false;
    let mut profile_picker_import_config_path = None;
    let mut profile_import_config_path = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--web-root" => {
                config.web_root = PathBuf::from(args.next().context("--web-root requires a path")?);
            }
            "--ui-bind" => {
                config.ui_bind = args
                    .next()
                    .context("--ui-bind requires an address like 127.0.0.1:0")?;
            }
            "--gateway-bind" => {
                config.gateway_bind = args
                    .next()
                    .context("--gateway-bind requires an address like 127.0.0.1:0")?;
            }
            "--program" => {
                config.program = Some(args.next().context("--program requires a path")?);
            }
            "--arg" => {
                config
                    .args
                    .push(args.next().context("--arg requires a value")?);
            }
            "--ssh-profile-json" => {
                if config.ssh_profile.is_some() {
                    bail!("only one --ssh-profile-json value is allowed");
                }
                let path = PathBuf::from(
                    args.next()
                        .context("--ssh-profile-json requires a profile JSON path")?,
                );
                config.ssh_profile = Some(load_ssh_profile_json(&path)?);
            }
            "--profile-store" => {
                if profile_store_path.is_some() {
                    bail!("only one --profile-store value is allowed");
                }
                profile_store_path = Some(PathBuf::from(
                    args.next()
                        .context("--profile-store requires a profile store JSON path")?,
                ));
            }
            "--ssh-profile-id" => {
                if ssh_profile_id.is_some() {
                    bail!("only one --ssh-profile-id value is allowed");
                }
                ssh_profile_id = Some(
                    args.next()
                        .context("--ssh-profile-id requires a profile id")?,
                );
            }
            "--profile-picker" => {
                if profile_picker {
                    bail!("only one --profile-picker flag is allowed");
                }
                profile_picker = true;
            }
            "--profile-picker-import-openssh" => {
                if profile_picker_import_config_path.is_some() {
                    bail!("only one --profile-picker-import-openssh value is allowed");
                }
                profile_picker_import_config_path =
                    Some(PathBuf::from(args.next().context(
                        "--profile-picker-import-openssh requires an OpenSSH config path",
                    )?));
            }
            "--profile-import-openssh" => {
                if profile_import_config_path.is_some() {
                    bail!("only one --profile-import-openssh value is allowed");
                }
                profile_import_config_path = Some(PathBuf::from(
                    args.next()
                        .context("--profile-import-openssh requires an OpenSSH config path")?,
                ));
            }
            "--open-browser" => config.open_browser = true,
            "--mouse-selection-override" => {
                let value = args
                    .next()
                    .context("--mouse-selection-override requires a value")?;
                config.mouse_selection_override =
                    MouseSelectionOverridePolicy::parse_config_value(&value)?;
            }
            "--scrollback-lines" => {
                let value = args.next().context("--scrollback-lines requires a value")?;
                config.max_scrollback_lines = value
                    .parse::<usize>()
                    .context("--scrollback-lines must be an integer")?;
            }
            _ => bail!("unknown argument {arg}"),
        }
    }

    if profile_picker && profile_import_config_path.is_some() {
        bail!("--profile-picker cannot be combined with --profile-import-openssh");
    }
    if !profile_picker && profile_picker_import_config_path.is_some() {
        bail!("--profile-picker-import-openssh requires --profile-picker");
    }

    if profile_picker {
        if config.ssh_profile.is_some() {
            bail!("--profile-picker cannot be combined with --ssh-profile-json");
        }
        if ssh_profile_id.is_some() {
            bail!("--profile-picker cannot be combined with --ssh-profile-id");
        }
        if config.program.is_some() || !config.args.is_empty() {
            bail!("--profile-picker cannot be combined with --program or --arg");
        }
        config.profile_picker_store_path = Some(match profile_store_path {
            Some(path) => path,
            None => default_profile_store().context(
                "--profile-picker without --profile-store requires a default profile store path",
            )?,
        });
        if let Some(config_path) = profile_picker_import_config_path {
            config
                .profile_picker_import_sources
                .push(ProfilePickerImportSource::openssh_config(config_path));
        }
    } else if let Some(config_path) = profile_import_config_path {
        if config.ssh_profile.is_some() {
            bail!("--profile-import-openssh cannot be combined with --ssh-profile-json");
        }
        if ssh_profile_id.is_some() {
            bail!("--profile-import-openssh cannot be combined with --ssh-profile-id");
        }
        if config.program.is_some() || !config.args.is_empty() {
            bail!("--profile-import-openssh cannot be combined with --program or --arg");
        }
        let store_path = match profile_store_path {
            Some(path) => path,
            None => default_profile_store().context(
                "--profile-import-openssh without --profile-store requires a default profile store path",
            )?,
        };
        config.profile_import_review = Some(ProfileImportReviewConfig {
            config_path,
            store_path,
        });
    } else {
        if config.ssh_profile.is_some()
            && (profile_store_path.is_some() || ssh_profile_id.is_some())
        {
            bail!("--ssh-profile-json cannot be combined with --profile-store or --ssh-profile-id");
        }
        match (profile_store_path, ssh_profile_id) {
            (Some(path), Some(profile_id)) => {
                config.ssh_profile = Some(load_ssh_profile_from_store(&path, &profile_id)?);
            }
            (Some(_), None) => bail!("--profile-store requires --ssh-profile-id"),
            (None, Some(profile_id)) => {
                let path = default_profile_store().context(
                    "--ssh-profile-id without --profile-store requires a default profile store path",
                )?;
                config.ssh_profile = Some(load_ssh_profile_from_store(&path, &profile_id)?);
            }
            (None, None) => {}
        }
    }

    validate_config(&config)?;
    Ok(config)
}

pub fn run(config: LauncherConfig) -> Result<()> {
    validate_config(&config)?;
    if config.profile_import_review.is_some() {
        return run_profile_import_review(config);
    }
    if config.profile_picker_store_path.is_some() {
        return run_profile_picker(config);
    }

    let web_assets = WebAssets::load(config.web_root.clone())?;
    let ui_listener = TcpListener::bind(&config.ui_bind)
        .with_context(|| format!("bind launcher UI at {}", config.ui_bind))?;
    let gateway_listener = TcpListener::bind(&config.gateway_bind)
        .with_context(|| format!("bind launcher gateway at {}", config.gateway_bind))?;
    let gateway_addr = gateway_listener.local_addr()?;
    let session = build_launch_session_with_policy(
        ui_listener.local_addr()?,
        gateway_addr,
        config.mouse_selection_override,
        config.max_scrollback_lines,
    )?;
    let gateway_config = gateway_config_for_session(&config, &session, gateway_addr)?;
    let session_config = Arc::new(SessionConfigState::new(session, SESSION_CONFIG_TTL));
    let ui_url = session_config.session().ui_url.clone();

    let gateway_done = Arc::new(AtomicBool::new(false));
    let gateway_done_for_thread = Arc::clone(&gateway_done);
    let gateway_thread = thread::spawn(move || {
        let result = run_once_on_listener(gateway_listener, gateway_config);
        if let Err(error) = &result {
            eprintln!("Witty gateway stopped with error: {error:#}");
        }
        gateway_done_for_thread.store(true, Ordering::SeqCst);
        result
    });

    eprintln!("Witty launcher listening on {ui_url}");
    if config.open_browser {
        open_browser(&ui_url)?;
    }
    serve_http_loop(
        ui_listener,
        web_assets,
        LauncherHttpState::Direct(session_config),
        gateway_done,
    )?;

    if let Err(error) = gateway_thread
        .join()
        .unwrap_or_else(|_| Err(anyhow!("gateway thread panicked")))
    {
        eprintln!("Witty gateway thread ended: {error:#}");
    }

    Ok(())
}

fn run_profile_picker(config: LauncherConfig) -> Result<()> {
    let store_path = config
        .profile_picker_store_path
        .clone()
        .context("profile picker mode requires a profile store path")?;
    let summary = read_profile_store(&store_path)
        .with_context(|| format!("read profile store for picker {}", store_path.display()))?
        .redacted_summary()
        .context("build redacted profile store summary")?;
    let web_assets = WebAssets::load(config.web_root.clone())?;
    let ui_listener = TcpListener::bind(&config.ui_bind)
        .with_context(|| format!("bind launcher UI at {}", config.ui_bind))?;
    let gateway_listener = TcpListener::bind(&config.gateway_bind)
        .with_context(|| format!("bind launcher gateway at {}", config.gateway_bind))?;
    let gateway_done = Arc::new(AtomicBool::new(false));
    let profile_picker_runtime = ProfilePickerRuntime::new(
        gateway_listener,
        Arc::clone(&gateway_done),
        config.mouse_selection_override,
        config.max_scrollback_lines,
    )?;
    let session = build_profile_picker_session(
        ui_listener.local_addr()?,
        store_path,
        summary,
        config.profile_picker_import_sources.clone(),
    )?;
    let ui_url = session.ui_url.clone();
    let profile_picker = Arc::new(ProfilePickerState::new_with_runtime(
        session,
        SESSION_CONFIG_TTL,
        profile_picker_runtime,
    ));

    eprintln!("Witty profile picker listening on {ui_url}");
    if config.open_browser {
        open_browser(&ui_url)?;
    }

    serve_http_loop(
        ui_listener,
        web_assets,
        LauncherHttpState::ProfilePicker(profile_picker),
        gateway_done,
    )
}

fn run_profile_import_review(config: LauncherConfig) -> Result<()> {
    let review_config = config
        .profile_import_review
        .clone()
        .context("profile import review mode requires import config")?;
    let review =
        build_profile_import_review(&review_config.config_path, &review_config.store_path)?;
    let web_assets = WebAssets::load(config.web_root.clone())?;
    let ui_listener = TcpListener::bind(&config.ui_bind)
        .with_context(|| format!("bind launcher UI at {}", config.ui_bind))?;
    let done = Arc::new(AtomicBool::new(false));
    let session = build_profile_import_review_session(
        ui_listener.local_addr()?,
        review_config.config_path,
        review_config.store_path,
        review,
    )?;
    let ui_url = session.ui_url.clone();
    let import_review = Arc::new(ProfileImportReviewState::new(
        session,
        SESSION_CONFIG_TTL,
        Arc::clone(&done),
    ));

    eprintln!("Witty profile import review listening on {ui_url}");
    if config.open_browser {
        open_browser(&ui_url)?;
    }

    serve_http_loop(
        ui_listener,
        web_assets,
        LauncherHttpState::ProfileImportReview(import_review),
        done,
    )
}

pub fn build_launch_session(
    ui_addr: SocketAddr,
    gateway_addr: SocketAddr,
) -> Result<LaunchSession> {
    build_launch_session_with_policy(
        ui_addr,
        gateway_addr,
        MouseSelectionOverridePolicy::default(),
        DEFAULT_MAX_SCROLLBACK_LINES,
    )
}

pub fn build_launch_session_with_policy(
    ui_addr: SocketAddr,
    gateway_addr: SocketAddr,
    mouse_selection_override: MouseSelectionOverridePolicy,
    max_scrollback_lines: usize,
) -> Result<LaunchSession> {
    if !ui_addr.ip().is_loopback() {
        bail!("launcher UI listener must be loopback, got {ui_addr}");
    }
    if !gateway_addr.ip().is_loopback() {
        bail!("launcher gateway listener must be loopback, got {gateway_addr}");
    }

    let id = random_hex(SESSION_ID_BYTES)?;
    let token = random_hex(SESSION_TOKEN_BYTES)?;
    let ui_authority = socket_authority(ui_addr);
    let gateway_authority = socket_authority(gateway_addr);
    let ui_origin = format!("http://{ui_authority}");
    let ui_url = format!("{ui_origin}/index.html#session={id}");
    let gateway_url = format!("ws://{gateway_authority}/witty");

    Ok(LaunchSession {
        id,
        token,
        ui_origin,
        ui_url,
        gateway_url,
        mouse_selection_override,
        max_scrollback_lines,
    })
}

pub fn build_profile_picker_session(
    ui_addr: SocketAddr,
    store_path: PathBuf,
    summary: ProfileStoreSummary,
    import_sources: Vec<ProfilePickerImportSource>,
) -> Result<ProfilePickerSession> {
    if !ui_addr.ip().is_loopback() {
        bail!("launcher UI listener must be loopback, got {ui_addr}");
    }

    let id = random_hex(SESSION_ID_BYTES)?;
    let ui_token = random_hex(SESSION_TOKEN_BYTES)?;
    let ui_authority = socket_authority(ui_addr);
    let ui_origin = format!("http://{ui_authority}");
    let ui_url = format!("{ui_origin}/index.html#profile_picker={id}");

    Ok(ProfilePickerSession {
        id,
        ui_token,
        ui_origin,
        ui_url,
        ui_addr,
        store_path,
        summary,
        import_sources,
    })
}

pub fn build_profile_import_review_session(
    ui_addr: SocketAddr,
    config_path: PathBuf,
    store_path: PathBuf,
    review: OpenSshImportReview,
) -> Result<ProfileImportReviewSession> {
    if !ui_addr.ip().is_loopback() {
        bail!("launcher UI listener must be loopback, got {ui_addr}");
    }

    let id = random_hex(SESSION_ID_BYTES)?;
    let ui_token = random_hex(SESSION_TOKEN_BYTES)?;
    let ui_authority = socket_authority(ui_addr);
    let ui_origin = format!("http://{ui_authority}");
    let ui_url = format!("{ui_origin}/index.html#profile_import={id}");

    Ok(ProfileImportReviewSession {
        id,
        ui_token,
        ui_origin,
        ui_url,
        ui_addr,
        config_path,
        store_path,
        review,
    })
}

pub fn browser_session_config(session: &LaunchSession) -> BrowserSessionConfig {
    BrowserSessionConfig {
        protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
        gateway_url: session.gateway_url.clone(),
        token: session.token.clone(),
        mouse_selection_override: session.mouse_selection_override,
        scrollback_lines: session.max_scrollback_lines,
        expires_at_ms: SESSION_CONFIG_TTL_MS,
    }
}

pub fn browser_session_config_json(session: &LaunchSession) -> Result<String> {
    serde_json::to_string(&browser_session_config(session)).context("serialize session config")
}

pub fn profile_picker_bootstrap(session: &ProfilePickerSession) -> ProfilePickerBootstrap {
    let import_actions: Vec<_> = session
        .import_sources
        .iter()
        .map(|source| ProfilePickerImportAction {
            id: source.id.clone(),
            kind: PROFILE_PICKER_OPENSSH_IMPORT_ACTION_KIND.to_owned(),
            label: PROFILE_PICKER_OPENSSH_IMPORT_ACTION_LABEL.to_owned(),
        })
        .collect();
    ProfilePickerBootstrap {
        kind: "profile_picker".to_owned(),
        protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
        ui_token: session.ui_token.clone(),
        selection_url: format!("/profile-picker/{}/select", session.id),
        import_url: (!import_actions.is_empty())
            .then(|| format!("/profile-picker/{}/import", session.id)),
        expires_at_ms: SESSION_CONFIG_TTL_MS,
        summary: session.summary.clone(),
        import_actions,
    }
}

pub fn profile_picker_bootstrap_json(session: &ProfilePickerSession) -> Result<String> {
    serde_json::to_string(&profile_picker_bootstrap(session))
        .context("serialize profile picker bootstrap")
}

fn profile_picker_import_entry_json(session: &ProfileImportReviewSession) -> Result<String> {
    serde_json::to_string(&ProfilePickerImportEntry {
        kind: "profile_import_entry".to_owned(),
        protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
        import_url: format!("/index.html#profile_import={}", session.id),
    })
    .context("serialize profile picker import entry")
}

pub fn profile_import_review_bootstrap(
    session: &ProfileImportReviewSession,
) -> ProfileImportReviewBootstrap {
    ProfileImportReviewBootstrap {
        kind: "profile_import".to_owned(),
        protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
        ui_token: session.ui_token.clone(),
        confirm_url: format!("/profile-import/{}/confirm", session.id),
        expires_at_ms: SESSION_CONFIG_TTL_MS,
        review: session.review.clone(),
    }
}

pub fn profile_import_review_bootstrap_json(
    session: &ProfileImportReviewSession,
) -> Result<String> {
    serde_json::to_string(&profile_import_review_bootstrap(session))
        .context("serialize profile import review bootstrap")
}

pub fn gateway_config_for_session(
    config: &LauncherConfig,
    session: &LaunchSession,
    gateway_addr: SocketAddr,
) -> Result<GatewayConfig> {
    let mut gateway = GatewayConfig {
        bind: gateway_addr.to_string(),
        program: config.program.clone(),
        args: config.args.clone(),
        token: Some(session.token.clone()),
        allowed_origins: vec![session.ui_origin.clone()],
        ..GatewayConfig::default()
    };
    if let Some(profile) = &config.ssh_profile {
        gateway.local_pty_config = Some(
            profile
                .to_openssh_profile()
                .context("convert SSH profile to OpenSSH launch profile")?
                .to_local_pty_config(gateway.default_size)
                .context("convert OpenSSH profile to gateway PTY config")?,
        );
    }
    Ok(gateway)
}

pub fn random_hex(byte_count: usize) -> Result<String> {
    let mut bytes = vec![0_u8; byte_count];
    getrandom::fill(&mut bytes).map_err(|error| anyhow!("generate random token: {error}"))?;
    Ok(hex_encode(&bytes))
}

fn load_ssh_profile_json(path: &Path) -> Result<SshProfile> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("read SSH profile JSON {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("parse SSH profile JSON {}", path.display()))
}

fn load_ssh_profile_from_store(path: &Path, profile_id: &str) -> Result<SshProfile> {
    let store = read_profile_store(path)?;
    let profile = store
        .profile(profile_id)
        .with_context(|| format!("SSH profile id {profile_id:?} was not found"))?;

    match profile
        .launchability()
        .with_context(|| format!("validate SSH profile id {profile_id:?}"))?
    {
        SshProfileLaunchability::Launchable => Ok(profile.clone()),
        SshProfileLaunchability::RequiresCredentialResolver => {
            bail!("SSH profile id {profile_id:?} requires a credential resolver")
        }
    }
}

fn load_launchable_profile_for_picker(
    path: &Path,
    profile_id: &str,
) -> std::result::Result<SshProfile, ProfilePickerSelectionUnavailable> {
    let store = read_profile_store(path)
        .map_err(|error| ProfilePickerSelectionUnavailable::Launch(error.to_string()))?;
    let profile = store
        .profile(profile_id)
        .ok_or(ProfilePickerSelectionUnavailable::NotFound)?;

    match profile
        .launchability()
        .map_err(|error| ProfilePickerSelectionUnavailable::Launch(error.to_string()))?
    {
        SshProfileLaunchability::Launchable => Ok(profile.clone()),
        SshProfileLaunchability::RequiresCredentialResolver => {
            Err(ProfilePickerSelectionUnavailable::RequiresCredentialResolver)
        }
    }
}

fn build_profile_import_review(
    config_path: &Path,
    store_path: &Path,
) -> Result<OpenSshImportReview> {
    let config = fs::read_to_string(config_path).with_context(|| {
        format!(
            "read OpenSSH config for import review {}",
            config_path.display()
        )
    })?;
    let mut preview = parse_openssh_import_preview(&config, Some(config_path.to_path_buf()));
    let store = read_profile_store_if_present(store_path)?;
    preview.mark_conflicts_from_store(&store);
    Ok(preview.redacted_review())
}

fn run_profile_import_confirm(
    config_path: &Path,
    store_path: &Path,
    selection: &OpenSshImportSelection,
    conflict_policy: OpenSshImportConflictPolicy,
) -> Result<ProfileImportConfirmReport> {
    let config = fs::read_to_string(config_path).with_context(|| {
        format!(
            "read OpenSSH config for import confirmation {}",
            config_path.display()
        )
    })?;
    let preview = parse_openssh_import_preview(&config, Some(config_path.to_path_buf()));
    let mut apply_report = None;
    let edit_report = edit_profile_store(
        store_path,
        ProfileStoreEditOpenMode::CreateIfMissing,
        |store| {
            let report = apply_openssh_import_preview(store, &preview, selection, conflict_policy)?;
            let mutation = report.mutation;
            apply_report = Some(report);
            Ok(mutation)
        },
    )?;
    let apply_report =
        apply_report.context("profile import confirmation did not produce an apply report")?;

    Ok(profile_import_confirm_report(&edit_report, &apply_report))
}

fn profile_import_confirm_report(
    edit_report: &ProfileStoreEditReport,
    apply_report: &OpenSshImportApplyReport,
) -> ProfileImportConfirmReport {
    ProfileImportConfirmReport {
        changed: edit_report.mutation.changed,
        profiles: edit_report.mutation.profile_count,
        default_changed: edit_report.mutation.default_profile_changed,
        bytes: edit_report.write.bytes_written,
        created_parent_dir: edit_report.write.created_parent_dir,
        selected: apply_report.selected,
        added: apply_report.added,
        replaced: apply_report.replaced,
        warning_count: apply_report.total_warning_count(),
        global_warning_count: apply_report.global_warning_count,
        next_picker_url: None,
    }
}

fn read_profile_store_if_present(path: &Path) -> Result<ProfileStoreV1> {
    if path
        .try_exists()
        .with_context(|| format!("check profile store {}", path.display()))?
    {
        read_profile_store(path)
    } else {
        Ok(ProfileStoreV1::new())
    }
}

fn validate_profile_picker_profile_id(id: &str) -> std::result::Result<(), &'static str> {
    if id.trim().is_empty() {
        return Err("profile id must not be empty");
    }
    if id.chars().any(char::is_whitespace) {
        return Err("profile id must not contain whitespace");
    }
    if id.chars().any(char::is_control) {
        return Err("profile id must not contain control characters");
    }
    Ok(())
}

fn validate_profile_picker_action_id(id: &str) -> std::result::Result<(), &'static str> {
    if id.trim().is_empty() {
        return Err("profile picker action id must not be empty");
    }
    if id.chars().any(char::is_whitespace) {
        return Err("profile picker action id must not contain whitespace");
    }
    if id.chars().any(char::is_control) {
        return Err("profile picker action id must not contain control characters");
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserOpenCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn browser_open_command(url: &str) -> BrowserOpenCommand {
    #[cfg(target_os = "macos")]
    {
        BrowserOpenCommand {
            program: "open".to_owned(),
            args: vec![url.to_owned()],
        }
    }

    #[cfg(target_os = "windows")]
    {
        BrowserOpenCommand {
            program: "cmd".to_owned(),
            args: vec![
                "/C".to_owned(),
                "start".to_owned(),
                "".to_owned(),
                url.to_owned(),
            ],
        }
    }

    #[cfg(all(target_family = "unix", not(target_os = "macos")))]
    {
        BrowserOpenCommand {
            program: "xdg-open".to_owned(),
            args: vec![url.to_owned()],
        }
    }

    #[cfg(not(any(target_family = "unix", target_os = "windows")))]
    {
        let _ = url;
        BrowserOpenCommand {
            program: String::new(),
            args: Vec::new(),
        }
    }
}

pub fn external_url_open_command(uri: &str) -> Result<BrowserOpenCommand> {
    validate_external_url(uri).map_err(|error| anyhow!("URL is not allowed: {error}"))?;
    Ok(browser_open_command(uri))
}

pub fn open_external_url(uri: &str) -> Result<()> {
    let command = external_url_open_command(uri)?;
    spawn_open_command(&command)
}

fn open_browser(url: &str) -> Result<()> {
    open_external_url(url)
}

fn spawn_open_command(command: &BrowserOpenCommand) -> Result<()> {
    if command.program.is_empty() {
        bail!("opening external URLs is not supported on this platform");
    }
    Command::new(&command.program)
        .args(&command.args)
        .spawn()
        .with_context(|| format!("open external URL with {}", command.program))?;
    Ok(())
}

struct SessionConfigState {
    session: LaunchSession,
    ttl: Duration,
    created_at: Instant,
    served: AtomicBool,
}

impl SessionConfigState {
    fn new(session: LaunchSession, ttl: Duration) -> Self {
        Self {
            session,
            ttl,
            created_at: Instant::now(),
            served: AtomicBool::new(false),
        }
    }

    fn session(&self) -> &LaunchSession {
        &self.session
    }

    fn take_config_json(&self) -> std::result::Result<String, SessionConfigUnavailable> {
        if self.created_at.elapsed() >= self.ttl {
            return Err(SessionConfigUnavailable::Expired);
        }
        if self.served.swap(true, Ordering::SeqCst) {
            return Err(SessionConfigUnavailable::Used);
        }
        browser_session_config_json(&self.session)
            .map_err(|error| SessionConfigUnavailable::Serialize(error.to_string()))
    }
}

type ProfilePickerSessions = Arc<Mutex<BTreeMap<String, Arc<ProfilePickerSessionState>>>>;
type ProfileImportReviews = Arc<Mutex<BTreeMap<String, Arc<ProfileImportReviewState>>>>;

struct ProfilePickerSessionState {
    session: ProfilePickerSession,
    created_at: Instant,
    served: AtomicBool,
    selected: AtomicBool,
}

impl ProfilePickerSessionState {
    fn new(session: ProfilePickerSession) -> Self {
        Self {
            session,
            created_at: Instant::now(),
            served: AtomicBool::new(false),
            selected: AtomicBool::new(false),
        }
    }

    fn take_bootstrap_json(
        &self,
        ttl: Duration,
    ) -> std::result::Result<String, ProfilePickerUnavailable> {
        if self.created_at.elapsed() >= ttl {
            return Err(ProfilePickerUnavailable::Expired);
        }
        if self.served.swap(true, Ordering::SeqCst) {
            return Err(ProfilePickerUnavailable::Used);
        }
        profile_picker_bootstrap_json(&self.session)
            .map_err(|error| ProfilePickerUnavailable::Serialize(error.to_string()))
    }
}

struct ProfilePickerState {
    #[cfg(test)]
    initial_session_id: String,
    ttl: Duration,
    sessions: ProfilePickerSessions,
    runtime: Option<ProfilePickerRuntime>,
    import_reviews: ProfileImportReviews,
}

impl ProfilePickerState {
    #[cfg(test)]
    fn new(session: ProfilePickerSession, ttl: Duration) -> Self {
        Self::new_with_optional_runtime(session, ttl, None)
    }

    fn new_with_runtime(
        session: ProfilePickerSession,
        ttl: Duration,
        runtime: ProfilePickerRuntime,
    ) -> Self {
        Self::new_with_optional_runtime(session, ttl, Some(runtime))
    }

    fn new_with_optional_runtime(
        session: ProfilePickerSession,
        ttl: Duration,
        runtime: Option<ProfilePickerRuntime>,
    ) -> Self {
        let session_id = session.id.clone();
        #[cfg(test)]
        let initial_session_id = session_id.clone();
        let mut sessions = BTreeMap::new();
        sessions.insert(
            session_id,
            Arc::new(ProfilePickerSessionState::new(session)),
        );
        Self {
            #[cfg(test)]
            initial_session_id,
            ttl,
            sessions: Arc::new(Mutex::new(sessions)),
            runtime,
            import_reviews: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn picker_session(&self, id: &str) -> Option<Arc<ProfilePickerSessionState>> {
        self.sessions.lock().ok()?.get(id).map(Arc::clone)
    }

    fn import_review(&self, id: &str) -> Option<Arc<ProfileImportReviewState>> {
        self.import_reviews.lock().ok()?.get(id).map(Arc::clone)
    }

    #[cfg(test)]
    fn take_bootstrap_json(&self) -> std::result::Result<String, ProfilePickerUnavailable> {
        self.take_bootstrap_json_for_id(&self.initial_session_id)
    }

    fn take_bootstrap_json_for_id(
        &self,
        id: &str,
    ) -> std::result::Result<String, ProfilePickerUnavailable> {
        let session = self
            .picker_session(id)
            .ok_or(ProfilePickerUnavailable::Expired)?;
        session.take_bootstrap_json(self.ttl)
    }

    #[cfg(test)]
    fn select_profile_json(
        &self,
        body: &[u8],
    ) -> std::result::Result<String, ProfilePickerSelectionUnavailable> {
        self.select_profile_json_for_id(&self.initial_session_id, body)
    }

    fn select_profile_json_for_id(
        &self,
        id: &str,
        body: &[u8],
    ) -> std::result::Result<String, ProfilePickerSelectionUnavailable> {
        if body.is_empty() {
            return Err(ProfilePickerSelectionUnavailable::BadRequest(
                "profile picker selection request body is required".to_owned(),
            ));
        }
        let selection: ProfilePickerSelectionRequest =
            serde_json::from_slice(body).map_err(|_| {
                ProfilePickerSelectionUnavailable::BadRequest(
                    "profile picker selection JSON is malformed".to_owned(),
                )
            })?;
        let session = self
            .picker_session(id)
            .ok_or(ProfilePickerSelectionUnavailable::Expired)?;
        self.select_profile(&session, &selection)
    }

    fn select_profile(
        &self,
        session_state: &ProfilePickerSessionState,
        selection: &ProfilePickerSelectionRequest,
    ) -> std::result::Result<String, ProfilePickerSelectionUnavailable> {
        if session_state.created_at.elapsed() >= self.ttl {
            return Err(ProfilePickerSelectionUnavailable::Expired);
        }
        if selection.ui_token != session_state.session.ui_token {
            return Err(ProfilePickerSelectionUnavailable::Unauthorized);
        }
        validate_profile_picker_profile_id(&selection.profile_id)
            .map_err(|message| ProfilePickerSelectionUnavailable::BadRequest(message.to_owned()))?;

        let profile = load_launchable_profile_for_picker(
            session_state.session.store_path(),
            &selection.profile_id,
        )?;
        let runtime = self
            .runtime
            .as_ref()
            .ok_or(ProfilePickerSelectionUnavailable::RuntimeUnavailable)?;
        if session_state
            .selected
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ProfilePickerSelectionUnavailable::AlreadySelected);
        }

        let session = match runtime.start_selected_session(&session_state.session, profile) {
            Ok(session) => session,
            Err(error) => {
                session_state.selected.store(false, Ordering::SeqCst);
                return Err(ProfilePickerSelectionUnavailable::Launch(error.to_string()));
            }
        };
        browser_session_config_json(&session)
            .map_err(|error| ProfilePickerSelectionUnavailable::Serialize(error.to_string()))
    }

    #[cfg(test)]
    fn start_import_json(
        &self,
        body: &[u8],
    ) -> std::result::Result<String, ProfilePickerImportUnavailable> {
        self.start_import_json_for_id(&self.initial_session_id, body)
    }

    fn start_import_json_for_id(
        &self,
        id: &str,
        body: &[u8],
    ) -> std::result::Result<String, ProfilePickerImportUnavailable> {
        if body.is_empty() {
            return Err(ProfilePickerImportUnavailable::BadRequest(
                "profile picker import request body is required".to_owned(),
            ));
        }
        let request: ProfilePickerImportRequest = serde_json::from_slice(body).map_err(|_| {
            ProfilePickerImportUnavailable::BadRequest(
                "profile picker import JSON is malformed".to_owned(),
            )
        })?;
        let session = self
            .picker_session(id)
            .ok_or(ProfilePickerImportUnavailable::Expired)?;
        self.start_import(&session, &request)
    }

    fn start_import(
        &self,
        session_state: &ProfilePickerSessionState,
        request: &ProfilePickerImportRequest,
    ) -> std::result::Result<String, ProfilePickerImportUnavailable> {
        if session_state.created_at.elapsed() >= self.ttl {
            return Err(ProfilePickerImportUnavailable::Expired);
        }
        if request.ui_token != session_state.session.ui_token {
            return Err(ProfilePickerImportUnavailable::Unauthorized);
        }
        validate_profile_picker_action_id(&request.action_id)
            .map_err(|message| ProfilePickerImportUnavailable::BadRequest(message.to_owned()))?;
        let source = session_state
            .session
            .import_sources()
            .iter()
            .find(|source| source.id == request.action_id)
            .cloned()
            .ok_or(ProfilePickerImportUnavailable::NotFound)?;
        if session_state
            .selected
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ProfilePickerImportUnavailable::AlreadyUsed);
        }

        let review = match build_profile_import_review(
            &source.config_path,
            session_state.session.store_path(),
        ) {
            Ok(review) => review,
            Err(error) => {
                session_state.selected.store(false, Ordering::SeqCst);
                return Err(ProfilePickerImportUnavailable::Build(error.to_string()));
            }
        };
        let session = build_profile_import_review_session(
            session_state.session.ui_addr(),
            source.config_path,
            session_state.session.store_path().to_path_buf(),
            review,
        )
        .map_err(|error| {
            session_state.selected.store(false, Ordering::SeqCst);
            ProfilePickerImportUnavailable::Build(error.to_string())
        })?;
        let json = profile_picker_import_entry_json(&session).map_err(|error| {
            session_state.selected.store(false, Ordering::SeqCst);
            ProfilePickerImportUnavailable::Serialize(error.to_string())
        })?;
        let import_review = Arc::new(ProfileImportReviewState::new_with_picker_refresh(
            session,
            SESSION_CONFIG_TTL,
            ProfileImportPickerRefresh {
                ui_addr: session_state.session.ui_addr(),
                store_path: session_state.session.store_path().to_path_buf(),
                import_sources: session_state.session.import_sources().to_vec(),
                picker_sessions: Arc::clone(&self.sessions),
            },
        ));
        let mut active = self.import_reviews.lock().map_err(|_| {
            session_state.selected.store(false, Ordering::SeqCst);
            ProfilePickerImportUnavailable::Build(
                "profile picker import review lock was poisoned".to_owned(),
            )
        })?;
        active.insert(import_review.session().id.clone(), import_review);
        Ok(json)
    }

    #[cfg(test)]
    fn active_import_review(&self) -> Option<Arc<ProfileImportReviewState>> {
        self.import_reviews
            .lock()
            .ok()
            .and_then(|active| active.values().last().map(Arc::clone))
    }
}

struct ProfileImportPickerRefresh {
    ui_addr: SocketAddr,
    store_path: PathBuf,
    import_sources: Vec<ProfilePickerImportSource>,
    picker_sessions: ProfilePickerSessions,
}

impl ProfileImportPickerRefresh {
    fn create_next_picker_url(&self) -> Result<String> {
        let store = read_profile_store_if_present(&self.store_path)?;
        let summary = store
            .redacted_summary()
            .context("build refreshed profile picker summary")?;
        let session = build_profile_picker_session(
            self.ui_addr,
            self.store_path.clone(),
            summary,
            self.import_sources.clone(),
        )?;
        let next_picker_url = format!("/index.html#profile_picker={}", session.id);
        let mut sessions = self
            .picker_sessions
            .lock()
            .map_err(|_| anyhow!("profile picker session lock was poisoned"))?;
        sessions.insert(
            session.id.clone(),
            Arc::new(ProfilePickerSessionState::new(session)),
        );
        Ok(next_picker_url)
    }
}

enum ProfileImportPostConfirm {
    Finish(Arc<AtomicBool>),
    RefreshPicker(ProfileImportPickerRefresh),
}

struct ProfileImportReviewState {
    session: ProfileImportReviewSession,
    ttl: Duration,
    created_at: Instant,
    served: AtomicBool,
    confirmed: AtomicBool,
    post_confirm: ProfileImportPostConfirm,
}

impl ProfileImportReviewState {
    fn new(session: ProfileImportReviewSession, ttl: Duration, done: Arc<AtomicBool>) -> Self {
        Self {
            session,
            ttl,
            created_at: Instant::now(),
            served: AtomicBool::new(false),
            confirmed: AtomicBool::new(false),
            post_confirm: ProfileImportPostConfirm::Finish(done),
        }
    }

    fn new_with_picker_refresh(
        session: ProfileImportReviewSession,
        ttl: Duration,
        refresh: ProfileImportPickerRefresh,
    ) -> Self {
        Self {
            session,
            ttl,
            created_at: Instant::now(),
            served: AtomicBool::new(false),
            confirmed: AtomicBool::new(false),
            post_confirm: ProfileImportPostConfirm::RefreshPicker(refresh),
        }
    }

    fn session(&self) -> &ProfileImportReviewSession {
        &self.session
    }

    fn take_bootstrap_json(&self) -> std::result::Result<String, ProfileImportReviewUnavailable> {
        if self.created_at.elapsed() >= self.ttl {
            return Err(ProfileImportReviewUnavailable::Expired);
        }
        if self.served.swap(true, Ordering::SeqCst) {
            return Err(ProfileImportReviewUnavailable::Used);
        }
        let json = profile_import_review_bootstrap_json(&self.session)
            .map_err(|error| ProfileImportReviewUnavailable::Serialize(error.to_string()))?;
        Ok(json)
    }

    fn confirm_json(
        &self,
        body: &[u8],
    ) -> std::result::Result<String, ProfileImportConfirmUnavailable> {
        if body.is_empty() {
            return Err(ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation request body is required".to_owned(),
            ));
        }
        let confirm: ProfileImportConfirmRequest = serde_json::from_slice(body).map_err(|_| {
            ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation JSON is malformed".to_owned(),
            )
        })?;
        self.confirm(&confirm)
    }

    fn confirm(
        &self,
        confirm: &ProfileImportConfirmRequest,
    ) -> std::result::Result<String, ProfileImportConfirmUnavailable> {
        if self.created_at.elapsed() >= self.ttl {
            return Err(ProfileImportConfirmUnavailable::Expired);
        }
        if confirm.ui_token != self.session.ui_token {
            return Err(ProfileImportConfirmUnavailable::Unauthorized);
        }
        validate_profile_import_confirm_request(confirm, &self.session.review)?;
        if self
            .confirmed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ProfileImportConfirmUnavailable::Used);
        }

        let selection = OpenSshImportSelection::profile_ids(confirm.profile_ids.clone());
        let mut report = match run_profile_import_confirm(
            self.session.config_path(),
            self.session.store_path(),
            &selection,
            confirm.conflict,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.confirmed.store(false, Ordering::SeqCst);
                return Err(ProfileImportConfirmUnavailable::Apply(error.to_string()));
            }
        };
        match &self.post_confirm {
            ProfileImportPostConfirm::Finish(done) => {
                done.store(true, Ordering::SeqCst);
            }
            ProfileImportPostConfirm::RefreshPicker(refresh) => {
                report.next_picker_url =
                    Some(refresh.create_next_picker_url().map_err(|error| {
                        ProfileImportConfirmUnavailable::Continue(error.to_string())
                    })?);
            }
        }
        let json = serde_json::to_string(&report)
            .map_err(|error| ProfileImportConfirmUnavailable::Serialize(error.to_string()))?;
        Ok(json)
    }
}

fn validate_profile_import_confirm_request(
    confirm: &ProfileImportConfirmRequest,
    review: &OpenSshImportReview,
) -> std::result::Result<(), ProfileImportConfirmUnavailable> {
    if confirm.profile_ids.is_empty() {
        return Err(ProfileImportConfirmUnavailable::BadRequest(
            "profile import confirmation requires at least one profile id".to_owned(),
        ));
    }

    let mut candidates = BTreeMap::<&str, (usize, bool)>::new();
    for candidate in &review.candidates {
        let entry = candidates
            .entry(candidate.id.as_str())
            .or_insert((0, candidate.has_conflict));
        entry.0 += 1;
        entry.1 |= candidate.has_conflict;
    }

    let mut selected = BTreeSet::new();
    for id in &confirm.profile_ids {
        validate_profile_picker_profile_id(id)
            .map_err(|message| ProfileImportConfirmUnavailable::BadRequest(message.to_owned()))?;
        if !selected.insert(id.as_str()) {
            return Err(ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation contains duplicate profile ids".to_owned(),
            ));
        }
        let Some((candidate_count, has_conflict)) = candidates.get(id.as_str()) else {
            return Err(ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation contains unknown profile ids".to_owned(),
            ));
        };
        if *candidate_count != 1 {
            return Err(ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation contains duplicate candidate ids".to_owned(),
            ));
        }
        if confirm.conflict == OpenSshImportConflictPolicy::Reject && *has_conflict {
            return Err(ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation reject policy cannot select conflicting profile ids"
                    .to_owned(),
            ));
        }
    }

    Ok(())
}

struct ProfilePickerRuntime {
    gateway_listener: Mutex<Option<TcpListener>>,
    gateway_done: Arc<AtomicBool>,
    mouse_selection_override: MouseSelectionOverridePolicy,
    max_scrollback_lines: usize,
}

impl ProfilePickerRuntime {
    fn new(
        gateway_listener: TcpListener,
        gateway_done: Arc<AtomicBool>,
        mouse_selection_override: MouseSelectionOverridePolicy,
        max_scrollback_lines: usize,
    ) -> Result<Self> {
        let gateway_addr = gateway_listener
            .local_addr()
            .context("read profile picker gateway listener address")?;
        if !gateway_addr.ip().is_loopback() {
            bail!("launcher gateway listener must be loopback, got {gateway_addr}");
        }
        Ok(Self {
            gateway_listener: Mutex::new(Some(gateway_listener)),
            gateway_done,
            mouse_selection_override,
            max_scrollback_lines,
        })
    }

    fn start_selected_session(
        &self,
        picker_session: &ProfilePickerSession,
        profile: SshProfile,
    ) -> Result<LaunchSession> {
        let gateway_addr = self.gateway_addr()?;
        let session = build_launch_session_with_policy(
            picker_session.ui_addr(),
            gateway_addr,
            self.mouse_selection_override,
            self.max_scrollback_lines,
        )?;
        let launcher = LauncherConfig {
            ssh_profile: Some(profile),
            mouse_selection_override: self.mouse_selection_override,
            max_scrollback_lines: self.max_scrollback_lines,
            ..LauncherConfig::default()
        };
        let gateway_config = gateway_config_for_session(&launcher, &session, gateway_addr)?;
        let gateway_listener = self.take_gateway_listener()?;
        let gateway_done = Arc::clone(&self.gateway_done);
        thread::spawn(move || {
            let result = run_once_on_listener(gateway_listener, gateway_config);
            if let Err(error) = &result {
                eprintln!("Witty gateway stopped with error: {error:#}");
            }
            gateway_done.store(true, Ordering::SeqCst);
        });
        Ok(session)
    }

    fn gateway_addr(&self) -> Result<SocketAddr> {
        let listener = self
            .gateway_listener
            .lock()
            .map_err(|_| anyhow!("profile picker gateway listener lock was poisoned"))?;
        listener
            .as_ref()
            .context("profile picker gateway listener already used")?
            .local_addr()
            .context("read profile picker gateway listener address")
    }

    fn take_gateway_listener(&self) -> Result<TcpListener> {
        let mut listener = self
            .gateway_listener
            .lock()
            .map_err(|_| anyhow!("profile picker gateway listener lock was poisoned"))?;
        listener
            .take()
            .context("profile picker gateway listener already used")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SessionConfigUnavailable {
    Used,
    Expired,
    Serialize(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfilePickerUnavailable {
    Used,
    Expired,
    Serialize(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileImportReviewUnavailable {
    Used,
    Expired,
    Serialize(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileImportConfirmUnavailable {
    BadRequest(String),
    Unauthorized,
    Used,
    Expired,
    Apply(String),
    Continue(String),
    Serialize(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfilePickerSelectionUnavailable {
    BadRequest(String),
    Unauthorized,
    NotFound,
    RequiresCredentialResolver,
    AlreadySelected,
    Expired,
    RuntimeUnavailable,
    Launch(String),
    Serialize(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfilePickerImportUnavailable {
    BadRequest(String),
    Unauthorized,
    NotFound,
    AlreadyUsed,
    Expired,
    Build(String),
    Serialize(String),
}

fn validate_config(config: &LauncherConfig) -> Result<()> {
    if !config.web_root.is_dir() {
        bail!(
            "web root must be an existing directory: {}. Build web assets with scripts/build-witty-web-dist.sh, pass --web-root, or set {}",
            config.web_root.display(),
            WITTY_WEB_ROOT_ENV
        );
    }
    parse_socket_addr("--ui-bind", &config.ui_bind)?;
    parse_socket_addr("--gateway-bind", &config.gateway_bind)?;
    if config.program.is_none() && !config.args.is_empty() {
        bail!("--arg requires --program");
    }
    if config.ssh_profile.is_some() && (config.program.is_some() || !config.args.is_empty()) {
        bail!("SSH profile launch cannot be combined with --program or --arg");
    }
    if config.profile_picker_store_path.is_some()
        && (config.ssh_profile.is_some() || config.program.is_some() || !config.args.is_empty())
    {
        bail!("profile picker cannot be combined with direct launch options");
    }
    if !config.profile_picker_import_sources.is_empty()
        && config.profile_picker_store_path.is_none()
    {
        bail!("profile picker import sources require profile picker mode");
    }
    if config.profile_import_review.is_some()
        && (config.profile_picker_store_path.is_some()
            || !config.profile_picker_import_sources.is_empty()
            || config.ssh_profile.is_some()
            || config.program.is_some()
            || !config.args.is_empty())
    {
        bail!("profile import review cannot be combined with other launch options");
    }
    Ok(())
}

fn default_web_root() -> PathBuf {
    let env_root = env::var_os(WITTY_WEB_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let current_exe = env::current_exe().ok();

    resolve_web_root(None, env_root, current_exe.as_deref())
}

fn resolve_web_root(
    explicit: Option<PathBuf>,
    env_root: Option<PathBuf>,
    current_exe: Option<&Path>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = env_root {
        return path;
    }
    if let Some(path) = current_exe
        .into_iter()
        .flat_map(installed_web_root_candidates)
        .find(|candidate| candidate.is_dir())
    {
        return path;
    }

    PathBuf::from(DEVELOPMENT_WEB_ROOT)
}

fn installed_web_root_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let Some(exe_dir) = current_exe.parent() else {
        return Vec::new();
    };

    let mut candidates = vec![exe_dir.join("share/witty/web")];
    if let Some(install_root) = exe_dir.parent() {
        candidates.push(install_root.join("share/witty/web"));
    }

    candidates
}

fn parse_socket_addr(name: &str, value: &str) -> Result<SocketAddr> {
    value
        .parse::<SocketAddr>()
        .with_context(|| format!("{name} must be an IP socket address, got {value}"))
}

fn serve_http_loop(
    listener: TcpListener,
    web_assets: WebAssets,
    http_state: LauncherHttpState,
    gateway_done: Arc<AtomicBool>,
) -> Result<()> {
    listener
        .set_nonblocking(true)
        .context("set launcher UI listener nonblocking")?;

    while !gateway_done.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = handle_http_connection(stream, &web_assets, &http_state) {
                    eprintln!("Witty launcher HTTP error: {error:#}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => return Err(error).context("accept launcher HTTP connection"),
        }
    }

    Ok(())
}

enum LauncherHttpState {
    Direct(Arc<SessionConfigState>),
    ProfilePicker(Arc<ProfilePickerState>),
    ProfileImportReview(Arc<ProfileImportReviewState>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfilePickerRouteKind {
    Bootstrap,
    Select,
    Import,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileImportRouteKind {
    Bootstrap,
    Confirm,
}

fn profile_picker_route(path: &str) -> Option<(&str, ProfilePickerRouteKind)> {
    let rest = path.strip_prefix("/profile-picker/")?;
    if let Some(id) = rest.strip_suffix(".json") {
        return valid_launcher_session_id(id).then_some((id, ProfilePickerRouteKind::Bootstrap));
    }
    if let Some(id) = rest.strip_suffix("/select") {
        return valid_launcher_session_id(id).then_some((id, ProfilePickerRouteKind::Select));
    }
    if let Some(id) = rest.strip_suffix("/import") {
        return valid_launcher_session_id(id).then_some((id, ProfilePickerRouteKind::Import));
    }
    None
}

fn profile_import_route(path: &str) -> Option<(&str, ProfileImportRouteKind)> {
    let rest = path.strip_prefix("/profile-import/")?;
    if let Some(id) = rest.strip_suffix(".json") {
        return valid_launcher_session_id(id).then_some((id, ProfileImportRouteKind::Bootstrap));
    }
    if let Some(id) = rest.strip_suffix("/confirm") {
        return valid_launcher_session_id(id).then_some((id, ProfileImportRouteKind::Confirm));
    }
    None
}

fn protected_launcher_route_path(path: &str) -> bool {
    path.starts_with("/session/")
        || path.starts_with("/profile-picker/")
        || path.starts_with("/profile-import/")
}

fn valid_launcher_session_id(id: &str) -> bool {
    id.len() == SESSION_ID_BYTES * 2
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn handle_http_connection(
    mut stream: TcpStream,
    web_assets: &WebAssets,
    http_state: &LauncherHttpState,
) -> Result<()> {
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(_) => {
            return write_response(
                &mut stream,
                400,
                "Bad Request",
                "text/plain; charset=utf-8",
                b"bad request",
                true,
            );
        }
    };
    let path = request.path.as_str();

    if let LauncherHttpState::ProfilePicker(profile_picker) = http_state {
        if let Some((import_id, route)) = profile_import_route(path) {
            if let Some(import_review) = profile_picker.import_review(import_id) {
                match route {
                    ProfileImportRouteKind::Bootstrap => {
                        if request.method != "GET" {
                            return write_response(
                                &mut stream,
                                405,
                                "Method Not Allowed",
                                "text/plain; charset=utf-8",
                                b"method not allowed",
                                true,
                            );
                        }
                        return match import_review.take_bootstrap_json() {
                            Ok(body) => write_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                body.as_bytes(),
                                true,
                            ),
                            Err(ProfileImportReviewUnavailable::Used) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile import review bootstrap already used",
                                true,
                            ),
                            Err(ProfileImportReviewUnavailable::Expired) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile import review bootstrap expired",
                                true,
                            ),
                            Err(ProfileImportReviewUnavailable::Serialize(error)) => {
                                write_response(
                                    &mut stream,
                                    500,
                                    "Internal Server Error",
                                    "text/plain; charset=utf-8",
                                    error.as_bytes(),
                                    true,
                                )
                            }
                        };
                    }
                    ProfileImportRouteKind::Confirm => {
                        if request.method != "POST" {
                            return write_response(
                                &mut stream,
                                405,
                                "Method Not Allowed",
                                "text/plain; charset=utf-8",
                                b"method not allowed",
                                true,
                            );
                        }
                        if !request_has_json_content_type(&request) {
                            return write_json_content_type_required(&mut stream);
                        }
                        return match import_review.confirm_json(&request.body) {
                            Ok(body) => write_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                body.as_bytes(),
                                true,
                            ),
                            Err(ProfileImportConfirmUnavailable::BadRequest(message)) => {
                                write_response(
                                    &mut stream,
                                    400,
                                    "Bad Request",
                                    "text/plain; charset=utf-8",
                                    message.as_bytes(),
                                    true,
                                )
                            }
                            Err(ProfileImportConfirmUnavailable::Unauthorized) => write_response(
                                &mut stream,
                                401,
                                "Unauthorized",
                                "text/plain; charset=utf-8",
                                b"invalid profile import token",
                                true,
                            ),
                            Err(ProfileImportConfirmUnavailable::Used) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile import confirmation already used",
                                true,
                            ),
                            Err(ProfileImportConfirmUnavailable::Expired) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile import confirmation expired",
                                true,
                            ),
                            Err(ProfileImportConfirmUnavailable::Apply(_)) => write_response(
                                &mut stream,
                                409,
                                "Conflict",
                                "text/plain; charset=utf-8",
                                b"profile import confirmation failed",
                                true,
                            ),
                            Err(
                                ProfileImportConfirmUnavailable::Continue(_)
                                | ProfileImportConfirmUnavailable::Serialize(_),
                            ) => write_response(
                                &mut stream,
                                500,
                                "Internal Server Error",
                                "text/plain; charset=utf-8",
                                b"profile import confirmation response failed",
                                true,
                            ),
                        };
                    }
                }
            }
        }
    }

    if let LauncherHttpState::ProfilePicker(profile_picker) = http_state {
        if let Some((picker_id, route)) = profile_picker_route(path) {
            if profile_picker.picker_session(picker_id).is_some() {
                match route {
                    ProfilePickerRouteKind::Bootstrap => {
                        if request.method != "GET" {
                            return write_response(
                                &mut stream,
                                405,
                                "Method Not Allowed",
                                "text/plain; charset=utf-8",
                                b"method not allowed",
                                true,
                            );
                        }
                        return match profile_picker.take_bootstrap_json_for_id(picker_id) {
                            Ok(body) => write_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                body.as_bytes(),
                                true,
                            ),
                            Err(ProfilePickerUnavailable::Used) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile picker bootstrap already used",
                                true,
                            ),
                            Err(ProfilePickerUnavailable::Expired) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile picker bootstrap expired",
                                true,
                            ),
                            Err(ProfilePickerUnavailable::Serialize(error)) => write_response(
                                &mut stream,
                                500,
                                "Internal Server Error",
                                "text/plain; charset=utf-8",
                                error.as_bytes(),
                                true,
                            ),
                        };
                    }
                    ProfilePickerRouteKind::Select => {
                        if request.method != "POST" {
                            return write_response(
                                &mut stream,
                                405,
                                "Method Not Allowed",
                                "text/plain; charset=utf-8",
                                b"method not allowed",
                                true,
                            );
                        }
                        if !request_has_json_content_type(&request) {
                            return write_json_content_type_required(&mut stream);
                        }
                        return match profile_picker
                            .select_profile_json_for_id(picker_id, &request.body)
                        {
                            Ok(body) => write_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                body.as_bytes(),
                                true,
                            ),
                            Err(ProfilePickerSelectionUnavailable::BadRequest(message)) => {
                                write_response(
                                    &mut stream,
                                    400,
                                    "Bad Request",
                                    "text/plain; charset=utf-8",
                                    message.as_bytes(),
                                    true,
                                )
                            }
                            Err(ProfilePickerSelectionUnavailable::Unauthorized) => write_response(
                                &mut stream,
                                401,
                                "Unauthorized",
                                "text/plain; charset=utf-8",
                                b"invalid profile picker token",
                                true,
                            ),
                            Err(ProfilePickerSelectionUnavailable::NotFound) => write_response(
                                &mut stream,
                                404,
                                "Not Found",
                                "text/plain; charset=utf-8",
                                b"selected profile was not found",
                                true,
                            ),
                            Err(ProfilePickerSelectionUnavailable::RequiresCredentialResolver) => {
                                write_response(
                                    &mut stream,
                                    409,
                                    "Conflict",
                                    "text/plain; charset=utf-8",
                                    b"selected profile requires a credential resolver",
                                    true,
                                )
                            }
                            Err(ProfilePickerSelectionUnavailable::AlreadySelected) => {
                                write_response(
                                    &mut stream,
                                    409,
                                    "Conflict",
                                    "text/plain; charset=utf-8",
                                    b"profile picker selection already used",
                                    true,
                                )
                            }
                            Err(ProfilePickerSelectionUnavailable::Expired) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile picker selection expired",
                                true,
                            ),
                            Err(
                                ProfilePickerSelectionUnavailable::RuntimeUnavailable
                                | ProfilePickerSelectionUnavailable::Launch(_)
                                | ProfilePickerSelectionUnavailable::Serialize(_),
                            ) => write_response(
                                &mut stream,
                                500,
                                "Internal Server Error",
                                "text/plain; charset=utf-8",
                                b"profile picker selection failed",
                                true,
                            ),
                        };
                    }
                    ProfilePickerRouteKind::Import => {
                        if request.method != "POST" {
                            return write_response(
                                &mut stream,
                                405,
                                "Method Not Allowed",
                                "text/plain; charset=utf-8",
                                b"method not allowed",
                                true,
                            );
                        }
                        if !request_has_json_content_type(&request) {
                            return write_json_content_type_required(&mut stream);
                        }
                        return match profile_picker
                            .start_import_json_for_id(picker_id, &request.body)
                        {
                            Ok(body) => write_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                body.as_bytes(),
                                true,
                            ),
                            Err(ProfilePickerImportUnavailable::BadRequest(message)) => {
                                write_response(
                                    &mut stream,
                                    400,
                                    "Bad Request",
                                    "text/plain; charset=utf-8",
                                    message.as_bytes(),
                                    true,
                                )
                            }
                            Err(ProfilePickerImportUnavailable::Unauthorized) => write_response(
                                &mut stream,
                                401,
                                "Unauthorized",
                                "text/plain; charset=utf-8",
                                b"invalid profile picker token",
                                true,
                            ),
                            Err(ProfilePickerImportUnavailable::NotFound) => write_response(
                                &mut stream,
                                404,
                                "Not Found",
                                "text/plain; charset=utf-8",
                                b"profile picker import action was not found",
                                true,
                            ),
                            Err(ProfilePickerImportUnavailable::AlreadyUsed) => write_response(
                                &mut stream,
                                409,
                                "Conflict",
                                "text/plain; charset=utf-8",
                                b"profile picker already used",
                                true,
                            ),
                            Err(ProfilePickerImportUnavailable::Expired) => write_response(
                                &mut stream,
                                410,
                                "Gone",
                                "text/plain; charset=utf-8",
                                b"profile picker import expired",
                                true,
                            ),
                            Err(
                                ProfilePickerImportUnavailable::Build(_)
                                | ProfilePickerImportUnavailable::Serialize(_),
                            ) => write_response(
                                &mut stream,
                                500,
                                "Internal Server Error",
                                "text/plain; charset=utf-8",
                                b"profile picker import failed",
                                true,
                            ),
                        };
                    }
                }
            }
        }
    }

    match http_state {
        LauncherHttpState::Direct(session_config)
            if request.method == "GET"
                && path == format!("/session/{}.json", session_config.session().id) =>
        {
            return match session_config.take_config_json() {
                Ok(body) => write_response(
                    &mut stream,
                    200,
                    "OK",
                    "application/json",
                    body.as_bytes(),
                    true,
                ),
                Err(SessionConfigUnavailable::Used) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"session config already used",
                    true,
                ),
                Err(SessionConfigUnavailable::Expired) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"session config expired",
                    true,
                ),
                Err(SessionConfigUnavailable::Serialize(error)) => write_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                    true,
                ),
            };
        }
        LauncherHttpState::ProfileImportReview(import_review)
            if request.method == "GET"
                && path == format!("/profile-import/{}.json", import_review.session().id) =>
        {
            return match import_review.take_bootstrap_json() {
                Ok(body) => write_response(
                    &mut stream,
                    200,
                    "OK",
                    "application/json",
                    body.as_bytes(),
                    true,
                ),
                Err(ProfileImportReviewUnavailable::Used) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"profile import review bootstrap already used",
                    true,
                ),
                Err(ProfileImportReviewUnavailable::Expired) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"profile import review bootstrap expired",
                    true,
                ),
                Err(ProfileImportReviewUnavailable::Serialize(error)) => write_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                    true,
                ),
            };
        }
        LauncherHttpState::ProfileImportReview(import_review)
            if request.method == "POST"
                && path == format!("/profile-import/{}/confirm", import_review.session().id) =>
        {
            if !request_has_json_content_type(&request) {
                return write_json_content_type_required(&mut stream);
            }
            return match import_review.confirm_json(&request.body) {
                Ok(body) => write_response(
                    &mut stream,
                    200,
                    "OK",
                    "application/json",
                    body.as_bytes(),
                    true,
                ),
                Err(ProfileImportConfirmUnavailable::BadRequest(message)) => write_response(
                    &mut stream,
                    400,
                    "Bad Request",
                    "text/plain; charset=utf-8",
                    message.as_bytes(),
                    true,
                ),
                Err(ProfileImportConfirmUnavailable::Unauthorized) => write_response(
                    &mut stream,
                    401,
                    "Unauthorized",
                    "text/plain; charset=utf-8",
                    b"invalid profile import token",
                    true,
                ),
                Err(ProfileImportConfirmUnavailable::Used) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"profile import confirmation already used",
                    true,
                ),
                Err(ProfileImportConfirmUnavailable::Expired) => write_response(
                    &mut stream,
                    410,
                    "Gone",
                    "text/plain; charset=utf-8",
                    b"profile import confirmation expired",
                    true,
                ),
                Err(ProfileImportConfirmUnavailable::Apply(_)) => write_response(
                    &mut stream,
                    409,
                    "Conflict",
                    "text/plain; charset=utf-8",
                    b"profile import confirmation failed",
                    true,
                ),
                Err(
                    ProfileImportConfirmUnavailable::Continue(_)
                    | ProfileImportConfirmUnavailable::Serialize(_),
                ) => write_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "text/plain; charset=utf-8",
                    b"profile import confirmation response failed",
                    true,
                ),
            };
        }
        LauncherHttpState::Direct(session_config)
            if path == format!("/session/{}.json", session_config.session().id) =>
        {
            return write_response(
                &mut stream,
                405,
                "Method Not Allowed",
                "text/plain; charset=utf-8",
                b"method not allowed",
                true,
            );
        }
        LauncherHttpState::ProfileImportReview(import_review)
            if path == format!("/profile-import/{}.json", import_review.session().id)
                || path == format!("/profile-import/{}/confirm", import_review.session().id) =>
        {
            return write_response(
                &mut stream,
                405,
                "Method Not Allowed",
                "text/plain; charset=utf-8",
                b"method not allowed",
                true,
            );
        }
        _ => {}
    }

    if protected_launcher_route_path(path) {
        return write_response(
            &mut stream,
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            true,
        );
    }

    if request.method != "GET" {
        return write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed",
            true,
        );
    }

    let Some(asset) = web_assets.asset_for_request_path(path) else {
        return write_response(
            &mut stream,
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            true,
        );
    };

    let body = fs::read(&asset.file_path)
        .with_context(|| format!("read launcher static file {}", asset.file_path.display()))?;
    write_response(&mut stream, 200, "OK", &asset.content_type, &body, false)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HttpRequest {
    method: String,
    path: String,
    content_type: Option<String>,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        if let Some(index) = header_end_index(&buffer) {
            if index > REQUEST_HEADER_LIMIT {
                bail!("launcher HTTP request header exceeded {REQUEST_HEADER_LIMIT} bytes");
            }
            break index;
        }
        if buffer.len() > REQUEST_HEADER_LIMIT {
            bail!("launcher HTTP request header exceeded {REQUEST_HEADER_LIMIT} bytes");
        }

        let count = stream
            .read(&mut chunk)
            .context("read launcher HTTP request")?;
        if count == 0 {
            bail!("launcher HTTP request ended before header terminator");
        }
        buffer.extend_from_slice(&chunk[..count]);
    };

    let header_bytes = &buffer[..header_end];
    let header_text =
        std::str::from_utf8(header_bytes).context("launcher HTTP request header was not UTF-8")?;
    let (method, path) = request_method_path(header_text)?;
    validate_request_headers(header_text)?;
    let content_type = request_content_type(header_text)?;
    let content_length = request_content_length(header_text)?;
    if content_length > REQUEST_BODY_LIMIT {
        bail!("launcher HTTP request body exceeded {REQUEST_BODY_LIMIT} bytes");
    }

    while buffer.len().saturating_sub(header_end) < content_length {
        let count = stream
            .read(&mut chunk)
            .context("read launcher HTTP request body")?;
        if count == 0 {
            bail!("launcher HTTP request body ended before Content-Length");
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.len().saturating_sub(header_end) > REQUEST_BODY_LIMIT {
            bail!("launcher HTTP request body exceeded {REQUEST_BODY_LIMIT} bytes");
        }
    }

    let body = buffer[header_end..header_end + content_length].to_vec();
    Ok(HttpRequest {
        method,
        path,
        content_type,
        body,
    })
}

fn header_end_index(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn request_method_path(headers: &str) -> Result<(String, String)> {
    let request_line = headers
        .lines()
        .next()
        .context("bad launcher HTTP request")?;
    let mut parts = request_line.split(' ');
    let method = parts.next().context("bad launcher HTTP method")?;
    let path = parts.next().context("bad launcher HTTP path")?;
    let version = parts.next().context("bad launcher HTTP version")?;
    if parts.next().is_some() || method.is_empty() || path.is_empty() || version.is_empty() {
        bail!("bad launcher HTTP request line");
    }
    if !method.bytes().all(is_http_token_byte) {
        bail!("bad launcher HTTP method");
    }
    if version != "HTTP/1.0" && version != "HTTP/1.1" {
        bail!("unsupported launcher HTTP version {version}");
    }
    if !path.starts_with('/') {
        bail!("launcher HTTP request path must be origin-form");
    }
    Ok((method.to_owned(), path.to_owned()))
}

fn validate_request_headers(headers: &str) -> Result<()> {
    for line in headers.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            bail!("folded launcher HTTP headers are not supported");
        }
        let Some((name, _value)) = line.split_once(':') else {
            bail!("bad launcher HTTP header line");
        };
        if name.is_empty() || !name.bytes().all(is_http_token_byte) {
            bail!("bad launcher HTTP header name");
        }
    }
    Ok(())
}

fn is_http_token_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
    )
}

fn request_content_length(headers: &str) -> Result<usize> {
    let mut content_length = None;
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if content_length.is_some() {
            bail!("duplicate launcher HTTP Content-Length header");
        }
        content_length = Some(
            value
                .trim()
                .parse::<usize>()
                .context("parse launcher HTTP Content-Length")?,
        );
    }

    Ok(content_length.unwrap_or(0))
}

fn request_content_type(headers: &str) -> Result<Option<String>> {
    let mut content_type = None;
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        if content_type.is_some() {
            bail!("duplicate launcher HTTP Content-Type header");
        }
        content_type = Some(value.trim().to_owned());
    }

    Ok(content_type)
}

fn request_has_json_content_type(request: &HttpRequest) -> bool {
    request
        .content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

fn write_json_content_type_required(stream: &mut TcpStream) -> Result<()> {
    write_response(
        stream,
        415,
        "Unsupported Media Type",
        "text/plain; charset=utf-8",
        b"content type must be application/json",
        true,
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    no_store: bool,
) -> Result<()> {
    let cache_header = if no_store {
        "Cache-Control: no-store\r\n"
    } else {
        ""
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\n{cache_header}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn socket_authority(addr: SocketAddr) -> String {
    if addr.is_ipv6() {
        format!("[{}]:{}", addr.ip(), addr.port())
    } else {
        addr.to_string()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebAssets {
    assets: BTreeMap<String, WebAsset>,
}

impl WebAssets {
    fn load(root: PathBuf) -> Result<Self> {
        let root_canonical = fs::canonicalize(&root)
            .with_context(|| format!("canonicalize web root {}", root.display()))?;
        let manifest_path = root.join(WEB_ASSET_MANIFEST_FILE);
        let manifest_json = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read web asset manifest {}", manifest_path.display()))?;
        let manifest: WebAssetManifest =
            serde_json::from_str(&manifest_json).context("parse web asset manifest")?;

        if manifest.schema != WEB_ASSET_MANIFEST_SCHEMA {
            bail!(
                "unsupported web asset manifest schema {}, expected {}",
                manifest.schema,
                WEB_ASSET_MANIFEST_SCHEMA
            );
        }
        if manifest.app != WEB_ASSET_MANIFEST_APP {
            bail!(
                "unsupported web asset manifest app {}, expected {}",
                manifest.app,
                WEB_ASSET_MANIFEST_APP
            );
        }
        if manifest.protocol != BROWSER_GATEWAY_PROTOCOL_VERSION {
            bail!(
                "web asset manifest protocol {}, expected {}",
                manifest.protocol,
                BROWSER_GATEWAY_PROTOCOL_VERSION
            );
        }
        if manifest.generated_by.as_deref().is_some_and(|value| {
            value.trim().is_empty() || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        }) {
            bail!("web asset manifest generated_by is invalid");
        }

        let mut assets = BTreeMap::new();
        for entry in manifest.assets {
            validate_manifest_asset(&root, &root_canonical, &entry)?;
            let asset = WebAsset {
                file_path: fs::canonicalize(root.join(&entry.path))
                    .with_context(|| format!("canonicalize web asset {}", entry.path))?,
                content_type: entry.content_type,
            };
            if assets.insert(entry.path.clone(), asset).is_some() {
                bail!("duplicate web asset manifest path {}", entry.path);
            }
        }

        if !assets.contains_key("index.html") {
            bail!("web asset manifest must include index.html");
        }

        Ok(Self { assets })
    }

    fn asset_for_request_path(&self, path: &str) -> Option<&WebAsset> {
        let relative = if path == "/" {
            "index.html"
        } else {
            path.strip_prefix('/')?
        };
        is_safe_relative_asset_path(relative)
            .then(|| self.assets.get(relative))
            .flatten()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebAsset {
    file_path: PathBuf,
    content_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct WebAssetManifest {
    schema: u16,
    app: String,
    protocol: u16,
    #[serde(default)]
    generated_by: Option<String>,
    assets: Vec<WebAssetManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct WebAssetManifestEntry {
    path: String,
    content_type: String,
    sha256: String,
    bytes: u64,
}

fn validate_manifest_asset(
    root: &Path,
    root_canonical: &Path,
    entry: &WebAssetManifestEntry,
) -> Result<()> {
    if !is_safe_relative_asset_path(&entry.path) {
        bail!(
            "web asset manifest path is not a safe relative path: {}",
            entry.path
        );
    }
    if entry.content_type.trim().is_empty()
        || entry
            .content_type
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n'))
    {
        bail!(
            "web asset manifest content type is invalid for {}",
            entry.path
        );
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("web asset manifest sha256 is invalid for {}", entry.path);
    }

    let file_path = root.join(&entry.path);
    let canonical = fs::canonicalize(&file_path)
        .with_context(|| format!("canonicalize web asset {}", file_path.display()))?;
    if !canonical.starts_with(root_canonical) {
        bail!("web asset escapes web root: {}", entry.path);
    }
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("stat web asset {}", canonical.display()))?;
    if !metadata.is_file() {
        bail!("web asset is not a file: {}", entry.path);
    }
    if metadata.len() != entry.bytes {
        bail!(
            "web asset byte count mismatch for {}: manifest {}, actual {}",
            entry.path,
            entry.bytes,
            metadata.len()
        );
    }

    Ok(())
}

fn is_safe_relative_asset_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return false;
    }

    Path::new(path)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Shutdown;
    use std::time::{SystemTime, UNIX_EPOCH};
    use witty_transport::ProfileStoreV1;

    #[test]
    fn random_hex_has_expected_length_and_charset() {
        let token = random_hex(32).unwrap();

        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn protected_launcher_routes_require_lowercase_hex_ids() {
        let id = "0123456789abcdef0123456789abcdef";

        assert_eq!(
            profile_picker_route(&format!("/profile-picker/{id}.json")),
            Some((id, ProfilePickerRouteKind::Bootstrap))
        );
        assert_eq!(
            profile_picker_route(&format!("/profile-picker/{id}/select")),
            Some((id, ProfilePickerRouteKind::Select))
        );
        assert_eq!(
            profile_picker_route(&format!("/profile-picker/{id}/import")),
            Some((id, ProfilePickerRouteKind::Import))
        );
        assert_eq!(
            profile_import_route(&format!("/profile-import/{id}.json")),
            Some((id, ProfileImportRouteKind::Bootstrap))
        );
        assert_eq!(
            profile_import_route(&format!("/profile-import/{id}/confirm")),
            Some((id, ProfileImportRouteKind::Confirm))
        );

        for invalid in [
            "picker",
            "0123456789abcdef0123456789abcdeg",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0",
            "0123456789abcdef0123456789abcdef/extra",
        ] {
            assert!(profile_picker_route(&format!("/profile-picker/{invalid}.json")).is_none());
            assert!(profile_picker_route(&format!("/profile-picker/{invalid}/select")).is_none());
            assert!(profile_picker_route(&format!("/profile-picker/{invalid}/import")).is_none());
            assert!(profile_import_route(&format!("/profile-import/{invalid}.json")).is_none());
            assert!(profile_import_route(&format!("/profile-import/{invalid}/confirm")).is_none());
        }
    }

    #[test]
    fn unhandled_protected_launcher_routes_return_no_store_not_found() {
        let response = launcher_http_response(
            LauncherHttpState::Direct(Arc::new(SessionConfigState::new(
                LaunchSession {
                    id: "0123456789abcdef0123456789abcdef".to_owned(),
                    token: "token".to_owned(),
                    ui_origin: "http://127.0.0.1:10000".to_owned(),
                    ui_url:
                        "http://127.0.0.1:10000/index.html#session=0123456789abcdef0123456789abcdef"
                            .to_owned(),
                    gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
                    mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
                    max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
                },
                Duration::from_secs(60),
            ))),
            "POST /profile-picker/not-a-session/select HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        );

        assert!(response.starts_with("HTTP/1.1 404 Not Found"), "{response}");
        assert!(response.contains("Cache-Control: no-store"), "{response}");

        for request in [
            "POST /profile-picker/fedcba9876543210fedcba9876543210/select HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/fedcba9876543210fedcba9876543210/select/extra HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/not-a-session/import HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/not-a-session/import/extra HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-import/fedcba9876543210fedcba9876543210/confirm HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-import/fedcba9876543210fedcba9876543210/confirm/extra HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "GET /profile-import/not-a-session.json HTTP/1.1\r\n\r\n",
            "POST /profile-import/fedcba9876543210fedcba9876543210.json/extra HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /session/fedcba9876543210fedcba9876543210.json HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "POST /session/fedcba9876543210fedcba9876543210.json/extra HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "GET /session/not-a-session.json HTTP/1.1\r\n\r\n",
        ] {
            let response = launcher_http_response(
                LauncherHttpState::Direct(Arc::new(SessionConfigState::new(
                    LaunchSession {
                        id: "0123456789abcdef0123456789abcdef".to_owned(),
                        token: "token".to_owned(),
                        ui_origin: "http://127.0.0.1:10000".to_owned(),
                        ui_url:
                            "http://127.0.0.1:10000/index.html#session=0123456789abcdef0123456789abcdef"
                                .to_owned(),
                        gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
                        mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
                        max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
                    },
                    Duration::from_secs(60),
                ))),
                request,
            );
            assert!(
                response.starts_with("HTTP/1.1 404 Not Found"),
                "{request}: {response}"
            );
            assert!(
                response.contains("Cache-Control: no-store"),
                "{request}: {response}"
            );
        }

        let oversized_header = format!(
            "GET / HTTP/1.1\r\nX-Fill: {}\r\n\r\n",
            "a".repeat(REQUEST_HEADER_LIMIT)
        );
        let response = launcher_http_response(
            LauncherHttpState::Direct(Arc::new(SessionConfigState::new(
                LaunchSession {
                    id: "0123456789abcdef0123456789abcdef".to_owned(),
                    token: "token".to_owned(),
                    ui_origin: "http://127.0.0.1:10000".to_owned(),
                    ui_url:
                        "http://127.0.0.1:10000/index.html#session=0123456789abcdef0123456789abcdef"
                            .to_owned(),
                    gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
                    mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
                    max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
                },
                Duration::from_secs(60),
            ))),
            &oversized_header,
        );
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "{response}"
        );
        assert!(response.contains("Cache-Control: no-store"), "{response}");
    }

    #[test]
    fn malformed_launcher_http_requests_return_no_store_bad_request() {
        for request in [
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nContent-Type: application/json\r\ncontent-type: text/plain\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nContent-Length: 0\r\ncontent-length: 0\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nContent-Length: 16385\r\n\r\n",
            "BROKEN\r\n\r\n",
            "GET / HTTP/2.0\r\n\r\n",
            "GET / HTTP/1.1 extra\r\n\r\n",
            "GET http://127.0.0.1/index.html HTTP/1.1\r\n\r\n",
            "GET  / HTTP/1.1\r\n\r\n",
            "GET\t/\tHTTP/1.1\r\n\r\n",
            "BAD METHOD / HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: 127.0.0.1",
            "",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nBrokenHeader\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\n Content-Length: 0\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\n: value\r\nContent-Length: 0\r\n\r\n",
            "POST /profile-picker/0123456789abcdef0123456789abcdef/select HTTP/1.1\r\nBad Header: value\r\nContent-Length: 0\r\n\r\n",
        ] {
            let response = launcher_http_response(
                LauncherHttpState::Direct(Arc::new(SessionConfigState::new(
                    LaunchSession {
                        id: "0123456789abcdef0123456789abcdef".to_owned(),
                        token: "token".to_owned(),
                        ui_origin: "http://127.0.0.1:10000".to_owned(),
                        ui_url:
                            "http://127.0.0.1:10000/index.html#session=0123456789abcdef0123456789abcdef"
                                .to_owned(),
                        gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
                        mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
                        max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
                    },
                    Duration::from_secs(60),
                ))),
                request,
            );
            assert!(
                response.starts_with("HTTP/1.1 400 Bad Request"),
                "{request}: {response}"
            );
            assert!(
                response.contains("Cache-Control: no-store"),
                "{request}: {response}"
            );
        }
    }

    #[test]
    fn launch_session_url_keeps_token_out_of_page_url() {
        let session = build_launch_session(
            "127.0.0.1:10000".parse().unwrap(),
            "127.0.0.1:10001".parse().unwrap(),
        )
        .unwrap();

        assert!(session.ui_url.contains("#session="));
        assert!(!session.ui_url.contains(&session.token));
        assert_eq!(session.ui_origin, "http://127.0.0.1:10000");
        assert_eq!(session.gateway_url, "ws://127.0.0.1:10001/witty");
        assert_eq!(
            session.mouse_selection_override,
            MouseSelectionOverridePolicy::ShiftSelect
        );
        assert_eq!(session.max_scrollback_lines, DEFAULT_MAX_SCROLLBACK_LINES);
    }

    #[test]
    fn browser_session_config_contains_gateway_url_and_token() {
        let session = LaunchSession {
            id: "session".to_owned(),
            token: "token".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#session=session".to_owned(),
            gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
            mouse_selection_override: MouseSelectionOverridePolicy::Disabled,
            max_scrollback_lines: 1234,
        };

        assert_eq!(
            browser_session_config_json(&session).unwrap(),
            r#"{"protocol":1,"gateway_url":"ws://127.0.0.1:10001/witty","token":"token","mouse_selection_override":"disabled","scrollback_lines":1234,"expires_at_ms":180000}"#
        );
    }

    #[test]
    fn browser_session_config_rejects_unknown_fields() {
        let json = r#"{
            "protocol":1,
            "gateway_url":"ws://127.0.0.1:10001/witty",
            "token":"token",
            "mouse_selection_override":"disabled",
            "scrollback_lines":1234,
            "expires_at_ms":180000,
            "ssh_profile":{"id":"prod"}
        }"#;

        assert!(serde_json::from_str::<BrowserSessionConfig>(json).is_err());
    }

    #[test]
    fn profile_picker_import_entry_rejects_unknown_fields() {
        let json = r#"{
            "kind":"profile_import_entry",
            "protocol":1,
            "import_url":"/index.html#profile_import=0123456789abcdef0123456789abcdef",
            "config_path":"/home/user/.ssh/config"
        }"#;

        assert!(serde_json::from_str::<ProfilePickerImportEntry>(json).is_err());
    }

    #[test]
    fn profile_action_requests_reject_unknown_fields() {
        let selection = r#"{
            "ui_token":"ui-secret",
            "profile_id":"prod",
            "store_path":"/home/user/.config/witty/profiles.v1.json"
        }"#;
        let import = r#"{
            "ui_token":"ui-secret",
            "action_id":"openssh-config",
            "config_path":"/home/user/.ssh/config"
        }"#;
        let confirmation = r#"{
            "ui_token":"ui-secret",
            "profile_ids":["prod"],
            "conflict":"reject",
            "profile_store_path":"/home/user/.config/witty/profiles.v1.json"
        }"#;

        assert!(serde_json::from_str::<ProfilePickerSelectionRequest>(selection).is_err());
        assert!(serde_json::from_str::<ProfilePickerImportRequest>(import).is_err());
        assert!(serde_json::from_str::<ProfileImportConfirmRequest>(confirmation).is_err());
    }

    #[test]
    fn profile_action_posts_require_json_content_type() {
        let request = |content_type: Option<&str>| HttpRequest {
            method: "POST".to_owned(),
            path: "/profile-picker/0123456789abcdef0123456789abcdef/select".to_owned(),
            content_type: content_type.map(str::to_owned),
            body: br#"{"ui_token":"ui-secret","profile_id":"prod"}"#.to_vec(),
        };

        assert!(request_has_json_content_type(&request(Some(
            "application/json"
        ))));
        assert!(request_has_json_content_type(&request(Some(
            "Application/JSON; charset=utf-8"
        ))));
        assert!(!request_has_json_content_type(&request(None)));
        assert!(!request_has_json_content_type(&request(Some("text/plain"))));
        assert!(!request_has_json_content_type(&request(Some(
            "application/x-www-form-urlencoded"
        ))));
    }

    #[test]
    fn request_content_type_parses_single_header_only() {
        let headers = "POST /x HTTP/1.1\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: 2\r\n\r\n";

        assert_eq!(
            request_content_type(headers).unwrap().as_deref(),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(
            request_content_type("GET /x HTTP/1.1\r\n\r\n").unwrap(),
            None
        );
        assert!(request_content_type(
            "POST /x HTTP/1.1\r\nContent-Type: application/json\r\ncontent-type: text/plain\r\n\r\n"
        )
        .is_err());
    }

    #[test]
    fn profile_picker_session_url_keeps_ui_token_out_of_page_url() {
        let summary = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )])
        .redacted_summary()
        .unwrap();
        let store_path = PathBuf::from("/home/alice/.config/witty/profiles.v1.json");
        let session = build_profile_picker_session(
            "127.0.0.1:10000".parse().unwrap(),
            store_path.clone(),
            summary,
            Vec::new(),
        )
        .unwrap();

        assert!(session.ui_url.contains("#profile_picker="));
        assert!(!session.ui_url.contains(&session.ui_token));
        assert_eq!(session.ui_origin, "http://127.0.0.1:10000");
        assert_eq!(session.store_path(), store_path.as_path());
    }

    #[test]
    fn profile_picker_bootstrap_contains_redacted_summary_only() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        prod.target.user("alice").port(2222);
        prod.credential = witty_transport::SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        prod.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        prod.openssh.extra_args.push("-vv".to_owned());
        prod.openssh.remote_command.push("uptime".to_owned());
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-secret-prod".to_owned(),
        };
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, vaulted])
        };
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: PathBuf::from("/home/alice/.config/witty/profiles.v1.json"),
            summary: store.redacted_summary().unwrap(),
            import_sources: Vec::new(),
        };

        let json = profile_picker_bootstrap_json(&session).unwrap();

        assert!(json.contains(r#""kind":"profile_picker""#));
        assert!(json.contains(r#""ui_token":"ui-secret""#));
        assert!(json.contains(r#""selection_url":"/profile-picker/picker/select""#));
        assert!(json.contains(r#""id":"prod""#));
        assert!(json.contains(r#""name":"Production""#));
        assert!(json.contains(r#""tags":["work"]"#));
        assert!(json.contains(r#""is_default":true"#));
        assert!(json.contains(r#""launchability":"launchable""#));
        assert!(json.contains(r#""launchability":"requires_credential_resolver""#));
        for sensitive in [
            "prod.example.com",
            "vault.example.com",
            "alice",
            "2222",
            "prod_ed25519",
            "/home/alice/.ssh/config",
            "/home/alice/.config/witty/profiles.v1.json",
            "-vv",
            "uptime",
            "vault-secret-prod",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
            "\"remote_command\"",
            "\"secret_id\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile picker bootstrap leaked {sensitive}: {json}"
            );
        }
    }

    #[test]
    fn profile_picker_bootstrap_rejects_unknown_fields() {
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);
        let session = ProfilePickerSession {
            id: "0123456789abcdef0123456789abcdef".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url:
                "http://127.0.0.1:10000/index.html#profile_picker=0123456789abcdef0123456789abcdef"
                    .to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: PathBuf::from("/home/alice/.config/witty/profiles.v1.json"),
            summary: store.redacted_summary().unwrap(),
            import_sources: Vec::new(),
        };
        let json = profile_picker_bootstrap_json(&session).unwrap();
        let parsed: ProfilePickerBootstrap = serde_json::from_str(&json).unwrap();
        assert!(parsed.import_actions.is_empty());

        let extra_envelope = r#"{
            "kind":"profile_picker",
            "protocol":1,
            "ui_token":"ui-secret",
            "selection_url":"/profile-picker/0123456789abcdef0123456789abcdef/select",
            "expires_at_ms":180000,
            "summary":{
                "profiles":[],
                "default_profile_id":null,
                "launchable_profiles":0,
                "credential_resolver_required_profiles":0
            },
            "store_path":"/home/user/.config/witty/profiles.v1.json"
        }"#;
        assert!(serde_json::from_str::<ProfilePickerBootstrap>(extra_envelope).is_err());

        let extra_action = r#"{
            "kind":"profile_picker",
            "protocol":1,
            "ui_token":"ui-secret",
            "selection_url":"/profile-picker/0123456789abcdef0123456789abcdef/select",
            "import_url":"/profile-picker/0123456789abcdef0123456789abcdef/import",
            "expires_at_ms":180000,
            "summary":{
                "profiles":[],
                "default_profile_id":null,
                "launchable_profiles":0,
                "credential_resolver_required_profiles":0
            },
            "import_actions":[{
                "id":"openssh-config",
                "kind":"openssh_config",
                "label":"OpenSSH Import",
                "config_path":"/home/user/.ssh/config"
            }]
        }"#;
        assert!(serde_json::from_str::<ProfilePickerBootstrap>(extra_action).is_err());
    }

    #[test]
    fn profile_picker_bootstrap_contains_redacted_import_action_only() {
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: PathBuf::from("/home/alice/.config/witty/profiles.v1.json"),
            summary: store.redacted_summary().unwrap(),
            import_sources: vec![ProfilePickerImportSource::openssh_config(
                "/home/alice/.ssh/config",
            )],
        };

        let json = profile_picker_bootstrap_json(&session).unwrap();

        assert!(json.contains(r#""import_url":"/profile-picker/picker/import""#));
        assert!(json.contains(r#""id":"openssh-config""#));
        assert!(json.contains(r#""kind":"openssh_config""#));
        assert!(json.contains(r#""label":"OpenSSH Import""#));
        for sensitive in [
            "prod.example.com",
            "/home/alice/.ssh/config",
            "/home/alice/.config/witty/profiles.v1.json",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
            "\"config_path\"",
            "\"store_path\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile picker import action leaked {sensitive}: {json}"
            );
        }
    }

    #[test]
    fn profile_import_review_session_url_keeps_ui_token_and_paths_out_of_page_url() {
        let review = OpenSshImportReview {
            candidates: Vec::new(),
            selected_by_default: Vec::new(),
            warning_count: 0,
            global_warning_count: 0,
            conflict_count: 0,
        };
        let config_path = PathBuf::from("/home/alice/.ssh/config");
        let store_path = PathBuf::from("/home/alice/.config/witty/profiles.v1.json");
        let session = build_profile_import_review_session(
            "127.0.0.1:10000".parse().unwrap(),
            config_path.clone(),
            store_path.clone(),
            review,
        )
        .unwrap();

        assert!(session.ui_url.contains("#profile_import="));
        assert!(!session.ui_url.contains(&session.ui_token));
        assert!(!session.ui_url.contains("/home/alice"));
        assert_eq!(session.ui_origin, "http://127.0.0.1:10000");
        assert_eq!(session.config_path(), config_path.as_path());
        assert_eq!(session.store_path(), store_path.as_path());
    }

    #[test]
    fn profile_import_review_bootstrap_contains_redacted_review_only() {
        let review = OpenSshImportReview {
            candidates: vec![witty_transport::OpenSshImportCandidateSummary {
                id: "prod".to_owned(),
                name: "Production".to_owned(),
                tags: vec!["imported".to_owned(), "openssh".to_owned()],
                warning_count: 1,
                has_conflict: true,
            }],
            selected_by_default: Vec::new(),
            warning_count: 2,
            global_warning_count: 1,
            conflict_count: 1,
        };
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: PathBuf::from("/home/alice/.ssh/config"),
            store_path: PathBuf::from("/home/alice/.config/witty/profiles.v1.json"),
            review,
        };

        let json = profile_import_review_bootstrap_json(&session).unwrap();

        assert!(json.contains(r#""kind":"profile_import""#));
        assert!(json.contains(r#""ui_token":"ui-secret""#));
        assert!(json.contains(r#""confirm_url":"/profile-import/review/confirm""#));
        assert!(json.contains(r#""id":"prod""#));
        assert!(json.contains(r#""has_conflict":true"#));
        for sensitive in [
            "/home/alice/.ssh/config",
            "/home/alice/.config/witty",
            "prod.example.com",
            "\"profile\"",
            "\"source\"",
            "\"target\"",
            "\"credential\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile import bootstrap leaked {sensitive}: {json}"
            );
        }
    }

    #[test]
    fn profile_import_review_bootstrap_rejects_unknown_fields() {
        let extra_envelope = r#"{
            "kind":"profile_import",
            "protocol":1,
            "ui_token":"ui-secret",
            "confirm_url":"/profile-import/0123456789abcdef0123456789abcdef/confirm",
            "expires_at_ms":180000,
            "review":{
                "candidates":[],
                "selected_by_default":[],
                "warning_count":0,
                "global_warning_count":0,
                "conflict_count":0
            },
            "config_path":"/home/user/.ssh/config"
        }"#;
        assert!(serde_json::from_str::<ProfileImportReviewBootstrap>(extra_envelope).is_err());
    }

    #[test]
    fn profile_import_review_state_serves_bootstrap_once() {
        let review = OpenSshImportReview {
            candidates: Vec::new(),
            selected_by_default: Vec::new(),
            warning_count: 0,
            global_warning_count: 0,
            conflict_count: 0,
        };
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: PathBuf::from("ssh_config"),
            store_path: PathBuf::from("profiles.v1.json"),
            review,
        };
        let done = Arc::new(AtomicBool::new(false));
        let state =
            ProfileImportReviewState::new(session, Duration::from_secs(60), Arc::clone(&done));

        assert!(state
            .take_bootstrap_json()
            .unwrap()
            .contains(r#""ui_token":"ui-secret""#));
        assert!(!done.load(Ordering::SeqCst));
        assert_eq!(
            state.take_bootstrap_json().unwrap_err(),
            ProfileImportReviewUnavailable::Used
        );
    }

    #[test]
    fn profile_import_confirmation_writes_selected_ids_and_returns_aggregate_json_once() {
        let root = unique_temp_dir("witty-profile-import-confirm");
        let config_path = root.join("ssh_config");
        let store_path = root.join("nested").join("profiles.v1.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
    User alice
    Port 2222
    IdentityFile /home/alice/.ssh/prod_ed25519
    RemoteCommand uptime
Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();
        let review = build_profile_import_review(&config_path, &store_path).unwrap();
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: config_path.clone(),
            store_path: store_path.clone(),
            review,
        };
        let done = Arc::new(AtomicBool::new(false));
        let state =
            ProfileImportReviewState::new(session, Duration::from_secs(60), Arc::clone(&done));

        assert_eq!(
            state
                .confirm_json(br#"{"ui_token":"bad","profile_ids":["prod"],"conflict":"reject"}"#)
                .unwrap_err(),
            ProfileImportConfirmUnavailable::Unauthorized
        );
        assert!(!store_path.exists());
        assert!(!done.load(Ordering::SeqCst));

        let json = state
            .confirm_json(br#"{"ui_token":"ui-secret","profile_ids":["prod"],"conflict":"reject"}"#)
            .unwrap();
        let report: ProfileImportConfirmReport = serde_json::from_str(&json).unwrap();

        assert!(report.changed);
        assert_eq!(report.profiles, 1);
        assert_eq!(report.selected, 1);
        assert_eq!(report.added, 1);
        assert_eq!(report.replaced, 0);
        assert_eq!(report.warning_count, 2);
        assert_eq!(report.global_warning_count, 0);
        assert!(report.next_picker_url.is_none());
        assert!(report.bytes > 0);
        assert!(report.created_parent_dir);
        assert!(done.load(Ordering::SeqCst));
        let store = read_profile_store(&store_path).unwrap();
        assert!(store.profile("prod").is_some());
        assert!(store.profile("staging").is_none());
        for sensitive in [
            "prod.example.com",
            "alice",
            "2222",
            "prod_ed25519",
            "uptime",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile import confirmation leaked {sensitive}: {json}"
            );
        }
        assert_eq!(
            state
                .confirm_json(
                    br#"{"ui_token":"ui-secret","profile_ids":["staging"],"conflict":"reject"}"#
                )
                .unwrap_err(),
            ProfileImportConfirmUnavailable::Used
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_import_confirmation_preflight_failures_do_not_claim_review() {
        let root = unique_temp_dir("witty-profile-import-confirm-preflight");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();
        fs::write(
            &store_path,
            ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing",
                "old.example.com",
            )])
            .to_pretty_json()
            .unwrap(),
        )
        .unwrap();
        let review = build_profile_import_review(&config_path, &store_path).unwrap();
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: config_path.clone(),
            store_path: store_path.clone(),
            review,
        };
        let done = Arc::new(AtomicBool::new(false));
        let state =
            ProfileImportReviewState::new(session, Duration::from_secs(60), Arc::clone(&done));

        for body in [
            br#"{"ui_token":"ui-secret","profile_ids":["staging","staging"],"conflict":"reject"}"#
                .as_slice(),
            br#"{"ui_token":"ui-secret","profile_ids":["missing"],"conflict":"reject"}"#.as_slice(),
            br#"{"ui_token":"ui-secret","profile_ids":["prod"],"conflict":"reject"}"#.as_slice(),
        ] {
            assert!(matches!(
                state.confirm_json(body).unwrap_err(),
                ProfileImportConfirmUnavailable::BadRequest(_)
            ));
            assert!(!state.confirmed.load(Ordering::SeqCst));
            assert!(!done.load(Ordering::SeqCst));
        }

        let json = state
            .confirm_json(
                br#"{"ui_token":"ui-secret","profile_ids":["staging"],"conflict":"reject"}"#,
            )
            .unwrap();
        let report: ProfileImportConfirmReport = serde_json::from_str(&json).unwrap();

        assert_eq!(report.selected, 1);
        assert_eq!(report.added, 1);
        assert_eq!(report.replaced, 0);
        assert!(done.load(Ordering::SeqCst));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_import_confirmation_duplicate_candidate_preflight_does_not_claim_review() {
        let root = unique_temp_dir("witty-profile-import-confirm-duplicate-candidate");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();
        let mut review = build_profile_import_review(&config_path, &store_path).unwrap();
        review.candidates.push(review.candidates[0].clone());
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: config_path.clone(),
            store_path: store_path.clone(),
            review,
        };
        let done = Arc::new(AtomicBool::new(false));
        let state =
            ProfileImportReviewState::new(session, Duration::from_secs(60), Arc::clone(&done));

        assert_eq!(
            state
                .confirm_json(
                    br#"{"ui_token":"ui-secret","profile_ids":["prod"],"conflict":"replace"}"#
                )
                .unwrap_err(),
            ProfileImportConfirmUnavailable::BadRequest(
                "profile import confirmation contains duplicate candidate ids".to_owned()
            )
        );
        assert!(!state.confirmed.load(Ordering::SeqCst));
        assert!(!done.load(Ordering::SeqCst));
        assert!(!store_path.exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_import_confirmation_apply_failure_releases_claim_for_retry() {
        let root = unique_temp_dir("witty-profile-import-confirm-apply-retry");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        let prod_config = r#"
Host prod
    HostName prod.example.com
"#;
        fs::write(&config_path, prod_config).unwrap();
        let review = build_profile_import_review(&config_path, &store_path).unwrap();
        let session = ProfileImportReviewSession {
            id: "review".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_import=review".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            config_path: config_path.clone(),
            store_path: store_path.clone(),
            review,
        };
        let done = Arc::new(AtomicBool::new(false));
        let state =
            ProfileImportReviewState::new(session, Duration::from_secs(60), Arc::clone(&done));

        fs::write(
            &config_path,
            r#"
Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();
        assert!(matches!(
            state
                .confirm_json(
                    br#"{"ui_token":"ui-secret","profile_ids":["prod"],"conflict":"replace"}"#
                )
                .unwrap_err(),
            ProfileImportConfirmUnavailable::Apply(_)
        ));
        assert!(!state.confirmed.load(Ordering::SeqCst));
        assert!(!done.load(Ordering::SeqCst));
        assert!(!store_path.exists());

        fs::write(&config_path, prod_config).unwrap();
        let json = state
            .confirm_json(
                br#"{"ui_token":"ui-secret","profile_ids":["prod"],"conflict":"replace"}"#,
            )
            .unwrap();
        let report: ProfileImportConfirmReport = serde_json::from_str(&json).unwrap();

        assert_eq!(report.selected, 1);
        assert_eq!(report.added, 1);
        assert!(done.load(Ordering::SeqCst));
        assert!(read_profile_store(&store_path)
            .unwrap()
            .profile("prod")
            .is_some());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_import_confirm_report_rejects_unknown_fields() {
        let report = r#"{
            "changed":true,
            "profiles":2,
            "default_changed":false,
            "bytes":128,
            "created_parent_dir":false,
            "selected":1,
            "added":1,
            "replaced":0,
            "warning_count":0,
            "global_warning_count":0,
            "store_path":"/home/user/.config/witty/profiles.v1.json"
        }"#;

        assert!(serde_json::from_str::<ProfileImportConfirmReport>(report).is_err());
    }

    #[test]
    fn profile_import_confirmation_reject_conflict_preserves_store_bytes() {
        let root = unique_temp_dir("witty-profile-import-confirm-conflict");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        fs::write(
            &config_path,
            r#"
Host prod
    HostName new.example.com
"#,
        )
        .unwrap();
        fs::write(
            &store_path,
            ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing",
                "old.example.com",
            )])
            .to_pretty_json()
            .unwrap(),
        )
        .unwrap();
        let before = fs::read_to_string(&store_path).unwrap();

        let error = run_profile_import_confirm(
            &config_path,
            &store_path,
            &OpenSshImportSelection::profile_ids(["prod"]),
            OpenSshImportConflictPolicy::Reject,
        )
        .unwrap_err();
        let after = fs::read_to_string(&store_path).unwrap();

        assert!(format!("{error:#}").contains("selected profile id conflicts"));
        assert_eq!(before, after);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn build_profile_import_review_marks_conflicts_and_keeps_review_redacted() {
        let root = unique_temp_dir("witty-profile-import-review");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
    User alice
    Port 2222
    IdentityFile /home/alice/.ssh/prod_ed25519
    RemoteCommand uptime
Include /home/alice/.ssh/conf.d/*.conf
"#,
        )
        .unwrap();
        fs::write(
            &store_path,
            ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing",
                "old.example.com",
            )])
            .to_pretty_json()
            .unwrap(),
        )
        .unwrap();

        let review = build_profile_import_review(&config_path, &store_path).unwrap();
        let json = serde_json::to_string(&review).unwrap();
        let root_string = root.to_string_lossy().into_owned();

        assert_eq!(review.conflict_count, 1);
        assert_eq!(review.candidates[0].id, "prod");
        assert!(review.candidates[0].has_conflict);
        assert!(review.selected_by_default.is_empty());
        for sensitive in [
            root_string.as_str(),
            "prod.example.com",
            "old.example.com",
            "alice",
            "2222",
            "prod_ed25519",
            "uptime",
            "/home/alice/.ssh/conf.d",
            "\"target\"",
            "\"credential\"",
            "\"source\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile import review leaked {sensitive}: {json}"
            );
        }

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn session_config_state_serves_config_once() {
        let session = LaunchSession {
            id: "session".to_owned(),
            token: "token".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#session=session".to_owned(),
            gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
            mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
        };
        let state = SessionConfigState::new(session, Duration::from_secs(60));

        assert!(state
            .take_config_json()
            .unwrap()
            .contains(r#""token":"token""#));
        assert_eq!(
            state.take_config_json().unwrap_err(),
            SessionConfigUnavailable::Used
        );
    }

    #[test]
    fn session_config_state_expires_before_first_read() {
        let session = LaunchSession {
            id: "session".to_owned(),
            token: "token".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#session=session".to_owned(),
            gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
            mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
        };
        let state = SessionConfigState::new(session, Duration::ZERO);

        assert_eq!(
            state.take_config_json().unwrap_err(),
            SessionConfigUnavailable::Expired
        );
    }

    #[test]
    fn profile_picker_state_serves_bootstrap_once() {
        let summary = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )])
        .redacted_summary()
        .unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: PathBuf::from("profiles.v1.json"),
            summary,
            import_sources: Vec::new(),
        };
        let state = ProfilePickerState::new(session, Duration::from_secs(60));

        assert!(state
            .take_bootstrap_json()
            .unwrap()
            .contains(r#""ui_token":"ui-secret""#));
        assert_eq!(
            state.take_bootstrap_json().unwrap_err(),
            ProfilePickerUnavailable::Used
        );
    }

    #[test]
    fn profile_picker_state_expires_before_first_read() {
        let summary = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )])
        .redacted_summary()
        .unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: PathBuf::from("profiles.v1.json"),
            summary,
            import_sources: Vec::new(),
        };
        let state = ProfilePickerState::new(session, Duration::ZERO);

        assert_eq!(
            state.take_bootstrap_json().unwrap_err(),
            ProfilePickerUnavailable::Expired
        );
    }

    #[test]
    fn profile_picker_selection_validates_token_profile_id_and_launchability() {
        let root = unique_temp_dir("witty-profile-picker-selection-validation");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let prod = SshProfile::new("prod", "Production", "prod.example.com");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-secret-prod".to_owned(),
        };
        let store = ProfileStoreV1::with_profiles(vec![prod, vaulted]);
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: store_path.clone(),
            summary: store.redacted_summary().unwrap(),
            import_sources: Vec::new(),
        };
        let state = ProfilePickerState::new(session, Duration::from_secs(60));

        assert_eq!(
            state
                .select_profile_json(br#"{"ui_token":"bad","profile_id":"prod"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::Unauthorized
        );
        assert!(matches!(
            state
                .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"bad id"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::BadRequest(_)
        ));
        assert_eq!(
            state
                .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"missing"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::NotFound
        );
        assert!(matches!(
            state
                .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"vaulted"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::RequiresCredentialResolver
        ));
        assert_eq!(
            state
                .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"prod"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::RuntimeUnavailable
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_picker_import_action_starts_review_and_confirm_write() {
        let root = unique_temp_dir("witty-profile-picker-import-action");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
    User alice
Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();
        fs::write(
            &store_path,
            ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing",
                "old.example.com",
            )])
            .to_pretty_json()
            .unwrap(),
        )
        .unwrap();
        let store = read_profile_store(&store_path).unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: store_path.clone(),
            summary: store.redacted_summary().unwrap(),
            import_sources: vec![ProfilePickerImportSource::openssh_config(
                config_path.clone(),
            )],
        };
        let state = ProfilePickerState::new(session, Duration::from_secs(60));

        assert_eq!(
            state
                .start_import_json(br#"{"ui_token":"bad","action_id":"openssh-config"}"#)
                .unwrap_err(),
            ProfilePickerImportUnavailable::Unauthorized
        );
        assert_eq!(
            state
                .start_import_json(br#"{"ui_token":"ui-secret","action_id":"missing"}"#)
                .unwrap_err(),
            ProfilePickerImportUnavailable::NotFound
        );

        let entry_json = state
            .start_import_json(br#"{"ui_token":"ui-secret","action_id":"openssh-config"}"#)
            .unwrap();
        let root_string = root.to_string_lossy().into_owned();

        assert!(entry_json.contains(r#""kind":"profile_import_entry""#));
        assert!(entry_json.contains("\"import_url\":\"/index.html#profile_import="));
        assert!(!entry_json.contains(&root_string));
        assert!(matches!(
            state
                .start_import_json(br#"{"ui_token":"ui-secret","action_id":"openssh-config"}"#)
                .unwrap_err(),
            ProfilePickerImportUnavailable::AlreadyUsed
        ));

        let import_review = state.active_import_review().unwrap();
        let bootstrap_json = import_review.take_bootstrap_json().unwrap();
        assert!(bootstrap_json.contains(r#""kind":"profile_import""#));
        assert!(bootstrap_json.contains(r#""id":"prod""#));
        assert!(!bootstrap_json.contains(&root_string));
        assert!(!bootstrap_json.contains("prod.example.com"));
        let confirm_body = format!(
            r#"{{"ui_token":"{}","profile_ids":["prod","staging"],"conflict":"replace"}}"#,
            import_review.session().ui_token
        );
        let report_json = import_review.confirm_json(confirm_body.as_bytes()).unwrap();
        let report: ProfileImportConfirmReport = serde_json::from_str(&report_json).unwrap();
        let store = read_profile_store(&store_path).unwrap();

        assert!(report.changed);
        assert_eq!(report.selected, 2);
        assert_eq!(report.added, 1);
        assert_eq!(report.replaced, 1);
        let next_picker_url = report.next_picker_url.as_deref().unwrap();
        let next_picker_id = next_picker_url
            .strip_prefix("/index.html#profile_picker=")
            .unwrap();
        assert_ne!(next_picker_id, "picker");
        assert_eq!(store.profiles.len(), 2);
        assert_eq!(
            store.profile("prod").unwrap().target.host,
            "prod.example.com"
        );
        assert_eq!(
            store.profile("staging").unwrap().target.host,
            "staging.example.com"
        );
        for sensitive in [&root_string, "prod.example.com", "staging.example.com"] {
            assert!(
                !report_json.contains(sensitive),
                "profile picker import report leaked {sensitive}: {report_json}"
            );
        }

        let next_bootstrap_json = state.take_bootstrap_json_for_id(next_picker_id).unwrap();
        assert!(next_bootstrap_json.contains(r#""kind":"profile_picker""#));
        assert!(next_bootstrap_json.contains(r#""id":"prod""#));
        assert!(next_bootstrap_json.contains(r#""id":"staging""#));
        assert!(next_bootstrap_json.contains(r#""id":"openssh-config""#));
        for sensitive in [&root_string, "prod.example.com", "staging.example.com"] {
            assert!(
                !next_bootstrap_json.contains(sensitive),
                "refreshed profile picker leaked {sensitive}: {next_bootstrap_json}"
            );
        }
        let next_bootstrap: serde_json::Value = serde_json::from_str(&next_bootstrap_json).unwrap();
        let next_ui_token = next_bootstrap
            .get("ui_token")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let next_import_body =
            format!(r#"{{"ui_token":"{next_ui_token}","action_id":"openssh-config"}}"#);
        let next_entry_json = state
            .start_import_json_for_id(next_picker_id, next_import_body.as_bytes())
            .unwrap();
        let next_entry: serde_json::Value = serde_json::from_str(&next_entry_json).unwrap();
        let next_review_url = next_entry
            .get("import_url")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let next_review_id = next_review_url
            .strip_prefix("/index.html#profile_import=")
            .unwrap();
        let next_review = state.import_review(next_review_id).unwrap();
        let next_review_bootstrap_json = next_review.take_bootstrap_json().unwrap();
        let next_review_bootstrap: serde_json::Value =
            serde_json::from_str(&next_review_bootstrap_json).unwrap();
        let next_candidates = next_review_bootstrap["review"]["candidates"]
            .as_array()
            .unwrap();

        assert_eq!(next_candidates.len(), 2);
        assert!(next_candidates
            .iter()
            .all(|candidate| candidate["has_conflict"].as_bool() == Some(true)));
        assert_eq!(
            state
                .take_bootstrap_json_for_id(next_picker_id)
                .unwrap_err(),
            ProfilePickerUnavailable::Used
        );
        assert!(matches!(
            state
                .start_import_json_for_id(next_picker_id, next_import_body.as_bytes())
                .unwrap_err(),
            ProfilePickerImportUnavailable::AlreadyUsed
        ));
        for sensitive in [&root_string, "prod.example.com", "staging.example.com"] {
            assert!(
                !next_entry_json.contains(sensitive)
                    && !next_review_bootstrap_json.contains(sensitive),
                "refreshed profile picker re-import leaked {sensitive}: {next_entry_json} {next_review_bootstrap_json}"
            );
        }

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_picker_import_build_failure_releases_one_use_claim() {
        let root = unique_temp_dir("witty-profile-picker-import-build-retry");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Existing",
            "old.example.com",
        )]);
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: store_path.clone(),
            summary: store.redacted_summary().unwrap(),
            import_sources: vec![ProfilePickerImportSource::openssh_config(
                config_path.clone(),
            )],
        };
        let state = ProfilePickerState::new(session, Duration::from_secs(60));
        let request = br#"{"ui_token":"ui-secret","action_id":"openssh-config"}"#;

        assert!(matches!(
            state.start_import_json(request).unwrap_err(),
            ProfilePickerImportUnavailable::Build(_)
        ));
        fs::write(
            &config_path,
            r#"
Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();
        let entry_json = state.start_import_json(request).unwrap();

        assert!(entry_json.contains(r#""kind":"profile_import_entry""#));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_picker_selection_starts_gateway_and_returns_redacted_session_config_once() {
        let root = unique_temp_dir("witty-profile-picker-selection");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user("alice").port(2222);
        prod.credential = witty_transport::SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        prod.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        prod.openssh.extra_args.push("-vv".to_owned());
        prod.openssh.remote_command.push("uptime".to_owned());
        let store = ProfileStoreV1::with_profiles(vec![prod]);
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        let gateway_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let gateway_addr = gateway_listener.local_addr().unwrap();
        let runtime = ProfilePickerRuntime::new(
            gateway_listener,
            Arc::new(AtomicBool::new(false)),
            MouseSelectionOverridePolicy::Disabled,
            4321,
        )
        .unwrap();
        let session = ProfilePickerSession {
            id: "picker".to_owned(),
            ui_token: "ui-secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#profile_picker=picker".to_owned(),
            ui_addr: "127.0.0.1:10000".parse().unwrap(),
            store_path: store_path.clone(),
            summary: store.redacted_summary().unwrap(),
            import_sources: Vec::new(),
        };
        let state = ProfilePickerState::new_with_runtime(session, Duration::from_secs(60), runtime);

        let json = state
            .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"prod"}"#)
            .unwrap();

        assert!(json.contains(r#""protocol":1"#));
        assert!(json.contains(&format!(r#""gateway_url":"ws://{gateway_addr}/witty""#)));
        assert!(json.contains(r#""mouse_selection_override":"disabled""#));
        assert!(json.contains(r#""scrollback_lines":4321"#));
        for sensitive in [
            "prod.example.com",
            "alice",
            "2222",
            "prod_ed25519",
            "/home/alice/.ssh/config",
            "-vv",
            "uptime",
            "\"profiles\"",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "profile picker selection response leaked {sensitive}: {json}"
            );
        }
        assert_eq!(
            state
                .select_profile_json(br#"{"ui_token":"ui-secret","profile_id":"prod"}"#)
                .unwrap_err(),
            ProfilePickerSelectionUnavailable::AlreadySelected
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn gateway_config_uses_exact_origin_and_session_token() {
        let launcher = LauncherConfig {
            program: Some("/bin/sh".to_owned()),
            args: vec!["-lc".to_owned(), "cat".to_owned()],
            ..LauncherConfig::default()
        };
        let session = LaunchSession {
            id: "session".to_owned(),
            token: "secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#session=session".to_owned(),
            gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
            mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
        };

        let gateway =
            gateway_config_for_session(&launcher, &session, "127.0.0.1:10001".parse().unwrap())
                .unwrap();

        assert_eq!(gateway.bind, "127.0.0.1:10001");
        assert_eq!(gateway.token.as_deref(), Some("secret"));
        assert_eq!(gateway.allowed_origins, ["http://127.0.0.1:10000"]);
        assert_eq!(gateway.program.as_deref(), Some("/bin/sh"));
        assert_eq!(gateway.args, ["-lc", "cat"]);
        assert!(gateway.local_pty_config.is_none());
    }

    #[test]
    fn gateway_config_uses_trusted_ssh_profile_without_browser_disclosure() {
        let mut profile = SshProfile::new("prod", "Production", "example.com");
        profile.target.user("alice").port(2222);
        let launcher = LauncherConfig {
            ssh_profile: Some(profile),
            ..LauncherConfig::default()
        };
        let session = LaunchSession {
            id: "session".to_owned(),
            token: "secret".to_owned(),
            ui_origin: "http://127.0.0.1:10000".to_owned(),
            ui_url: "http://127.0.0.1:10000/index.html#session=session".to_owned(),
            gateway_url: "ws://127.0.0.1:10001/witty".to_owned(),
            mouse_selection_override: MouseSelectionOverridePolicy::ShiftSelect,
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
        };

        let gateway =
            gateway_config_for_session(&launcher, &session, "127.0.0.1:10001".parse().unwrap())
                .unwrap();
        let local_pty_config = gateway.local_pty_config.as_ref().unwrap();
        let session_json = browser_session_config_json(&session).unwrap();

        assert_eq!(gateway.program, None);
        assert!(gateway.args.is_empty());
        assert_eq!(local_pty_config.program.as_deref(), Some("ssh"));
        assert_eq!(
            local_pty_config.args,
            ["-tt", "-p", "2222", "alice@example.com"]
        );
        assert!(!session_json.contains("example.com"));
        assert!(!session_json.contains("alice"));
        assert!(!session_json.contains("ssh"));
    }

    #[test]
    fn web_root_resolution_uses_precedence_chain() {
        let root = unique_temp_dir("witty-web-root-resolution");
        let installed = root.join("share/witty/web");
        fs::create_dir_all(&installed).unwrap();
        let exe = root.join("bin/witty");

        assert_eq!(
            resolve_web_root(
                Some(PathBuf::from("explicit-web")),
                Some(PathBuf::from("env-web")),
                Some(&exe)
            ),
            PathBuf::from("explicit-web")
        );
        assert_eq!(
            resolve_web_root(None, Some(PathBuf::from("env-web")), Some(&exe)),
            PathBuf::from("env-web")
        );
        assert_eq!(resolve_web_root(None, None, Some(&exe)), installed);

        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            resolve_web_root(None, None, None),
            PathBuf::from(DEVELOPMENT_WEB_ROOT)
        );
    }

    #[test]
    fn web_asset_manifest_allows_only_listed_assets() {
        let root = unique_temp_dir("witty-web-assets");
        fs::create_dir_all(&root).unwrap();
        write_test_asset(&root, "index.html", b"index");
        write_test_asset(&root, "pkg/witty_web_bg.wasm", b"wasm");
        write_test_asset(&root, "app.js", b"js");
        write_test_manifest(
            &root,
            &[
                ("index.html", "text/html; charset=utf-8", 5),
                ("pkg/witty_web_bg.wasm", "application/wasm", 4),
                ("app.js", "text/javascript; charset=utf-8", 2),
            ],
        );

        let assets = WebAssets::load(root.clone()).unwrap();
        assert_eq!(
            assets.asset_for_request_path("/").unwrap().file_path,
            fs::canonicalize(root.join("index.html")).unwrap()
        );
        assert_eq!(
            assets
                .asset_for_request_path("/pkg/witty_web_bg.wasm")
                .unwrap()
                .content_type,
            "application/wasm"
        );
        assert_eq!(
            assets.asset_for_request_path("/app.js").unwrap().file_path,
            fs::canonicalize(root.join("app.js")).unwrap()
        );
        assert!(assets.asset_for_request_path("/smoke.js").is_none());
        assert!(assets.asset_for_request_path("/../Cargo.toml").is_none());
        assert!(assets
            .asset_for_request_path("/session/other.json")
            .is_none());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn web_asset_manifest_rejects_unknown_fields() {
        let root = unique_temp_dir("witty-web-assets-unknown-fields");
        fs::create_dir_all(&root).unwrap();
        write_test_asset(&root, "index.html", b"index");

        let asset = format!(
            r#"{{"path":"index.html","content_type":"text/html; charset=utf-8","sha256":"{}","bytes":5,"source_path":"/tmp/index.html"}}"#,
            "0".repeat(64)
        );
        let manifest = format!(
            r#"{{"schema":1,"app":"witty-web","protocol":1,"generated_by":"test","assets":[{asset}]}}"#
        );
        fs::write(root.join(WEB_ASSET_MANIFEST_FILE), manifest).unwrap();
        assert!(WebAssets::load(root.clone()).is_err());

        write_test_manifest(&root, &[("index.html", "text/html; charset=utf-8", 5)]);
        let manifest = fs::read_to_string(root.join(WEB_ASSET_MANIFEST_FILE)).unwrap();
        fs::write(
            root.join(WEB_ASSET_MANIFEST_FILE),
            manifest.replace(r#""assets":"#, r#""source_root":"/tmp","assets":"#),
        )
        .unwrap();
        assert!(WebAssets::load(root.clone()).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn web_asset_manifest_rejects_unsafe_paths() {
        let root = unique_temp_dir("witty-web-assets-unsafe");
        fs::create_dir_all(&root).unwrap();
        write_test_asset(&root, "index.html", b"index");
        write_test_manifest(
            &root,
            &[
                ("index.html", "text/html; charset=utf-8", 5),
                ("../outside.js", "text/javascript; charset=utf-8", 2),
            ],
        );

        assert!(WebAssets::load(root.clone()).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn web_asset_manifest_rejects_byte_count_mismatch() {
        let root = unique_temp_dir("witty-web-assets-bytes");
        fs::create_dir_all(&root).unwrap();
        write_test_asset(&root, "index.html", b"index");
        write_test_manifest(&root, &[("index.html", "text/html; charset=utf-8", 99)]);

        assert!(WebAssets::load(root.clone()).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_rejects_args_without_program() {
        assert!(parse_config(["--arg".to_owned(), "-lc".to_owned()]).is_err());
    }

    #[test]
    fn parse_config_accepts_open_browser_flag() {
        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--open-browser".to_owned(),
        ])
        .unwrap();

        assert!(config.open_browser);
    }

    #[test]
    fn parse_config_accepts_mouse_selection_override_policy() {
        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--mouse-selection-override".to_owned(),
            "disabled".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config.mouse_selection_override,
            MouseSelectionOverridePolicy::Disabled
        );
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--mouse-selection-override".to_owned(),
            "raw".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn parse_config_accepts_scrollback_line_limit() {
        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--scrollback-lines".to_owned(),
            "2500".to_owned(),
        ])
        .unwrap();

        assert_eq!(config.max_scrollback_lines, 2500);
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--scrollback-lines".to_owned(),
            "many".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn parse_config_accepts_trusted_ssh_profile_json() {
        let root = unique_temp_dir("witty-ssh-profile-json");
        fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.json");
        let mut profile = SshProfile::new("prod", "Production", "example.com");
        profile.target.user("alice");
        fs::write(&profile_path, serde_json::to_string(&profile).unwrap()).unwrap();

        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--ssh-profile-json".to_owned(),
            profile_path.to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert_eq!(config.ssh_profile.as_ref().unwrap().id, "prod");
        assert_eq!(
            config.ssh_profile.unwrap().target.user.as_deref(),
            Some("alice")
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_selects_launchable_profile_from_store() {
        let root = unique_temp_dir("witty-profile-store");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user("alice");
        let staging = SshProfile::new("staging", "Staging", "staging.example.com");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, staging])
        };
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--ssh-profile-id".to_owned(),
            "prod".to_owned(),
            "--profile-store".to_owned(),
            store_path.to_string_lossy().into_owned(),
        ])
        .unwrap();

        let profile = config.ssh_profile.unwrap();
        assert_eq!(profile.id, "prod");
        assert_eq!(profile.target.user.as_deref(), Some("alice"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_accepts_profile_picker_with_explicit_store() {
        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config.profile_picker_store_path.as_deref(),
            Some(Path::new("profiles.v1.json"))
        );
        assert!(config.ssh_profile.is_none());
    }

    #[test]
    fn parse_config_profile_picker_uses_default_profile_store_path() {
        let default_path = PathBuf::from("/tmp/witty/profiles.v1.json");
        let config = parse_config_with_default_profile_store_path(
            [
                "--web-root".to_owned(),
                ".".to_owned(),
                "--profile-picker".to_owned(),
            ],
            || Ok(default_path.clone()),
        )
        .unwrap();

        assert_eq!(
            config.profile_picker_store_path.as_deref(),
            Some(default_path.as_path())
        );
    }

    #[test]
    fn parse_config_accepts_profile_picker_import_source_binding() {
        let config = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--profile-picker-import-openssh".to_owned(),
            "ssh_config".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config.profile_picker_store_path.as_deref(),
            Some(Path::new("profiles.v1.json"))
        );
        assert_eq!(
            config.profile_picker_import_sources,
            vec![ProfilePickerImportSource::openssh_config("ssh_config")]
        );
        assert!(config.profile_import_review.is_none());
    }

    #[test]
    fn parse_config_accepts_profile_import_review_with_explicit_or_default_store() {
        let explicit = parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            explicit
                .profile_import_review
                .as_ref()
                .map(|review| review.config_path.as_path()),
            Some(Path::new("ssh_config"))
        );
        assert_eq!(
            explicit
                .profile_import_review
                .as_ref()
                .map(|review| review.store_path.as_path()),
            Some(Path::new("profiles.v1.json"))
        );

        let default_path = PathBuf::from("/tmp/witty/profiles.v1.json");
        let defaulted = parse_config_with_default_profile_store_path(
            [
                "--web-root".to_owned(),
                ".".to_owned(),
                "--profile-import-openssh".to_owned(),
                "ssh_config".to_owned(),
            ],
            || Ok(default_path.clone()),
        )
        .unwrap();

        assert_eq!(
            defaulted
                .profile_import_review
                .as_ref()
                .map(|review| review.store_path.as_path()),
            Some(default_path.as_path())
        );
    }

    #[test]
    fn parse_config_rejects_profile_import_review_direct_launch_combinations() {
        let root = unique_temp_dir("witty-profile-import-conflict");
        fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.json");
        fs::write(
            &profile_path,
            serde_json::to_string(&SshProfile::new("prod", "Production", "example.com")).unwrap(),
        )
        .unwrap();

        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--profile-picker".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--program".to_owned(),
            "/bin/sh".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--ssh-profile-id".to_owned(),
            "prod".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--ssh-profile-json".to_owned(),
            profile_path.to_string_lossy().into_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--profile-import-openssh".to_owned(),
            "other_config".to_owned(),
        ])
        .is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_rejects_profile_picker_direct_launch_combinations() {
        let root = unique_temp_dir("witty-profile-picker-conflict");
        fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.json");
        fs::write(
            &profile_path,
            serde_json::to_string(&SshProfile::new("prod", "Production", "example.com")).unwrap(),
        )
        .unwrap();

        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--program".to_owned(),
            "/bin/sh".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--ssh-profile-id".to_owned(),
            "prod".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--ssh-profile-json".to_owned(),
            profile_path.to_string_lossy().into_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--profile-picker-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--profile-picker-import-openssh".to_owned(),
            "other_config".to_owned(),
        ])
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-picker-import-openssh".to_owned(),
            "ssh_config".to_owned(),
        ])
        .is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_uses_default_profile_store_path_when_only_profile_id_is_given() {
        let root = unique_temp_dir("witty-profile-store-default");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user("alice");
        fs::write(
            &store_path,
            ProfileStoreV1::with_profiles(vec![prod])
                .to_pretty_json()
                .unwrap(),
        )
        .unwrap();

        let config = parse_config_with_default_profile_store_path(
            [
                "--web-root".to_owned(),
                ".".to_owned(),
                "--ssh-profile-id".to_owned(),
                "prod".to_owned(),
            ],
            || Ok(store_path.clone()),
        )
        .unwrap();

        let profile = config.ssh_profile.unwrap();
        assert_eq!(profile.id, "prod");
        assert_eq!(profile.target.user.as_deref(), Some("alice"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_rejects_incomplete_or_missing_profile_store_selection() {
        let root = unique_temp_dir("witty-profile-store-missing");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let store =
            ProfileStoreV1::with_profiles(vec![SshProfile::new("prod", "Production", "host")]);
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-store".to_owned(),
            store_path.to_string_lossy().into_owned(),
        ])
        .is_err());
        assert!(parse_config_with_default_profile_store_path(
            [
                "--web-root".to_owned(),
                ".".to_owned(),
                "--ssh-profile-id".to_owned(),
                "prod".to_owned(),
            ],
            || Ok(root.join("missing-default-store.json")),
        )
        .is_err());
        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-store".to_owned(),
            store_path.to_string_lossy().into_owned(),
            "--ssh-profile-id".to_owned(),
            "missing".to_owned(),
        ])
        .is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_rejects_vault_profile_store_selection_without_resolver() {
        let root = unique_temp_dir("witty-profile-store-vault");
        fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-prod-key".to_owned(),
        };
        let store = ProfileStoreV1::with_profiles(vec![vaulted]);
        fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--profile-store".to_owned(),
            store_path.to_string_lossy().into_owned(),
            "--ssh-profile-id".to_owned(),
            "vaulted".to_owned(),
        ])
        .is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_config_rejects_ssh_profile_with_raw_program() {
        let root = unique_temp_dir("witty-ssh-profile-conflict");
        fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.json");
        fs::write(
            &profile_path,
            serde_json::to_string(&SshProfile::new("prod", "Production", "example.com")).unwrap(),
        )
        .unwrap();

        assert!(parse_config([
            "--web-root".to_owned(),
            ".".to_owned(),
            "--ssh-profile-json".to_owned(),
            profile_path.to_string_lossy().into_owned(),
            "--program".to_owned(),
            "/bin/sh".to_owned(),
        ])
        .is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn browser_open_command_uses_platform_default() {
        let command = browser_open_command("http://127.0.0.1:10000/index.html#session=test");

        #[cfg(all(target_family = "unix", not(target_os = "macos")))]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "xdg-open".to_owned(),
                args: vec!["http://127.0.0.1:10000/index.html#session=test".to_owned()],
            }
        );

        #[cfg(target_os = "macos")]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "open".to_owned(),
                args: vec!["http://127.0.0.1:10000/index.html#session=test".to_owned()],
            }
        );

        #[cfg(target_os = "windows")]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "cmd".to_owned(),
                args: vec![
                    "/C".to_owned(),
                    "start".to_owned(),
                    "".to_owned(),
                    "http://127.0.0.1:10000/index.html#session=test".to_owned(),
                ],
            }
        );
    }

    #[test]
    fn validate_external_url_allows_initial_hyperlink_schemes() {
        for uri in [
            "http://example.com",
            "https://example.com/docs?q=1",
            "mailto:dev@example.com",
            "HTTPS://example.com",
        ] {
            validate_external_url(uri).unwrap();
        }
    }

    #[test]
    fn validate_external_url_rejects_unsafe_or_unsupported_values() {
        for uri in [
            "",
            "example.com/no-scheme",
            "1http://example.com",
            "file:///tmp/example",
            "javascript:alert(1)",
            "https://example.com/\nnext",
        ] {
            assert!(validate_external_url(uri).is_err(), "{uri:?} should fail");
        }

        let oversized = format!("https://example.com/{}", "a".repeat(MAX_EXTERNAL_URL_BYTES));
        assert!(validate_external_url(&oversized).is_err());
    }

    #[test]
    fn external_url_open_command_validates_before_selecting_platform_opener() {
        assert!(external_url_open_command("file:///tmp/example").is_err());

        let command = external_url_open_command("https://example.com").unwrap();

        #[cfg(all(target_family = "unix", not(target_os = "macos")))]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "xdg-open".to_owned(),
                args: vec!["https://example.com".to_owned()],
            }
        );

        #[cfg(target_os = "macos")]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "open".to_owned(),
                args: vec!["https://example.com".to_owned()],
            }
        );

        #[cfg(target_os = "windows")]
        assert_eq!(
            command,
            BrowserOpenCommand {
                program: "cmd".to_owned(),
                args: vec![
                    "/C".to_owned(),
                    "start".to_owned(),
                    "".to_owned(),
                    "https://example.com".to_owned(),
                ],
            }
        );
    }

    fn write_test_asset(root: &Path, path: &str, body: &[u8]) {
        let file_path = root.join(path);
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(file_path, body).unwrap();
    }

    fn write_test_manifest(root: &Path, entries: &[(&str, &str, u64)]) {
        let assets = entries
            .iter()
            .map(|(path, content_type, bytes)| {
                format!(
                    r#"{{"path":"{path}","content_type":"{content_type}","sha256":"{}","bytes":{bytes}}}"#,
                    "0".repeat(64)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let manifest =
            format!(r#"{{"schema":1,"app":"witty-web","protocol":1,"assets":[{assets}]}}"#);
        fs::write(root.join(WEB_ASSET_MANIFEST_FILE), manifest).unwrap();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    fn launcher_http_response(http_state: LauncherHttpState, request: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let request = request.to_owned();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let web_assets = WebAssets {
                assets: BTreeMap::new(),
            };
            handle_http_connection(stream, &web_assets, &http_state).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        handle.join().unwrap();
        response
    }
}
