use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context as _, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const WITTY_LOG_ENV: &str = "WITTY_LOG";
const WITTY_LOG_DIR_ENV: &str = "WITTY_LOG_DIR";
const DEFAULT_WINDOW_LOG_FILTER: &str =
    "info,witty_app::fullscreen=debug,wgpu=warn,naga=warn";

#[derive(Debug)]
pub struct LoggingGuard {
    _file_guard: WorkerGuard,
}

pub fn init_window_logging() -> Result<LoggingGuard> {
    let log_dir = window_log_dir()?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("create Witty log directory {}", log_dir.display()))?;

    let filter_text = window_log_filter();
    let filter = EnvFilter::try_new(&filter_text).unwrap_or_else(|err| {
        eprintln!(
            "invalid {WITTY_LOG_ENV}={filter_text:?}; using {DEFAULT_WINDOW_LOG_FILTER:?}: {err}"
        );
        EnvFilter::new(DEFAULT_WINDOW_LOG_FILTER)
    });

    let file_appender = tracing_appender::rolling::daily(&log_dir, "witty.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .finish();

    let _ = tracing_log::LogTracer::init();
    tracing::subscriber::set_global_default(subscriber)
        .context("install Witty tracing subscriber")?;

    tracing::info!(
        target: "witty_app::logging",
        log_dir = %log_dir.display(),
        filter = %filter_text,
        debug_assertions = cfg!(debug_assertions),
        "witty logging initialized"
    );

    Ok(LoggingGuard {
        _file_guard: file_guard,
    })
}

fn window_log_filter() -> String {
    env::var(WITTY_LOG_ENV)
        .or_else(|_| env::var("RUST_LOG"))
        .unwrap_or_else(|_| DEFAULT_WINDOW_LOG_FILTER.to_owned())
}

fn window_log_dir() -> Result<PathBuf> {
    if let Some(value) = env::var_os(WITTY_LOG_DIR_ENV) {
        return Ok(PathBuf::from(value));
    }

    if let Some(value) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(value).join("witty").join("logs"));
    }

    let Some(home) = env::var_os("HOME") else {
        bail!("HOME is unset and XDG_STATE_HOME/{WITTY_LOG_DIR_ENV} are not set");
    };

    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("witty")
        .join("logs"))
}
