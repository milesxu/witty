use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use witty_core::GridSize;

use crate::{TerminalTransport, TransportEvent};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPtyConfig {
    pub size: GridSize,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

impl LocalPtyConfig {
    pub fn new(size: GridSize) -> Self {
        Self {
            size,
            program: None,
            args: Vec::new(),
            env: default_terminal_env(),
            cwd: None,
        }
    }

    pub fn command(size: GridSize, program: impl Into<String>) -> Self {
        Self {
            size,
            program: Some(program.into()),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
        }
    }

    pub fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub fn env(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn cwd(&mut self, cwd: impl Into<PathBuf>) -> &mut Self {
        self.cwd = Some(cwd.into());
        self
    }

    fn command_builder(&self) -> Result<CommandBuilder> {
        let Some(program) = &self.program else {
            if self.args.is_empty() {
                let mut command = CommandBuilder::new_default_prog();
                self.apply_command_options(&mut command);
                return Ok(command);
            }
            bail!("local pty args require an explicit program");
        };

        let mut command = CommandBuilder::new(program);
        command.args(&self.args);
        self.apply_command_options(&mut command);
        Ok(command)
    }

    fn apply_command_options(&self, command: &mut CommandBuilder) {
        for (key, value) in &self.env {
            command.env(key, value);
        }
        if let Some(cwd) = &self.cwd {
            command.cwd(cwd.as_os_str());
        }
    }
}

fn default_terminal_env() -> Vec<(String, String)> {
    vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
    ]
}

pub struct LocalPtyTransport {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    events: Receiver<TransportEvent>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    _reader_thread: JoinHandle<()>,
    _waiter_thread: JoinHandle<()>,
}

impl LocalPtyTransport {
    pub fn spawn_default(size: GridSize) -> Result<Self> {
        Self::spawn(LocalPtyConfig::new(size))
    }

    pub fn spawn(config: LocalPtyConfig) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(to_pty_size(config.size))
            .context("open local pty")?;
        let portable_pty::PtyPair { master, slave } = pair;

        let child = slave
            .spawn_command(config.command_builder()?)
            .context("spawn command in local pty")?;
        let reader = master.try_clone_reader().context("clone pty reader")?;
        let writer = master.take_writer().context("take pty writer")?;
        let killer = child.clone_killer();
        let (sender, events) = mpsc::channel();

        let reader_thread = {
            let sender = sender.clone();
            thread::spawn(move || read_pty(reader, sender))
        };
        let waiter_thread = thread::spawn(move || wait_child(child, sender));

        Ok(Self {
            master,
            writer,
            events,
            killer,
            _reader_thread: reader_thread,
            _waiter_thread: waiter_thread,
        })
    }
}

impl TerminalTransport for LocalPtyTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes).context("write to local pty")?;
        self.writer.flush().context("flush local pty writer")
    }

    fn resize(&mut self, size: GridSize) -> Result<()> {
        self.master
            .resize(to_pty_size(size))
            .context("resize local pty")
    }

    fn poll_event(&mut self) -> Result<Option<TransportEvent>> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }
}

impl Drop for LocalPtyTransport {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

fn to_pty_size(size: GridSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn read_pty(mut reader: Box<dyn Read + Send>, sender: Sender<TransportEvent>) {
    let mut buffer = [0_u8; 65_536];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if sender
                    .send(TransportEvent::Output(buffer[..count].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
                ) =>
            {
                break;
            }
            Err(err) => {
                let _ = sender.send(TransportEvent::Error(format!(
                    "local pty read failed: {err}"
                )));
                break;
            }
        }
    }
}

fn wait_child(mut child: Box<dyn Child + Send + Sync>, sender: Sender<TransportEvent>) {
    match child.wait() {
        Ok(status) => {
            let _ = sender.send(TransportEvent::Exit {
                code: Some(status.exit_code() as i32),
            });
        }
        Err(err) => {
            let _ = sender.send(TransportEvent::Error(format!(
                "local pty child wait failed: {err}"
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn default_shell_rejects_args() {
        let mut config = LocalPtyConfig::new(GridSize::new(24, 80));
        config.arg("-lc");

        assert!(config.command_builder().is_err());
    }

    #[test]
    fn default_shell_sets_terminal_environment() {
        let config = LocalPtyConfig::new(GridSize::new(24, 80));
        let command = config.command_builder().unwrap();

        assert_eq!(
            command
                .get_env("TERM")
                .map(|value| value.to_string_lossy().into_owned())
                .as_deref(),
            Some("xterm-256color")
        );
        assert_eq!(
            command
                .get_env("COLORTERM")
                .map(|value| value.to_string_lossy().into_owned())
                .as_deref(),
            Some("truecolor")
        );
    }

    #[test]
    fn command_builder_accepts_env_and_cwd_options() {
        let cwd = std::env::temp_dir();
        let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "dummy");
        config.env("TERM", "xterm-256color").cwd(&cwd);

        let command = config.command_builder().unwrap();

        assert_eq!(
            command
                .get_env("TERM")
                .map(|value| value.to_string_lossy().into_owned())
                .as_deref(),
            Some("xterm-256color")
        );
        assert_eq!(
            command
                .get_cwd()
                .map(|value| PathBuf::from(value.to_string_lossy().as_ref())),
            Some(cwd)
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_pty_captures_command_output_and_exit() {
        let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "/bin/sh");
        config.args(["-lc", "printf witty-pty; exit 7"]);
        let mut transport = LocalPtyTransport::spawn(config).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut exit_code = None;
        while Instant::now() < deadline {
            while let Some(event) = transport.poll_event().unwrap() {
                match event {
                    TransportEvent::Output(bytes) => output.extend(bytes),
                    TransportEvent::Exit { code } => exit_code = code,
                    TransportEvent::Error(err) => panic!("{err}"),
                }
            }

            if exit_code.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(String::from_utf8_lossy(&output).contains("witty-pty"));
        assert_eq!(exit_code, Some(7));
    }
}
