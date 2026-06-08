use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context as _, Result};
use serde::Serialize;
use witty_core::{BasicTerminal, GridSize, RenderSnapshot, TerminalHostAction};
use witty_transport::{LocalPtyConfig, LocalPtyTransport, TerminalTransport, TransportEvent};

const LESS_BASIC_RESTORE_CASE_ID: &str = "less-basic-restore";
const VIM_BASIC_EDIT_CASE_ID: &str = "vim-basic-edit";
const NVIM_BASIC_EDIT_CASE_ID: &str = "nvim-basic-edit";
const TMUX_BASIC_PANE_CASE_ID: &str = "tmux-basic-pane";
const HTOP_OR_BTOP_REDRAW_CASE_ID: &str = "htop-or-btop-redraw";
const VTTEST_SUBSET_CASE_ID: &str = "vttest-subset";
const LIST_REAL_TUI_SMOKE_CASES: &str = "list";
const RUN_ALL_REAL_TUI_SMOKE_CASES: &str = "all";
const VIM_EDIT_TOKEN: &str = "WITTY_vim_smoke_";
const TMUX_OSC52_PAYLOAD: &str = "VE1VWF9PU0M1Ml9PSw==";
const VTTEST_COMMANDS_ENV: &str = "WITTY_VTTEST_COMMANDS";
const REAL_TUI_SMOKE_SIZE: GridSize = GridSize { rows: 24, cols: 80 };
const REAL_TUI_SMOKE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_POLL_EVENTS_PER_TICK: usize = 256;
const MAX_SCREEN_SAMPLE_CHARS: usize = 2000;

pub fn run_real_tui_smoke_cli(case_id: &str) -> Result<()> {
    if case_id == LIST_REAL_TUI_SMOKE_CASES {
        println!("Real TUI smoke cases={}", real_tui_smoke_case_list_json()?);
        return Ok(());
    }
    if case_id == RUN_ALL_REAL_TUI_SMOKE_CASES {
        return run_all_real_tui_smokes_cli();
    }

    let mut outcome = run_real_tui_smoke(case_id)?;
    let artifact_path = write_real_tui_smoke_artifacts(&mut outcome)?;
    println!(
        "Real TUI smoke {} status={} artifact={}",
        outcome.report.case_id,
        outcome.report.status,
        artifact_path.display()
    );

    match outcome.report.status {
        RealTuiSmokeStatus::Passed | RealTuiSmokeStatus::Skipped => Ok(()),
        RealTuiSmokeStatus::Failed => bail!(
            "real TUI smoke {} failed; see {}",
            outcome.report.case_id,
            artifact_path.display()
        ),
    }
}

fn run_all_real_tui_smokes_cli() -> Result<()> {
    let suite = run_all_real_tui_smokes()?;
    let artifact_path = write_real_tui_smoke_suite_report(&suite)?;
    println!(
        "Real TUI smoke all status={} passed={} failed={} skipped={} artifact={}",
        suite.status,
        suite.passed,
        suite.failed,
        suite.skipped,
        artifact_path.display()
    );

    match suite.status {
        RealTuiSmokeStatus::Passed | RealTuiSmokeStatus::Skipped => Ok(()),
        RealTuiSmokeStatus::Failed => {
            bail!("real TUI smoke all failed; see {}", artifact_path.display())
        }
    }
}

fn run_all_real_tui_smokes() -> Result<RealTuiSmokeSuiteReport> {
    let started_at = Instant::now();
    let mut cases = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for case_id in real_tui_smoke_case_ids() {
        let mut outcome = run_real_tui_smoke(case_id)
            .unwrap_or_else(|error| failed_real_tui_smoke_outcome(case_id, error));
        let artifact_path = write_real_tui_smoke_artifacts(&mut outcome)?;

        match outcome.report.status {
            RealTuiSmokeStatus::Passed => passed += 1,
            RealTuiSmokeStatus::Failed => failed += 1,
            RealTuiSmokeStatus::Skipped => skipped += 1,
        }
        cases.push(RealTuiSmokeSuiteCase {
            case_id: outcome.report.case_id,
            status: outcome.report.status,
            artifact_path: artifact_path.display().to_string(),
            skip_reason: outcome.report.skip_reason,
            elapsed_ms: outcome.report.elapsed_ms,
        });
    }

    Ok(RealTuiSmokeSuiteReport {
        status: real_tui_smoke_suite_status(passed, failed, skipped),
        passed,
        failed,
        skipped,
        elapsed_ms: started_at.elapsed().as_millis(),
        cases,
    })
}

fn failed_real_tui_smoke_outcome(case_id: &str, error: anyhow::Error) -> RealTuiSmokeOutcome {
    let mut report = RealTuiSmokeReport::new(case_id);
    report.status = RealTuiSmokeStatus::Failed;
    report.push_assertion("case_completed", false, error.to_string());
    RealTuiSmokeOutcome {
        report,
        raw_output: None,
    }
}

fn run_real_tui_smoke(case_id: &str) -> Result<RealTuiSmokeOutcome> {
    match case_id {
        LESS_BASIC_RESTORE_CASE_ID => run_less_basic_restore_smoke(),
        VIM_BASIC_EDIT_CASE_ID => run_vim_basic_edit_smoke(),
        NVIM_BASIC_EDIT_CASE_ID => run_nvim_basic_edit_smoke(),
        TMUX_BASIC_PANE_CASE_ID => run_tmux_basic_pane_smoke(),
        HTOP_OR_BTOP_REDRAW_CASE_ID => run_htop_or_btop_redraw_smoke(),
        VTTEST_SUBSET_CASE_ID => run_vttest_subset_smoke(),
        _ => bail!(
            "unknown real TUI smoke case {case_id}; available cases: {}",
            real_tui_smoke_case_ids().join(", ")
        ),
    }
}

fn real_tui_smoke_case_ids() -> &'static [&'static str] {
    &[
        LESS_BASIC_RESTORE_CASE_ID,
        VIM_BASIC_EDIT_CASE_ID,
        NVIM_BASIC_EDIT_CASE_ID,
        TMUX_BASIC_PANE_CASE_ID,
        HTOP_OR_BTOP_REDRAW_CASE_ID,
        VTTEST_SUBSET_CASE_ID,
    ]
}

fn real_tui_smoke_case_list_json() -> serde_json::Result<String> {
    serde_json::to_string(real_tui_smoke_case_ids())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RealTuiSmokeSuiteReport {
    status: RealTuiSmokeStatus,
    passed: usize,
    failed: usize,
    skipped: usize,
    elapsed_ms: u128,
    cases: Vec<RealTuiSmokeSuiteCase>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RealTuiSmokeSuiteCase {
    case_id: String,
    status: RealTuiSmokeStatus,
    artifact_path: String,
    skip_reason: Option<String>,
    elapsed_ms: u128,
}

fn real_tui_smoke_suite_status(passed: usize, failed: usize, skipped: usize) -> RealTuiSmokeStatus {
    if failed > 0 {
        RealTuiSmokeStatus::Failed
    } else if passed == 0 && skipped > 0 {
        RealTuiSmokeStatus::Skipped
    } else {
        RealTuiSmokeStatus::Passed
    }
}

#[derive(Debug)]
struct RealTuiSmokeOutcome {
    report: RealTuiSmokeReport,
    raw_output: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RealTuiSmokeReport {
    case_id: String,
    status: RealTuiSmokeStatus,
    binary_path: Option<String>,
    version: Option<String>,
    skip_reason: Option<String>,
    exit_status: Option<i32>,
    output_bytes: usize,
    output_chunks: usize,
    host_reply_bytes: usize,
    clipboard_write_actions: usize,
    elapsed_ms: u128,
    final_screen_text_sample: String,
    assertions: Vec<RealTuiAssertion>,
    artifact_path: Option<String>,
    raw_capture_path: Option<String>,
}

impl RealTuiSmokeReport {
    fn new(case_id: &str) -> Self {
        Self {
            case_id: case_id.to_owned(),
            status: RealTuiSmokeStatus::Failed,
            binary_path: None,
            version: None,
            skip_reason: None,
            exit_status: None,
            output_bytes: 0,
            output_chunks: 0,
            host_reply_bytes: 0,
            clipboard_write_actions: 0,
            elapsed_ms: 0,
            final_screen_text_sample: String::new(),
            assertions: Vec::new(),
            artifact_path: None,
            raw_capture_path: None,
        }
    }

    fn skipped(case_id: &str, reason: impl Into<String>) -> Self {
        Self {
            status: RealTuiSmokeStatus::Skipped,
            skip_reason: Some(reason.into()),
            ..Self::new(case_id)
        }
    }

    fn push_assertion(&mut self, name: impl Into<String>, passed: bool, detail: impl Into<String>) {
        self.assertions.push(RealTuiAssertion {
            name: name.into(),
            passed,
            detail: detail.into(),
        });
    }

    fn finish_from_runtime(&mut self, runtime: &RealTuiRuntime, started_at: Instant) {
        self.exit_status = runtime.exit_code;
        self.output_bytes = runtime.output_bytes;
        self.output_chunks = runtime.output_chunks;
        self.host_reply_bytes = runtime.host_reply_bytes;
        self.clipboard_write_actions = runtime.clipboard_write_actions;
        self.elapsed_ms = started_at.elapsed().as_millis();
        self.final_screen_text_sample = screen_text_sample(&runtime.terminal.snapshot());
        if self.assertions.iter().all(|assertion| assertion.passed) {
            self.status = RealTuiSmokeStatus::Passed;
        } else {
            self.status = RealTuiSmokeStatus::Failed;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RealTuiSmokeStatus {
    Passed,
    Failed,
    Skipped,
}

impl fmt::Display for RealTuiSmokeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => f.write_str("passed"),
            Self::Failed => f.write_str("failed"),
            Self::Skipped => f.write_str("skipped"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RealTuiAssertion {
    name: String,
    passed: bool,
    detail: String,
}

struct RealTuiRuntime {
    transport: LocalPtyTransport,
    terminal: BasicTerminal,
    output_bytes: usize,
    output_chunks: usize,
    host_reply_bytes: usize,
    clipboard_write_actions: usize,
    exit_code: Option<i32>,
    raw_output: Option<Vec<u8>>,
}

impl RealTuiRuntime {
    fn spawn(config: LocalPtyConfig, capture_raw: bool) -> Result<Self> {
        Ok(Self {
            transport: LocalPtyTransport::spawn(config)?,
            terminal: BasicTerminal::new(REAL_TUI_SMOKE_SIZE),
            output_bytes: 0,
            output_chunks: 0,
            host_reply_bytes: 0,
            clipboard_write_actions: 0,
            exit_code: None,
            raw_output: capture_raw.then(Vec::new),
        })
    }

    fn write_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.transport.write(bytes)
    }

    fn wait_for_text(&mut self, needle: &str, deadline: Instant) -> Result<bool> {
        self.wait_until(deadline, |terminal| {
            screen_text(&terminal.snapshot()).contains(needle)
        })
    }

    fn wait_for_exit(&mut self, deadline: Instant) -> Result<bool> {
        while Instant::now() < deadline {
            self.poll_available()?;
            if self.exit_code.is_some() {
                return Ok(true);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.poll_available()?;
        Ok(self.exit_code.is_some())
    }

    fn wait_for_clipboard_actions_above(
        &mut self,
        previous_count: usize,
        deadline: Instant,
    ) -> Result<bool> {
        while Instant::now() < deadline {
            self.poll_available()?;
            if self.clipboard_write_actions > previous_count {
                return Ok(true);
            }
            if self.exit_code.is_some() {
                return Ok(self.clipboard_write_actions > previous_count);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.poll_available()?;
        Ok(self.clipboard_write_actions > previous_count)
    }

    fn wait_for_output_chunks_at_least(
        &mut self,
        min_chunks: usize,
        deadline: Instant,
    ) -> Result<bool> {
        while Instant::now() < deadline {
            self.poll_available()?;
            if self.output_chunks >= min_chunks {
                return Ok(true);
            }
            if self.exit_code.is_some() {
                return Ok(self.output_chunks >= min_chunks);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.poll_available()?;
        Ok(self.output_chunks >= min_chunks)
    }

    fn wait_until(
        &mut self,
        deadline: Instant,
        mut predicate: impl FnMut(&BasicTerminal) -> bool,
    ) -> Result<bool> {
        while Instant::now() < deadline {
            self.poll_available()?;
            if predicate(&self.terminal) {
                return Ok(true);
            }
            if self.exit_code.is_some() {
                return Ok(predicate(&self.terminal));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.poll_available()?;
        Ok(predicate(&self.terminal))
    }

    fn poll_available(&mut self) -> Result<()> {
        for _ in 0..MAX_POLL_EVENTS_PER_TICK {
            let Some(event) = self.transport.poll_event()? else {
                break;
            };
            match event {
                TransportEvent::Output(bytes) => {
                    self.output_bytes += bytes.len();
                    self.output_chunks += 1;
                    if let Some(raw_output) = &mut self.raw_output {
                        raw_output.extend_from_slice(&bytes);
                    }
                    self.terminal.feed(&bytes);
                    self.apply_host_actions()?;
                }
                TransportEvent::Exit { code } => {
                    self.exit_code = code;
                }
                TransportEvent::Error(err) => bail!("real TUI PTY error: {err}"),
            }
        }
        Ok(())
    }

    fn apply_host_actions(&mut self) -> Result<()> {
        for action in self.terminal.drain_host_actions() {
            match action {
                TerminalHostAction::TerminalReply(reply) => {
                    self.host_reply_bytes += reply.bytes.len();
                    self.transport.write(&reply.bytes)?;
                }
                TerminalHostAction::ClipboardWrite(_) => {
                    self.clipboard_write_actions += 1;
                }
                TerminalHostAction::ShellIntegration(_) => {}
                TerminalHostAction::CurrentDirectory(_) => {}
                TerminalHostAction::Bell => {}
            }
        }
        Ok(())
    }
}

fn run_less_basic_restore_smoke() -> Result<RealTuiSmokeOutcome> {
    let started_at = Instant::now();
    let mut report = RealTuiSmokeReport::new(LESS_BASIC_RESTORE_CASE_ID);
    let Some(less_path) = find_executable("less") else {
        let mut report = RealTuiSmokeReport::skipped(LESS_BASIC_RESTORE_CASE_ID, "less not found");
        report.elapsed_ms = started_at.elapsed().as_millis();
        return Ok(RealTuiSmokeOutcome {
            report,
            raw_output: None,
        });
    };
    report.binary_path = Some(less_path.display().to_string());
    report.version = command_version_first_line(&less_path);

    let work_dir = create_case_work_dir(LESS_BASIC_RESTORE_CASE_ID)?;
    let home_dir = work_dir.join("home");
    fs::create_dir_all(&home_dir)
        .with_context(|| format!("create smoke HOME {}", home_dir.display()))?;
    let input_path = work_dir.join("less-basic-restore.txt");
    fs::write(&input_path, less_fixture_text())
        .with_context(|| format!("write less fixture {}", input_path.display()))?;

    let mut config = LocalPtyConfig::command(
        REAL_TUI_SMOKE_SIZE,
        less_path.to_string_lossy().into_owned(),
    );
    config
        .args([
            "-R".to_owned(),
            "-M".to_owned(),
            input_path.to_string_lossy().into_owned(),
        ])
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("LESS", "")
        .env("HOME", home_dir.to_string_lossy().into_owned())
        .cwd(&work_dir);

    let capture_raw = capture_raw_real_tui_smoke();
    let mut runtime = RealTuiRuntime::spawn(config, capture_raw)
        .with_context(|| format!("spawn less at {}", less_path.display()))?;
    let deadline = Instant::now() + REAL_TUI_SMOKE_TIMEOUT;

    let initial_visible = runtime.wait_for_text("Line 001", deadline)?;
    report.push_assertion(
        "initial_pager_text_visible",
        initial_visible,
        "screen should show the first fixture line after less starts",
    );

    runtime.write_input(b"/Line 050\r")?;
    let search_visible = runtime.wait_for_text("Line 050", deadline)?;
    report.push_assertion(
        "searched_line_visible",
        search_visible,
        "searching for Line 050 should make that line visible",
    );

    runtime.write_input(b"n")?;
    runtime.poll_available()?;
    runtime.write_input(b"q")?;
    let exited = runtime.wait_for_exit(deadline)?;
    report.push_assertion(
        "process_exited",
        exited,
        "less should exit after q before the case deadline",
    );
    report.push_assertion(
        "process_exit_success",
        runtime.exit_code == Some(0),
        format!("expected exit status 0, got {:?}", runtime.exit_code),
    );

    runtime.poll_available()?;
    let final_screen = screen_text(&runtime.terminal.snapshot());
    report.push_assertion(
        "pager_content_restored_off_main_screen",
        !final_screen.contains("Line 050") && !final_screen.contains("Line 001"),
        "final main-screen snapshot should not retain alternate-screen pager content",
    );
    report.finish_from_runtime(&runtime, started_at);

    let _ = fs::remove_dir_all(&work_dir);

    Ok(RealTuiSmokeOutcome {
        report,
        raw_output: runtime.raw_output,
    })
}

fn run_vim_basic_edit_smoke() -> Result<RealTuiSmokeOutcome> {
    run_editor_basic_edit_smoke(VIM_BASIC_EDIT_CASE_ID, "vim", vim_basic_edit_args)
}

fn run_nvim_basic_edit_smoke() -> Result<RealTuiSmokeOutcome> {
    run_editor_basic_edit_smoke(NVIM_BASIC_EDIT_CASE_ID, "nvim", nvim_basic_edit_args)
}

fn run_editor_basic_edit_smoke(
    case_id: &str,
    binary_name: &str,
    args_for_file: fn(&Path) -> Vec<String>,
) -> Result<RealTuiSmokeOutcome> {
    let started_at = Instant::now();
    let mut report = RealTuiSmokeReport::new(case_id);
    let Some(editor_path) = find_executable(binary_name) else {
        let mut report = RealTuiSmokeReport::skipped(case_id, format!("{binary_name} not found"));
        report.elapsed_ms = started_at.elapsed().as_millis();
        return Ok(RealTuiSmokeOutcome {
            report,
            raw_output: None,
        });
    };
    report.binary_path = Some(editor_path.display().to_string());
    report.version = command_version_first_line(&editor_path);

    let work_dir = create_case_work_dir(case_id)?;
    let home_dir = work_dir.join("home");
    let xdg_config_home = work_dir.join("xdg-config");
    let xdg_data_home = work_dir.join("xdg-data");
    let xdg_state_home = work_dir.join("xdg-state");
    let xdg_cache_home = work_dir.join("xdg-cache");
    for dir in [
        &home_dir,
        &xdg_config_home,
        &xdg_data_home,
        &xdg_state_home,
        &xdg_cache_home,
    ] {
        fs::create_dir_all(dir)
            .with_context(|| format!("create smoke support dir {}", dir.display()))?;
    }

    let input_path = work_dir.join(format!("{case_id}.txt"));
    fs::write(&input_path, editor_fixture_text())
        .with_context(|| format!("write editor fixture {}", input_path.display()))?;

    let mut config = LocalPtyConfig::command(
        REAL_TUI_SMOKE_SIZE,
        editor_path.to_string_lossy().into_owned(),
    );
    config
        .args(args_for_file(&input_path))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", home_dir.to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            xdg_config_home.to_string_lossy().into_owned(),
        )
        .env(
            "XDG_DATA_HOME",
            xdg_data_home.to_string_lossy().into_owned(),
        )
        .env(
            "XDG_STATE_HOME",
            xdg_state_home.to_string_lossy().into_owned(),
        )
        .env(
            "XDG_CACHE_HOME",
            xdg_cache_home.to_string_lossy().into_owned(),
        )
        .env("VIMINIT", "")
        .env("GVIMINIT", "")
        .env("EXINIT", "")
        .cwd(&work_dir);

    let capture_raw = capture_raw_real_tui_smoke();
    let mut runtime = RealTuiRuntime::spawn(config, capture_raw)
        .with_context(|| format!("spawn {binary_name} at {}", editor_path.display()))?;
    let deadline = Instant::now() + REAL_TUI_SMOKE_TIMEOUT;

    let active_visible = runtime.wait_until(deadline, |terminal| {
        let text = screen_text(&terminal.snapshot());
        text.contains("Witty editor smoke fixture")
            || text.contains(case_id)
            || text.contains("[No Name]")
    })?;
    report.push_assertion(
        "editor_screen_visible",
        active_visible,
        "screen should show fixture text, file name, or an editor status line after startup",
    );

    let insert_command = format!("gg0i{VIM_EDIT_TOKEN}\x1b");
    runtime.write_input(insert_command.as_bytes())?;
    let inserted_visible = runtime.wait_for_text(VIM_EDIT_TOKEN, deadline)?;
    report.push_assertion(
        "inserted_text_visible",
        inserted_visible,
        "inserted smoke token should be visible before writing the file",
    );

    runtime.write_input(b":wq\r")?;
    let exited = runtime.wait_for_exit(deadline)?;
    report.push_assertion(
        "process_exited",
        exited,
        "editor should exit after :wq before the case deadline",
    );
    report.push_assertion(
        "process_exit_success",
        runtime.exit_code == Some(0),
        format!("expected exit status 0, got {:?}", runtime.exit_code),
    );

    let file_text = fs::read_to_string(&input_path).unwrap_or_default();
    report.push_assertion(
        "file_starts_with_inserted_token",
        file_text.starts_with(VIM_EDIT_TOKEN),
        "saved file should begin with the inserted smoke token",
    );
    report.push_assertion(
        "original_fixture_text_preserved",
        file_text.contains("Witty editor smoke fixture"),
        "saved file should still contain the original fixture text after insertion",
    );

    runtime.poll_available()?;
    let final_screen = screen_text(&runtime.terminal.snapshot());
    report.push_assertion(
        "editor_content_restored_off_main_screen",
        !final_screen.contains(VIM_EDIT_TOKEN)
            && !final_screen.contains("Witty editor smoke fixture"),
        "final main-screen snapshot should not retain alternate-screen editor content",
    );
    report.finish_from_runtime(&runtime, started_at);

    let _ = fs::remove_dir_all(&work_dir);

    Ok(RealTuiSmokeOutcome {
        report,
        raw_output: runtime.raw_output,
    })
}

fn vim_basic_edit_args(input_path: &Path) -> Vec<String> {
    vec![
        "-Nu".to_owned(),
        "NONE".to_owned(),
        "-n".to_owned(),
        "-i".to_owned(),
        "NONE".to_owned(),
        "-N".to_owned(),
        input_path.to_string_lossy().into_owned(),
    ]
}

fn nvim_basic_edit_args(input_path: &Path) -> Vec<String> {
    vec![
        "--clean".to_owned(),
        "-n".to_owned(),
        input_path.to_string_lossy().into_owned(),
    ]
}

fn run_tmux_basic_pane_smoke() -> Result<RealTuiSmokeOutcome> {
    let started_at = Instant::now();
    let mut report = RealTuiSmokeReport::new(TMUX_BASIC_PANE_CASE_ID);
    let Some(tmux_path) = find_executable("tmux") else {
        let mut report = RealTuiSmokeReport::skipped(TMUX_BASIC_PANE_CASE_ID, "tmux not found");
        report.elapsed_ms = started_at.elapsed().as_millis();
        return Ok(RealTuiSmokeOutcome {
            report,
            raw_output: None,
        });
    };
    report.binary_path = Some(tmux_path.display().to_string());
    report.version = command_version_first_line_with_args(&tmux_path, ["-V"]);

    let work_dir = create_case_work_dir(TMUX_BASIC_PANE_CASE_ID)?;
    let home_dir = work_dir.join("home");
    fs::create_dir_all(&home_dir)
        .with_context(|| format!("create smoke HOME {}", home_dir.display()))?;
    let socket_path = work_dir.join("tmux.sock");
    let config_path = work_dir.join("tmux.conf");
    let osc52_script_path = work_dir.join("emit-osc52.sh");
    fs::write(&config_path, tmux_smoke_config_text())
        .with_context(|| format!("write tmux smoke config {}", config_path.display()))?;
    fs::write(&osc52_script_path, tmux_osc52_script_text())
        .with_context(|| format!("write tmux OSC 52 script {}", osc52_script_path.display()))?;

    let mut config = LocalPtyConfig::command(
        REAL_TUI_SMOKE_SIZE,
        tmux_path.to_string_lossy().into_owned(),
    );
    config
        .args(tmux_basic_pane_args(&socket_path, &config_path))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", home_dir.to_string_lossy().into_owned())
        .cwd(&work_dir);

    let capture_raw = capture_raw_real_tui_smoke();
    let mut runtime = RealTuiRuntime::spawn(config, capture_raw)
        .with_context(|| format!("spawn tmux at {}", tmux_path.display()))?;
    let deadline = Instant::now() + REAL_TUI_SMOKE_TIMEOUT;

    let ready_visible = runtime.wait_for_text("TMUX READY", deadline)?;
    report.push_assertion(
        "initial_tmux_pane_visible",
        ready_visible,
        "attached tmux session should show initial pane output",
    );

    runtime.write_input(b"\x02\"")?;
    std::thread::sleep(Duration::from_millis(100));
    runtime.poll_available()?;
    runtime.write_input(b"printf 'TMUX PANE OK\\n'\r")?;
    let pane_output_visible = runtime.wait_for_text("TMUX PANE OK", deadline)?;
    report.push_assertion(
        "split_pane_output_visible",
        pane_output_visible,
        "output from the active tmux pane should be visible after split-pane",
    );

    runtime.write_input(b"stty -echo\r")?;
    std::thread::sleep(Duration::from_millis(100));
    runtime.poll_available()?;
    let clipboard_actions_before = runtime.clipboard_write_actions;
    let osc52_command = format!("sh {}\r", osc52_script_path.display());
    runtime.write_input(osc52_command.as_bytes())?;
    let osc52_forwarded =
        runtime.wait_for_clipboard_actions_above(clipboard_actions_before, deadline)?;
    report.push_assertion(
        "tmux_osc52_forwarded_as_host_action",
        osc52_forwarded,
        "tmux set-clipboard=on should forward pane OSC 52 output as a terminal host action",
    );

    let screen_after_osc52 = screen_text(&runtime.terminal.snapshot());
    report.push_assertion(
        "osc52_payload_not_rendered",
        !screen_after_osc52.contains(TMUX_OSC52_PAYLOAD),
        "OSC 52 payload should not be rendered into terminal cells",
    );

    let pane_count = tmux_pane_count(&tmux_path, &socket_path).unwrap_or_default();
    report.push_assertion(
        "split_pane_count_observed",
        pane_count >= 2,
        format!("expected at least 2 tmux panes, observed {pane_count}"),
    );

    runtime.write_input(b"\x02d")?;
    let exited = runtime.wait_for_exit(deadline)?;
    report.push_assertion(
        "client_detached",
        exited,
        "tmux client should exit after prefix d before the case deadline",
    );
    report.push_assertion(
        "client_exit_success",
        runtime.exit_code == Some(0),
        format!("expected client exit status 0, got {:?}", runtime.exit_code),
    );

    runtime.poll_available()?;
    let final_screen = screen_text(&runtime.terminal.snapshot());
    report.push_assertion(
        "tmux_content_restored_off_main_screen",
        !final_screen.contains("TMUX READY") && !final_screen.contains("TMUX PANE OK"),
        "final main-screen snapshot should not retain full-screen tmux content",
    );
    report.finish_from_runtime(&runtime, started_at);

    let _ = kill_tmux_server(&tmux_path, &socket_path);
    let _ = fs::remove_dir_all(&work_dir);

    Ok(RealTuiSmokeOutcome {
        report,
        raw_output: runtime.raw_output,
    })
}

fn tmux_basic_pane_args(socket_path: &Path, config_path: &Path) -> Vec<String> {
    vec![
        "-S".to_owned(),
        socket_path.to_string_lossy().into_owned(),
        "-f".to_owned(),
        config_path.to_string_lossy().into_owned(),
        "-u".to_owned(),
        "new-session".to_owned(),
        "-s".to_owned(),
        "witty-smoke".to_owned(),
        "/bin/sh -lc 'printf \"TMUX READY\\n\"; exec /bin/sh'".to_owned(),
    ]
}

fn tmux_smoke_config_text() -> String {
    format!(
        "\
set -g default-terminal \"{}\"
set -g set-clipboard on
set -g status on
set -g status-left \"Witty\"
set -g status-right \"\"
set -g prefix C-b
",
        tmux_default_terminal()
    )
}

fn tmux_osc52_script_text() -> String {
    format!("printf '\\033]52;c;{TMUX_OSC52_PAYLOAD}\\033\\\\'\n")
}

fn tmux_default_terminal() -> &'static str {
    if terminfo_name_available("tmux-256color") {
        "tmux-256color"
    } else {
        "screen-256color"
    }
}

fn terminfo_name_available(name: &str) -> bool {
    std::process::Command::new("infocmp")
        .arg("-x")
        .arg(name)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn tmux_pane_count(tmux_path: &Path, socket_path: &Path) -> Result<usize> {
    let output = std::process::Command::new(tmux_path)
        .arg("-S")
        .arg(socket_path)
        .args(["list-panes", "-F", "#{pane_id}"])
        .output()
        .with_context(|| format!("list tmux panes with socket {}", socket_path.display()))?;
    if !output.status.success() {
        bail!(
            "tmux list-panes failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().count())
}

fn kill_tmux_server(tmux_path: &Path, socket_path: &Path) -> Result<()> {
    let output = std::process::Command::new(tmux_path)
        .arg("-S")
        .arg(socket_path)
        .arg("kill-server")
        .output()
        .with_context(|| format!("kill tmux server with socket {}", socket_path.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "tmux kill-server failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn run_htop_or_btop_redraw_smoke() -> Result<RealTuiSmokeOutcome> {
    let started_at = Instant::now();
    let mut report = RealTuiSmokeReport::new(HTOP_OR_BTOP_REDRAW_CASE_ID);
    let Some(tool) = select_process_viewer_tool() else {
        let mut report =
            RealTuiSmokeReport::skipped(HTOP_OR_BTOP_REDRAW_CASE_ID, "neither htop nor btop found");
        report.elapsed_ms = started_at.elapsed().as_millis();
        return Ok(RealTuiSmokeOutcome {
            report,
            raw_output: None,
        });
    };
    report.binary_path = Some(tool.path.display().to_string());
    report.version = command_version_first_line(&tool.path);

    let work_dir = create_case_work_dir(HTOP_OR_BTOP_REDRAW_CASE_ID)?;
    let home_dir = work_dir.join("home");
    let xdg_config_home = work_dir.join("xdg-config");
    let xdg_cache_home = work_dir.join("xdg-cache");
    let xdg_state_home = work_dir.join("xdg-state");
    for dir in [
        &home_dir,
        &xdg_config_home,
        &xdg_cache_home,
        &xdg_state_home,
    ] {
        fs::create_dir_all(dir)
            .with_context(|| format!("create smoke support dir {}", dir.display()))?;
    }

    let mut config = LocalPtyConfig::command(
        REAL_TUI_SMOKE_SIZE,
        tool.path.to_string_lossy().into_owned(),
    );
    config
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", home_dir.to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            xdg_config_home.to_string_lossy().into_owned(),
        )
        .env(
            "XDG_CACHE_HOME",
            xdg_cache_home.to_string_lossy().into_owned(),
        )
        .env(
            "XDG_STATE_HOME",
            xdg_state_home.to_string_lossy().into_owned(),
        )
        .cwd(&work_dir);

    let capture_raw = capture_raw_real_tui_smoke();
    let mut runtime = RealTuiRuntime::spawn(config, capture_raw)
        .with_context(|| format!("spawn {} at {}", tool.name, tool.path.display()))?;
    let deadline = Instant::now() + REAL_TUI_SMOKE_TIMEOUT;

    let marker_visible = runtime.wait_until(deadline, |terminal| {
        process_viewer_marker_visible(tool.name, &screen_text(&terminal.snapshot()))
    })?;
    report.push_assertion(
        "process_table_marker_visible",
        marker_visible,
        format!(
            "{} should render a recognizable process/CPU table marker",
            tool.name
        ),
    );

    let redraw_observed = runtime.wait_for_output_chunks_at_least(2, deadline)?;
    report.push_assertion(
        "multiple_output_bursts_observed",
        redraw_observed,
        format!(
            "{} should emit at least two PTY output bursts, observed {}",
            tool.name, runtime.output_chunks
        ),
    );

    runtime.write_input(b"q")?;
    let exited = runtime.wait_for_exit(deadline)?;
    report.push_assertion(
        "process_exited",
        exited,
        format!("{} should exit after q before the case deadline", tool.name),
    );
    report.push_assertion(
        "process_exit_success",
        runtime.exit_code == Some(0),
        format!("expected exit status 0, got {:?}", runtime.exit_code),
    );

    runtime.poll_available()?;
    report.finish_from_runtime(&runtime, started_at);

    let _ = fs::remove_dir_all(&work_dir);

    Ok(RealTuiSmokeOutcome {
        report,
        raw_output: runtime.raw_output,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessViewerTool {
    name: &'static str,
    path: PathBuf,
}

fn select_process_viewer_tool() -> Option<ProcessViewerTool> {
    find_executable("htop")
        .map(|path| ProcessViewerTool { name: "htop", path })
        .or_else(|| find_executable("btop").map(|path| ProcessViewerTool { name: "btop", path }))
}

fn process_viewer_marker_visible(tool_name: &str, screen: &str) -> bool {
    let lower = screen.to_ascii_lowercase();
    match tool_name {
        "htop" => {
            (lower.contains("pid") && (lower.contains("command") || lower.contains("user")))
                || (lower.contains("tasks") && lower.contains("load average"))
        }
        "btop" => {
            lower.contains("btop")
                || (lower.contains("cpu") && lower.contains("mem"))
                || (lower.contains("proc") && lower.contains("pid"))
        }
        _ => false,
    }
}

fn run_vttest_subset_smoke() -> Result<RealTuiSmokeOutcome> {
    let started_at = Instant::now();
    let mut report = RealTuiSmokeReport::new(VTTEST_SUBSET_CASE_ID);
    let Some(vttest_path) = find_executable("vttest") else {
        let mut report = RealTuiSmokeReport::skipped(VTTEST_SUBSET_CASE_ID, "vttest not found");
        report.elapsed_ms = started_at.elapsed().as_millis();
        return Ok(RealTuiSmokeOutcome {
            report,
            raw_output: None,
        });
    };
    report.binary_path = Some(vttest_path.display().to_string());
    report.version = command_version_first_line_with_args(&vttest_path, ["-V"]);

    let work_dir = create_case_work_dir(VTTEST_SUBSET_CASE_ID)?;
    let home_dir = work_dir.join("home");
    fs::create_dir_all(&home_dir)
        .with_context(|| format!("create smoke HOME {}", home_dir.display()))?;

    let command_script_path = vttest_command_script_path()?;
    let mut config = LocalPtyConfig::command(
        REAL_TUI_SMOKE_SIZE,
        vttest_path.to_string_lossy().into_owned(),
    );
    config
        .args(vttest_subset_args(command_script_path.as_deref()))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", home_dir.to_string_lossy().into_owned())
        .cwd(&work_dir);

    let capture_raw = capture_raw_real_tui_smoke();
    let mut runtime = RealTuiRuntime::spawn(config, capture_raw)
        .with_context(|| format!("spawn vttest at {}", vttest_path.display()))?;
    let deadline = Instant::now() + REAL_TUI_SMOKE_TIMEOUT;

    let startup_visible = runtime.wait_until(deadline, |terminal| {
        vttest_startup_marker_visible(&screen_text(&terminal.snapshot()))
    })?;
    report.push_assertion(
        "startup_or_menu_marker_visible",
        startup_visible,
        "vttest should render its startup/menu text before scripted input runs",
    );

    if command_script_path.is_none() {
        runtime.write_input(b"0\r")?;
    }
    let exited = runtime.wait_for_exit(deadline)?;
    report.push_assertion(
        "process_exited",
        exited,
        format!(
            "vttest should exit before the case deadline{}",
            if command_script_path.is_some() {
                " after replaying the command script"
            } else {
                " after selecting menu item 0"
            }
        ),
    );
    report.push_assertion(
        "process_exit_success",
        runtime.exit_code == Some(0),
        format!("expected exit status 0, got {:?}", runtime.exit_code),
    );
    if let Some(path) = &command_script_path {
        report.push_assertion(
            "command_script_configured",
            true,
            format!("replayed vttest command script {}", path.display()),
        );
    } else {
        report.push_assertion(
            "menu_probe_only",
            true,
            format!("set {VTTEST_COMMANDS_ENV} to replay a recorded vttest subset"),
        );
    }

    runtime.poll_available()?;
    report.finish_from_runtime(&runtime, started_at);

    let _ = fs::remove_dir_all(&work_dir);

    Ok(RealTuiSmokeOutcome {
        report,
        raw_output: runtime.raw_output,
    })
}

fn vttest_command_script_path() -> Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os(VTTEST_COMMANDS_ENV).map(PathBuf::from) else {
        return Ok(None);
    };
    if path.is_file() {
        Ok(Some(path))
    } else {
        bail!(
            "{VTTEST_COMMANDS_ENV} must point to a vttest command file: {}",
            path.display()
        )
    }
}

fn vttest_subset_args(command_script_path: Option<&Path>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(path) = command_script_path {
        args.push("-c".to_owned());
        args.push(path.to_string_lossy().into_owned());
    }
    args.push("24x80.80".to_owned());
    args
}

fn vttest_startup_marker_visible(screen: &str) -> bool {
    let lower = screen.to_ascii_lowercase();
    lower.contains("vttest")
        || lower.contains("vt100")
        || lower.contains("choose test")
        || lower.contains("test of cursor")
        || lower.contains("terminal")
}

fn write_real_tui_smoke_artifacts(outcome: &mut RealTuiSmokeOutcome) -> Result<PathBuf> {
    let artifact_dir = real_tui_smoke_artifact_dir();
    fs::create_dir_all(&artifact_dir).with_context(|| {
        format!(
            "create real TUI smoke artifact dir {}",
            artifact_dir.display()
        )
    })?;

    if let Some(raw_output) = &outcome.raw_output {
        let raw_path = artifact_dir.join(format!(
            "{}.raw",
            safe_artifact_name(&outcome.report.case_id)
        ));
        fs::write(&raw_path, raw_output)
            .with_context(|| format!("write raw smoke capture {}", raw_path.display()))?;
        outcome.report.raw_capture_path = Some(raw_path.display().to_string());
    }

    let artifact_path = artifact_dir.join(format!(
        "{}.json",
        safe_artifact_name(&outcome.report.case_id)
    ));
    outcome.report.artifact_path = Some(artifact_path.display().to_string());
    let json = serde_json::to_vec_pretty(&outcome.report)?;
    fs::write(&artifact_path, json)
        .with_context(|| format!("write real TUI smoke report {}", artifact_path.display()))?;
    Ok(artifact_path)
}

fn write_real_tui_smoke_suite_report(suite: &RealTuiSmokeSuiteReport) -> Result<PathBuf> {
    let artifact_dir = real_tui_smoke_artifact_dir();
    fs::create_dir_all(&artifact_dir).with_context(|| {
        format!(
            "create real TUI smoke artifact dir {}",
            artifact_dir.display()
        )
    })?;

    let artifact_path = artifact_dir.join("all.json");
    let json = serde_json::to_vec_pretty(suite)?;
    fs::write(&artifact_path, json).with_context(|| {
        format!(
            "write real TUI smoke suite report {}",
            artifact_path.display()
        )
    })?;
    Ok(artifact_path)
}

fn real_tui_smoke_artifact_dir() -> PathBuf {
    std::env::var_os("WITTY_TUI_SMOKE_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/real-tui-smoke"))
}

fn capture_raw_real_tui_smoke() -> bool {
    std::env::var_os("WITTY_TUI_SMOKE_CAPTURE_RAW").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn safe_artifact_name(case_id: &str) -> String {
    case_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn create_case_work_dir(case_id: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "witty-real-tui-{}-{}-{}",
        safe_artifact_name(case_id),
        std::process::id(),
        now_nanos()
    ));
    fs::create_dir_all(&dir).with_context(|| format!("create smoke work dir {}", dir.display()))?;
    Ok(dir)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn less_fixture_text() -> String {
    let mut text = String::new();
    for line in 1..=120 {
        text.push_str(&format!(
            "Line {line:03} Witty real TUI smoke fixture row {line:03}\n"
        ));
    }
    text
}

fn editor_fixture_text() -> String {
    [
        "Witty editor smoke fixture",
        "This file is edited by a real terminal application.",
        "The first line should receive a deterministic prefix.",
    ]
    .join("\n")
        + "\n"
}

fn command_version_first_line(path: &Path) -> Option<String> {
    command_version_first_line_with_args(path, ["--version"])
}

fn command_version_first_line_with_args<I, S>(path: &Path, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new(path).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return is_executable_file(path).then(|| path.to_path_buf());
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    is_executable_metadata(&metadata)
}

#[cfg(unix)]
fn is_executable_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable_metadata(_metadata: &fs::Metadata) -> bool {
    true
}

fn screen_text_sample(snapshot: &RenderSnapshot) -> String {
    let text = screen_text(snapshot);
    text.chars().take(MAX_SCREEN_SAMPLE_CHARS).collect()
}

fn screen_text(snapshot: &RenderSnapshot) -> String {
    snapshot
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_tui_smoke_case_registry_lists_less_basic_restore() {
        assert_eq!(
            real_tui_smoke_case_ids(),
            [
                LESS_BASIC_RESTORE_CASE_ID,
                VIM_BASIC_EDIT_CASE_ID,
                NVIM_BASIC_EDIT_CASE_ID,
                TMUX_BASIC_PANE_CASE_ID,
                HTOP_OR_BTOP_REDRAW_CASE_ID,
                VTTEST_SUBSET_CASE_ID,
            ]
        );
    }

    #[test]
    fn real_tui_smoke_case_list_json_is_machine_readable() {
        let cases: Vec<String> = serde_json::from_str(&real_tui_smoke_case_list_json().unwrap())
            .expect("valid case list json");

        assert_eq!(
            cases,
            [
                LESS_BASIC_RESTORE_CASE_ID,
                VIM_BASIC_EDIT_CASE_ID,
                NVIM_BASIC_EDIT_CASE_ID,
                TMUX_BASIC_PANE_CASE_ID,
                HTOP_OR_BTOP_REDRAW_CASE_ID,
                VTTEST_SUBSET_CASE_ID,
            ]
        );
        assert!(!cases.contains(&LIST_REAL_TUI_SMOKE_CASES.to_owned()));
        assert!(!cases.contains(&RUN_ALL_REAL_TUI_SMOKE_CASES.to_owned()));
    }

    #[test]
    fn real_tui_smoke_suite_status_tracks_failures_and_all_skipped() {
        assert_eq!(
            real_tui_smoke_suite_status(1, 0, 5),
            RealTuiSmokeStatus::Passed
        );
        assert_eq!(
            real_tui_smoke_suite_status(0, 1, 5),
            RealTuiSmokeStatus::Failed
        );
        assert_eq!(
            real_tui_smoke_suite_status(0, 0, 6),
            RealTuiSmokeStatus::Skipped
        );
    }

    #[test]
    fn failed_real_tui_smoke_outcome_preserves_error_as_failed_assertion() {
        let outcome = failed_real_tui_smoke_outcome("broken-case", anyhow::anyhow!("spawn failed"));

        assert_eq!(outcome.report.case_id, "broken-case");
        assert_eq!(outcome.report.status, RealTuiSmokeStatus::Failed);
        assert_eq!(outcome.report.assertions.len(), 1);
        assert_eq!(outcome.report.assertions[0].name, "case_completed");
        assert!(!outcome.report.assertions[0].passed);
        assert_eq!(outcome.report.assertions[0].detail, "spawn failed");
    }

    #[test]
    fn safe_artifact_name_replaces_path_separators_and_spaces() {
        assert_eq!(safe_artifact_name("../less basic"), "___less_basic");
    }

    #[test]
    fn less_fixture_contains_search_target_and_enough_rows() {
        let fixture = less_fixture_text();

        assert!(fixture.contains("Line 001"));
        assert!(fixture.contains("Line 050"));
        assert_eq!(fixture.lines().count(), 120);
    }

    #[test]
    fn editor_fixture_and_args_are_deterministic() {
        let input = Path::new("/tmp/witty-editor-smoke.txt");
        let fixture = editor_fixture_text();

        assert!(fixture.starts_with("Witty editor smoke fixture"));
        assert!(fixture.ends_with('\n'));
        assert_eq!(
            vim_basic_edit_args(input),
            [
                "-Nu",
                "NONE",
                "-n",
                "-i",
                "NONE",
                "-N",
                "/tmp/witty-editor-smoke.txt",
            ]
        );
        assert_eq!(
            nvim_basic_edit_args(input),
            ["--clean", "-n", "/tmp/witty-editor-smoke.txt"]
        );
    }

    #[test]
    fn tmux_args_and_config_are_isolated_and_enable_clipboard_forwarding() {
        let socket = Path::new("/tmp/witty-tmux.sock");
        let config = Path::new("/tmp/witty-tmux.conf");
        let args = tmux_basic_pane_args(socket, config);
        let config_text = tmux_smoke_config_text();

        assert_eq!(args[0], "-S");
        assert_eq!(args[1], "/tmp/witty-tmux.sock");
        assert_eq!(args[2], "-f");
        assert_eq!(args[3], "/tmp/witty-tmux.conf");
        assert!(args.contains(&"-u".to_owned()));
        assert!(args.contains(&"new-session".to_owned()));
        assert!(config_text.contains("set -g set-clipboard on"));
        assert!(config_text.contains("set -g status-left \"Witty\""));
        assert!(tmux_osc52_script_text().contains(TMUX_OSC52_PAYLOAD));
    }

    #[test]
    fn process_viewer_marker_detection_accepts_htop_and_btop_tables() {
        assert!(process_viewer_marker_visible(
            "htop",
            "PID USER PRI NI VIRT RES SHR S CPU% MEM% TIME+ Command"
        ));
        assert!(process_viewer_marker_visible(
            "htop",
            "Tasks: 88, 102 thr; Load average: 0.10 0.20 0.30"
        ));
        assert!(process_viewer_marker_visible(
            "btop",
            "btop cpu mem proc pid command"
        ));
        assert!(!process_viewer_marker_visible("htop", "plain shell prompt"));
        assert!(!process_viewer_marker_visible(
            "unknown",
            "PID USER COMMAND"
        ));
    }

    #[test]
    fn vttest_subset_args_pin_screen_geometry_without_recorded_script() {
        assert_eq!(vttest_subset_args(None), ["24x80.80"]);
    }

    #[test]
    fn vttest_subset_args_replay_recorded_script_when_configured() {
        let script = Path::new("/tmp/witty-vttest.commands");

        assert_eq!(
            vttest_subset_args(Some(script)),
            ["-c", "/tmp/witty-vttest.commands", "24x80.80"]
        );
    }

    #[test]
    fn vttest_startup_marker_detection_accepts_common_menu_text() {
        assert!(vttest_startup_marker_visible(
            "VTTEST\nChoose test type\n1. Test of cursor movements"
        ));
        assert!(vttest_startup_marker_visible("VT100 terminal test"));
        assert!(!vttest_startup_marker_visible("plain shell prompt"));
    }

    #[test]
    fn smoke_report_serializes_skipped_status_without_raw_output() {
        let report = RealTuiSmokeReport::skipped("missing-tool", "tool not found");

        let json = serde_json::to_string(&report).unwrap();

        assert!(json.contains("\"status\":\"skipped\""));
        assert!(json.contains("\"skip_reason\":\"tool not found\""));
        assert!(!json.contains("raw output"));
    }

    #[cfg(unix)]
    #[test]
    fn find_executable_uses_path_entries_and_executable_bit() {
        let dir = create_case_work_dir("find-executable-test").unwrap();
        let tool = dir.join("witty-fake-tool");
        fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&tool).unwrap().permissions();
        {
            use std::os::unix::fs::PermissionsExt as _;
            permissions.set_mode(0o755);
        }
        fs::set_permissions(&tool, permissions).unwrap();

        let original_path = std::env::var_os("PATH").unwrap_or_default();
        std::env::set_var("PATH", dir.as_os_str());
        let found = find_executable("witty-fake-tool");
        std::env::set_var("PATH", original_path);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(found, Some(tool));
    }
}
