use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use serde_json::json;
use witty_core::{BasicTerminal, GridSize, RenderSnapshot, TerminalHostAction};
use witty_render_wgpu::{CellMetrics, FramePlanner, FrameStats, RectBatchItem};
use witty_transport::{LocalPtyConfig, LocalPtyTransport, TerminalTransport, TransportEvent};

const RUN_ENV: &str = "WITTY_RUN_FISH_PROMPT_PROBE";
const OUT_ENV: &str = "WITTY_FISH_PROMPT_PROBE_OUT";
const CASE_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_EVENTS_PER_TICK: usize = 256;
const SIZE: GridSize = GridSize {
    rows: 30,
    cols: 100,
};

#[test]
fn fish_prompt_probe() -> Result<()> {
    if env::var_os(RUN_ENV).is_none() {
        eprintln!("skipping fish prompt probe; set {RUN_ENV}=1 to run");
        return Ok(());
    }

    let Some(fish_path) = find_executable("fish") else {
        write_probe_report(json!({
            "status": "skipped",
            "skip_reason": "fish not found",
        }))?;
        return Ok(());
    };

    let output_path = env::var_os(OUT_ENV).map(PathBuf::from);
    let support_dir = output_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env::temp_dir().join("witty-fish-prompt-probe"));
    fs::create_dir_all(&support_dir)
        .with_context(|| format!("create probe support dir {}", support_dir.display()))?;

    let cases = [
        FishCase {
            id: "configured_private",
            args: &["--private"],
            isolated_home: false,
        },
        FishCase {
            id: "isolated_no_config",
            args: &["--private", "--no-config"],
            isolated_home: true,
        },
    ];

    let mut reports = Vec::new();
    for case in cases {
        reports.push(run_fish_case(&fish_path, &support_dir, case)?);
    }
    reports.push(run_tmux_fish_case(&fish_path, &support_dir)?);

    let isolated_prompt_visible = reports
        .iter()
        .find(|report| report["case_id"] == "isolated_no_config")
        .and_then(|report| report["prompt_marker_visible"].as_bool())
        .unwrap_or(false);

    write_probe_report(json!({
        "status": if isolated_prompt_visible { "passed" } else { "failed" },
        "fish_path": fish_path,
        "fish_version": command_version_first_line(&fish_path),
        "size": {
            "rows": SIZE.rows,
            "cols": SIZE.cols,
        },
        "cases": reports,
    }))?;

    assert!(
        isolated_prompt_visible,
        "isolated fish prompt should become visible through Witty PTY replies"
    );

    Ok(())
}

#[derive(Clone, Copy)]
struct FishCase {
    id: &'static str,
    args: &'static [&'static str],
    isolated_home: bool,
}

fn run_fish_case(
    fish_path: &Path,
    support_dir: &Path,
    case: FishCase,
) -> Result<serde_json::Value> {
    let started_at = Instant::now();
    let case_dir = support_dir.join(case.id);
    fs::create_dir_all(&case_dir)
        .with_context(|| format!("create fish probe case dir {}", case_dir.display()))?;

    let mut config = LocalPtyConfig::command(SIZE, fish_path.to_string_lossy().into_owned());
    config
        .args(case.args.iter().copied().map(str::to_owned))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .cwd(&case_dir);
    if case.isolated_home {
        let home = case_dir.join("home");
        let xdg_config = case_dir.join("xdg-config");
        fs::create_dir_all(&home)
            .with_context(|| format!("create fish probe HOME {}", home.display()))?;
        fs::create_dir_all(&xdg_config).with_context(|| {
            format!("create fish probe XDG_CONFIG_HOME {}", xdg_config.display())
        })?;
        config
            .env("HOME", home.to_string_lossy().into_owned())
            .env("XDG_CONFIG_HOME", xdg_config.to_string_lossy().into_owned());
    }

    let mut runtime = ProbeRuntime::spawn(config)?;
    let deadline = Instant::now() + CASE_TIMEOUT;
    let prompt_visible = runtime.wait_until(deadline, |terminal| {
        let text = screen_text(&terminal.snapshot());
        if case.isolated_home {
            text.lines().any(|line| line.trim_end().ends_with('>'))
        } else {
            text.lines().any(|line| !line.trim().is_empty())
        }
    })?;

    let snapshot = runtime.terminal.snapshot();
    let frame = FramePlanner::new(CellMetrics::default()).plan(&snapshot);
    let frame_stats = frame.stats;
    let text_decoration_rects = rects_json(&frame.text_decorations);
    let hyperlink_underline_rects = rects_json(&frame.hyperlink_underlines);
    let cursor_rect = frame.cursor.as_ref().map(rect_json);
    let before_exit = screen_text_sample(&snapshot);

    runtime.write_input(b"exit\n")?;
    let exited = runtime.wait_for_exit(deadline)?;

    Ok(json!({
        "case_id": case.id,
        "args": case.args,
        "prompt_marker_visible": prompt_visible,
        "process_exited": exited,
        "exit_status": runtime.exit_code,
        "elapsed_ms": started_at.elapsed().as_millis(),
        "output_bytes": runtime.output_bytes,
        "output_chunks": runtime.output_chunks,
        "host_reply_bytes": runtime.host_reply_bytes,
        "terminal_reply_actions": runtime.terminal_reply_actions,
        "clipboard_write_actions": runtime.clipboard_write_actions,
        "shell_integration_actions": runtime.shell_integration_actions,
        "current_directory_actions": runtime.current_directory_actions,
        "frame_stats": frame_stats_json(frame_stats),
        "text_decoration_rects": text_decoration_rects,
        "hyperlink_underline_rects": hyperlink_underline_rects,
        "cursor_rect": cursor_rect,
        "screen_text_sample_before_exit": before_exit,
    }))
}

fn run_tmux_fish_case(fish_path: &Path, support_dir: &Path) -> Result<serde_json::Value> {
    let Some(tmux_path) = find_executable("tmux") else {
        return Ok(json!({
            "case_id": "tmux_isolated_fish_no_config",
            "status": "skipped",
            "skip_reason": "tmux not found",
        }));
    };

    let started_at = Instant::now();
    let case_dir = support_dir.join("tmux_isolated_fish_no_config");
    let home = case_dir.join("home");
    let xdg_config = case_dir.join("xdg-config");
    fs::create_dir_all(&home)
        .with_context(|| format!("create tmux fish probe HOME {}", home.display()))?;
    fs::create_dir_all(&xdg_config).with_context(|| {
        format!(
            "create tmux fish probe XDG_CONFIG_HOME {}",
            xdg_config.display()
        )
    })?;

    let fish_command = format!("{} --private --no-config", fish_path.display());
    let mut config = LocalPtyConfig::command(SIZE, tmux_path.to_string_lossy().into_owned());
    config
        .args([
            "-S".to_owned(),
            "tmux.sock".to_owned(),
            "-f".to_owned(),
            "/dev/null".to_owned(),
            "-u".to_owned(),
            "new-session".to_owned(),
            fish_command,
        ])
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", home.to_string_lossy().into_owned())
        .env("XDG_CONFIG_HOME", xdg_config.to_string_lossy().into_owned())
        .cwd(&case_dir);

    let mut runtime = ProbeRuntime::spawn(config)?;
    let deadline = Instant::now() + CASE_TIMEOUT;
    let prompt_visible = runtime.wait_until(deadline, |terminal| {
        let text = screen_text(&terminal.snapshot());
        text.lines().any(|line| line.trim_end().ends_with('>'))
    })?;

    let snapshot = runtime.terminal.snapshot();
    let frame = FramePlanner::new(CellMetrics::default()).plan(&snapshot);
    let frame_stats = frame.stats;
    let text_decoration_rects = rects_json(&frame.text_decorations);
    let hyperlink_underline_rects = rects_json(&frame.hyperlink_underlines);
    let cursor_rect = frame.cursor.as_ref().map(rect_json);
    let before_exit = screen_text_sample(&snapshot);

    runtime.write_input(b"exit\n")?;
    let exited = runtime.wait_for_exit(deadline)?;
    let _ = std::process::Command::new(&tmux_path)
        .current_dir(&case_dir)
        .args(["-S", "tmux.sock", "kill-server"])
        .output();

    Ok(json!({
        "case_id": "tmux_isolated_fish_no_config",
        "status": "passed",
        "tmux_path": tmux_path,
        "tmux_version": command_version_first_line_with_args(&tmux_path, ["-V"]),
        "prompt_marker_visible": prompt_visible,
        "process_exited": exited,
        "exit_status": runtime.exit_code,
        "elapsed_ms": started_at.elapsed().as_millis(),
        "output_bytes": runtime.output_bytes,
        "output_chunks": runtime.output_chunks,
        "host_reply_bytes": runtime.host_reply_bytes,
        "terminal_reply_actions": runtime.terminal_reply_actions,
        "clipboard_write_actions": runtime.clipboard_write_actions,
        "shell_integration_actions": runtime.shell_integration_actions,
        "current_directory_actions": runtime.current_directory_actions,
        "frame_stats": frame_stats_json(frame_stats),
        "text_decoration_rects": text_decoration_rects,
        "hyperlink_underline_rects": hyperlink_underline_rects,
        "cursor_rect": cursor_rect,
        "screen_text_sample_before_exit": before_exit,
    }))
}

struct ProbeRuntime {
    transport: LocalPtyTransport,
    terminal: BasicTerminal,
    output_bytes: usize,
    output_chunks: usize,
    host_reply_bytes: usize,
    terminal_reply_actions: usize,
    clipboard_write_actions: usize,
    shell_integration_actions: usize,
    current_directory_actions: usize,
    exit_code: Option<i32>,
}

impl ProbeRuntime {
    fn spawn(config: LocalPtyConfig) -> Result<Self> {
        Ok(Self {
            transport: LocalPtyTransport::spawn(config)?,
            terminal: BasicTerminal::new(SIZE),
            output_bytes: 0,
            output_chunks: 0,
            host_reply_bytes: 0,
            terminal_reply_actions: 0,
            clipboard_write_actions: 0,
            shell_integration_actions: 0,
            current_directory_actions: 0,
            exit_code: None,
        })
    }

    fn write_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.transport.write(bytes)
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

    fn poll_available(&mut self) -> Result<()> {
        for _ in 0..MAX_EVENTS_PER_TICK {
            let Some(event) = self.transport.poll_event()? else {
                break;
            };
            match event {
                TransportEvent::Output(bytes) => {
                    self.output_bytes += bytes.len();
                    self.output_chunks += 1;
                    self.terminal.feed(&bytes);
                    self.apply_host_actions()?;
                }
                TransportEvent::Exit { code } => {
                    self.exit_code = code;
                }
                TransportEvent::Error(err) => anyhow::bail!("fish probe PTY error: {err}"),
            }
        }
        Ok(())
    }

    fn apply_host_actions(&mut self) -> Result<()> {
        for action in self.terminal.drain_host_actions() {
            match action {
                TerminalHostAction::TerminalReply(reply) => {
                    self.host_reply_bytes += reply.bytes.len();
                    self.terminal_reply_actions += 1;
                    self.transport.write(&reply.bytes)?;
                }
                TerminalHostAction::ClipboardWrite(_) => {
                    self.clipboard_write_actions += 1;
                }
                TerminalHostAction::ShellIntegration(_) => {
                    self.shell_integration_actions += 1;
                }
                TerminalHostAction::CurrentDirectory(_) => {
                    self.current_directory_actions += 1;
                }
                TerminalHostAction::Bell => {}
            }
        }
        Ok(())
    }
}

fn write_probe_report(value: serde_json::Value) -> Result<()> {
    let Some(path) = env::var_os(OUT_ENV).map(PathBuf::from) else {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create fish probe output dir {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&value)?)
        .with_context(|| format!("write fish probe report {}", path.display()))
}

fn frame_stats_json(stats: FrameStats) -> serde_json::Value {
    json!({
        "visibleRows": stats.visible_rows,
        "visibleCols": stats.visible_cols,
        "backgroundRuns": stats.background_runs,
        "glyphRuns": stats.glyph_runs,
        "glyphChars": stats.glyph_chars,
        "glyphPrepareBatches": stats.glyph_prepare_batches,
        "maxGlyphRunChars": stats.max_glyph_run_chars,
        "selectionRects": stats.selection_rects,
        "searchHighlightRects": stats.search_highlight_rects,
        "hyperlinkHoverRects": stats.hyperlink_hover_rects,
        "hyperlinkUnderlineRects": stats.hyperlink_underline_rects,
        "textDecorationRects": stats.text_decoration_rects,
        "imePreeditRects": stats.ime_preedit_rects,
        "searchActiveVisible": stats.search_active_visible,
        "cursorVisible": stats.cursor_visible,
        "rectVertices": stats.rect_vertices,
        "rectVertexCapacity": stats.rect_vertex_capacity,
        "fullDamage": stats.full_damage,
        "damageRegions": stats.damage_regions,
        "reusedRows": stats.reused_rows,
        "rebuiltRows": stats.rebuilt_rows,
    })
}

fn rects_json(rects: &[RectBatchItem]) -> serde_json::Value {
    serde_json::Value::Array(rects.iter().map(rect_json).collect())
}

fn rect_json(rect: &RectBatchItem) -> serde_json::Value {
    json!({
        "origin": {
            "x": rect.origin.x,
            "y": rect.origin.y,
        },
        "size": {
            "width": rect.size.width,
            "height": rect.size.height,
        },
        "color": {
            "r": rect.color.r,
            "g": rect.color.g,
            "b": rect.color.b,
            "a": rect.color.a,
        },
    })
}

fn screen_text_sample(snapshot: &RenderSnapshot) -> String {
    screen_text(snapshot).chars().take(2000).collect()
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

fn find_executable(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
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
