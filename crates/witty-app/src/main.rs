#![recursion_limit = "256"]

mod logging;
mod real_tui_smoke;
mod update_state;
mod window;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write as _},
};

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;
use update_state::read_restart_snapshot;
use window::{
    CursorBlinkRate, CursorStyleSource, MouseSelectionOverridePolicy,
    NativeSessionTabDisplayPolicy, NativeSessionTabLabelStyle, NativeSessionTabPosition,
    WindowLastActiveClosePolicy, WindowSmokeOptions,
    DEFAULT_WINDOW_TITLE,
};
use witty_core::{
    encode_terminal_key_input, parse_terminal_color, BasicTerminal, CellPoint, CellRange,
    CursorShape, GridSize, Osc52ClipboardPolicy, Rgba, TerminalColorTheme, TerminalHostAction,
    TerminalInputModes, TerminalKey, TerminalKeyEventType, TerminalKeyInput,
    TerminalKeyModifiers, TerminalKeypadKey, TerminalModifierKey, TerminalNamedKey,
    DEFAULT_MAX_SCROLLBACK_LINES, KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
    KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES, KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS,
    KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT, KITTY_KEYBOARD_REPORT_EVENT_TYPES,
};
use witty_plugin_api::{
    CommandRegistration, PluginAction, PluginEvent, PluginManifest, PluginPermissions,
    PluginRuntime,
};
use witty_plugin_wasm::WasmPluginRuntime;
use witty_render_wgpu::{
    available_font_families, native_wgpu_backend_policy, CellMetrics, FramePlanner, FrameStats,
    RendererBackgroundImageFit, RendererFontConfig, RendererVisualConfig, RetainedFramePlanner,
};
use witty_transport::{
    apply_openssh_import_preview, default_profile_store_path, edit_profile_store,
    parse_openssh_import_preview, read_profile_store, run_openssh_config_dump_smoke,
    LocalPtyConfig, LocalPtyTransport, MockTransport, OpenSshImportApplyReport,
    OpenSshImportConflict, OpenSshImportConflictPolicy, OpenSshImportPreview,
    OpenSshImportSelection, ProfileStoreDefaultPolicy, ProfileStoreEditOpenMode,
    ProfileStoreEditReport, ProfileStoreV1, SshProfile, SshProfileLaunchability, TerminalTransport,
    TransportEvent,
};
use witty_ui::{BuiltInPlugin, TerminalApp};

const WITTY_FONT_FAMILY_ENV: &str = "WITTY_FONT_FAMILY";
const WITTY_FONT_PATHS_ENV: &str = "WITTY_FONT_PATHS";
const WITTY_WINDOW_CONFIG_ENV: &str = "WITTY_WINDOW_CONFIG";
const NATIVE_WINDOW_CONFIG_FILE_NAME: &str = "window.v1.json";
const NATIVE_WINDOW_CONFIG_MAX_JSON_BYTES: u64 = 64 * 1024;
const WITTYRC_CONFIG_FILE_NAME: &str = ".wittyrc";
const WITTYRC_TEMPLATE: &str = include_str!("../templates/wittyrc");
const WITTYRC_CONFIG_MAX_TOML_BYTES: u64 = 64 * 1024;
const MIN_TERMINAL_FONT_SIZE: u16 = 6;
const MAX_TERMINAL_FONT_SIZE: u16 = 96;
const MIN_TERMINAL_PADDING: u16 = 0;
const MAX_TERMINAL_PADDING: u16 = 64;
const MIN_BACKGROUND_OPACITY: f32 = 0.0;
const MAX_BACKGROUND_OPACITY: f32 = 1.0;
const DEFAULT_WINDOW_ROWS: u16 = 24;
const DEFAULT_WINDOW_COLS: u16 = 80;
const MIN_WINDOW_ROWS: u16 = 5;
const MAX_WINDOW_ROWS: u16 = 200;
const MIN_WINDOW_COLS: u16 = 20;
const MAX_WINDOW_COLS: u16 = 400;
const RECOMMENDED_TERMINAL_FONT_FAMILY: &str = "Maple Mono NF CN";

fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run_main(args.clone()) {
        Ok(()) => Ok(()),
        Err(err) => {
            if startup_dialog_requested(&args) {
                window::show_startup_error_dialog("Witty could not start", &format!("{err:#}"));
            }
            Err(err)
        }
    }
}

fn run_main(args: Vec<String>) -> anyhow::Result<()> {
    let mut options = AppOptions::parse(args)?;
    let _logging_guard = if options.mode == AppMode::Window {
        match logging::init_window_logging() {
            Ok(guard) => Some(guard),
            Err(err) => {
                eprintln!("failed to initialize Witty logging: {err:#}");
                None
            }
        }
    } else {
        None
    };
    let mut startup_notices = Vec::new();
    let wittyrc_config_load =
        match options.apply_wittyrc_defaults(default_wittyrc_path, read_wittyrc_config) {
            Ok(load) => load,
            Err(err) if options.mode == AppMode::Window => {
                let config_ref = options.wittyrc_config_ref(default_wittyrc_path).ok();
                let notice = wittyrc_startup_error_notice(config_ref.as_ref(), &err);
                tracing::error!(
                    target: "witty_app::config",
                    error = %err,
                    "ignored invalid wittyrc and continued with defaults"
                );
                eprintln!("{notice}");
                startup_notices.push(notice);
                WittyrcConfigLoadReport {
                    path: config_ref.as_ref().map(|config_ref| config_ref.path.clone()),
                    required: config_ref.as_ref().is_some_and(|config_ref| config_ref.required),
                    status: WittyrcConfigLoadStatus::Failed,
                }
            }
            Err(err) => return Err(err),
        };
    let native_window_config_load = options.apply_native_window_config_defaults(
        |name| env::var_os(name),
        default_native_window_config_path,
        read_native_window_config,
    )?;
    if !mode_supports_wasm_plugin_startup(&options.mode) && !options.wasm_plugins.is_empty() {
        bail!("Wasm plugin startup loading is not supported for this mode");
    }
    if options.mode == AppMode::ProfileStore {
        let profile_store = options
            .profile_store
            .as_ref()
            .expect("profile-store mode should carry command options");
        let output = run_profile_store_cli(profile_store)?;
        print!("{output}");
        return Ok(());
    }
    if options.mode == AppMode::WittyrcTemplate {
        print!("{}", wittyrc_template());
        return Ok(());
    }
    if options.mode == AppMode::WittyrcDefaultPath {
        print!("{}", wittyrc_default_path_line(default_wittyrc_path)?);
        return Ok(());
    }
    if options.mode == AppMode::WittyrcInit {
        print!(
            "{}",
            init_wittyrc_for_options(&options, default_wittyrc_path)?
        );
        return Ok(());
    }
    if options.mode == AppMode::WittyrcCheck {
        print!(
            "{}",
            check_wittyrc_config(&options, default_wittyrc_path, read_wittyrc_config)?
        );
        return Ok(());
    }
    if options.mode == AppMode::WittyrcEffective {
        print!(
            "{}",
            wittyrc_effective_config_summary(
                &options,
                &wittyrc_config_load,
                &native_window_config_load
            )?
        );
        return Ok(());
    }
    if options.mode == AppMode::WindowConfigTemplate {
        print!("{}", native_window_config_template());
        return Ok(());
    }
    if options.mode == AppMode::WindowConfigDefaultPath {
        print!(
            "{}",
            native_window_config_default_path_line(default_native_window_config_path)?
        );
        return Ok(());
    }
    if options.mode == AppMode::WindowConfigInit {
        print!(
            "{}",
            init_native_window_config_for_options(&options, default_native_window_config_path)?
        );
        return Ok(());
    }
    if options.mode == AppMode::WindowConfigCheck {
        print!(
            "{}",
            check_native_window_config(
                &options,
                |name| env::var_os(name),
                default_native_window_config_path,
                read_native_window_config,
            )?
        );
        return Ok(());
    }
    if options.mode == AppMode::WindowConfigEffective {
        print!(
            "{}",
            native_window_effective_config_summary(
                &options,
                &native_window_config_load,
                &wittyrc_config_load,
            )?
        );
        return Ok(());
    }
    if options.mode == AppMode::Web {
        return witty_launcher::run_cli(options.launcher_args);
    }
    if options.mode == AppMode::Window {
        tracing::info!(
            target: "witty_app::window",
            version = env!("CARGO_PKG_VERSION"),
            debug_assertions = cfg!(debug_assertions),
            "starting Witty native window"
        );
        let restore_state = match options.restore_state_path.as_ref() {
            Some(path) => Some(
                read_restart_snapshot(path)
                    .with_context(|| format!("read restart state {}", path.display()))?
                    .with_context(|| format!("restart state does not exist: {}", path.display()))?,
            ),
            None => None,
        };
        let result = window::run(
            options.wasm_plugins,
            options.window_smoke,
            startup_notices,
            options.window_title,
            options.program,
            options.args,
            options.cwd,
            options.launch_env,
            options.mouse_selection_override,
            options.osc52_clipboard_policy,
            options.max_scrollback_lines,
            options.font_family,
            options.font_size,
            options.terminal_padding,
            options.background_opacity,
            options.background_image.clone(),
            options.background_image_fit,
            options.background_overlay_color,
            options.background_overlay_opacity,
            options.terminal_color_theme,
            options.cursor_shape,
            options.cursor_blink,
            options.cursor_blink_rate,
            options.cursor_style_source,
            options.session_tab_position,
            options.session_tab_label_style,
            options.session_tab_display_policy,
            options.font_paths,
            restore_state,
        );
        match &result {
            Ok(()) => tracing::info!(target: "witty_app::window", "Witty native window exited"),
            Err(err) => {
                let error_chain = format!("{err:#}");
                tracing::error!(
                    target: "witty_app::window",
                    error = %err,
                    error_debug = ?err,
                    error_chain = %error_chain,
                    "Witty native window exited with error"
                );
            }
        }
        return result;
    }
    if options.mode == AppMode::PtySmoke {
        return run_pty_smoke();
    }
    if options.mode == AppMode::IncrementalSmoke {
        return run_incremental_smoke();
    }
    if options.mode == AppMode::SelectionCopySmoke {
        return run_selection_copy_smoke();
    }
    if options.mode == AppMode::PrimarySelectionSmoke {
        return run_primary_selection_smoke();
    }
    if options.mode == AppMode::PrimarySelectionGuiSmoke {
        return run_primary_selection_gui_smoke();
    }
    if options.mode == AppMode::NativeSearchSmoke {
        return run_native_search_smoke();
    }
    if options.mode == AppMode::NativeCommandBlockSmoke {
        return run_native_command_block_smoke();
    }
    if options.mode == AppMode::OpenSshProfileSmoke {
        return run_openssh_profile_smoke();
    }
    if options.mode == AppMode::RealTuiSmoke {
        let case_id = options
            .real_tui_smoke_case
            .as_deref()
            .expect("real TUI smoke mode should carry a case id");
        return real_tui_smoke::run_real_tui_smoke_cli(case_id);
    }
    if options.mode == AppMode::RendererBackendInfo {
        return run_renderer_backend_info();
    }
    if options.mode == AppMode::RendererNoSurfaceDiagnostics {
        return run_renderer_no_surface_diagnostics();
    }
    if options.mode == AppMode::KeyboardProtocolDiagnostics {
        return run_keyboard_protocol_diagnostics();
    }
    if options.mode == AppMode::KeyboardProtocolCapture {
        return run_keyboard_protocol_capture();
    }
    if options.mode == AppMode::FontList {
        return run_font_list(options.font_list_filter.as_deref());
    }

    let transport = MockTransport::new(GridSize::new(24, 80));
    let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

    app.install_builtin_plugin(BuiltInCommandsPlugin)?;
    install_wasm_plugins(&mut app, &options.wasm_plugins)?;

    let mut terminal = BasicTerminal::new(GridSize::new(24, 80));
    terminal.feed(b"Witty Rust/wgpu prototype\r\nM2 vte parser spike is running.");
    app.set_snapshot(terminal.take_snapshot());

    let frame = app.frame_plan();
    println!(
        "Plugins loaded; {} commands; {} glyphs planned",
        app.commands().len(),
        frame.glyphs.len()
    );

    Ok(())
}

fn startup_dialog_requested(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--window") || env::var_os("WITTY_STARTUP_DIALOG").is_some()
}

fn wittyrc_startup_error_notice(
    config_ref: Option<&WittyrcConfigRef>,
    err: &anyhow::Error,
) -> String {
    let path = config_ref
        .map(|config_ref| config_ref.path.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_owned());
    format!(
        "Ignored .wittyrc because it could not be loaded.\r\nPath: {path}\r\nError: {err:#}\r\nWitty continued with built-in defaults and command-line settings.\r\nFix the file, then run: witty --wittyrc-check\r\nAfter the check passes, restart Witty to reload the config."
    )
}

#[derive(Clone, Debug, PartialEq)]
struct AppOptions {
    mode: AppMode,
    wasm_plugins: Vec<PathBuf>,
    window_smoke: WindowSmokeOptions,
    window_title: Option<String>,
    program: Option<String>,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    launch_env: Vec<(String, String)>,
    mouse_selection_override: MouseSelectionOverridePolicy,
    osc52_clipboard_policy: Osc52ClipboardPolicy,
    max_scrollback_lines: usize,
    font_family: Option<String>,
    font_size: Option<u16>,
    terminal_padding: Option<u16>,
    background_opacity: Option<f32>,
    background_image: Option<PathBuf>,
    background_image_fit: Option<RendererBackgroundImageFit>,
    background_overlay_color: Option<Rgba>,
    background_overlay_opacity: Option<f32>,
    terminal_color_theme: TerminalColorTheme,
    cursor_shape: CursorShape,
    cursor_blink: bool,
    cursor_blink_rate: CursorBlinkRate,
    cursor_style_source: CursorStyleSource,
    session_tab_position: NativeSessionTabPosition,
    session_tab_label_style: NativeSessionTabLabelStyle,
    session_tab_display_policy: NativeSessionTabDisplayPolicy,
    font_paths: Vec<PathBuf>,
    restore_state_path: Option<PathBuf>,
    wittyrc_path: Option<PathBuf>,
    no_wittyrc: bool,
    window_config_path: Option<PathBuf>,
    no_window_config: bool,
    explicit: AppOptionsExplicit,
    launcher_args: Vec<String>,
    real_tui_smoke_case: Option<String>,
    profile_store: Option<ProfileStoreCliOptions>,
    font_list_filter: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AppOptionsExplicit {
    window_last_active_close_policy: bool,
    window_title: bool,
    program: bool,
    args: bool,
    launch_env: bool,
    mouse_selection_override: bool,
    osc52_clipboard_policy: bool,
    max_scrollback_lines: bool,
    font_family: bool,
    font_size: bool,
    terminal_padding: bool,
    background_opacity: bool,
    background_image: bool,
    background_image_fit: bool,
    background_overlay_color: bool,
    background_overlay_opacity: bool,
    cursor_shape: bool,
    cursor_blink: bool,
    cursor_blink_rate: bool,
    cursor_style_source: bool,
    session_tab_position: bool,
    session_tab_label_style: bool,
    session_tab_show_single: bool,
    session_tab_show_multiple: bool,
    font_paths: bool,
    cwd: bool,
    window_cols: bool,
    window_rows: bool,
}

impl AppOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        Self::parse_with_env(args, |name| env::var_os(name))
    }

    fn parse_with_env(
        args: impl IntoIterator<Item = String>,
        env_var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self> {
        let mut mode = AppMode::Smoke;
        let mut wasm_plugins = Vec::new();
        let mut window_smoke = WindowSmokeOptions::default();
        let mut window_title = None;
        let mut program = None;
        let mut program_args = Vec::new();
        let mut cwd = None;
        let mut launch_env = Vec::new();
        let mut mouse_selection_override = MouseSelectionOverridePolicy::default();
        let mut mouse_selection_override_explicit = false;
        let mut osc52_clipboard_policy = Osc52ClipboardPolicy::default();
        let mut osc52_clipboard_policy_explicit = false;
        let mut max_scrollback_lines = DEFAULT_MAX_SCROLLBACK_LINES;
        let mut max_scrollback_lines_explicit = false;
        let mut font_family = None;
        let mut font_size = None;
        let mut terminal_padding = None;
        let mut background_opacity = None;
        let mut background_image = None;
        let mut background_image_fit = None;
        let mut background_overlay_color = None;
        let mut background_overlay_opacity = None;
        let terminal_color_theme = TerminalColorTheme::default();
        let mut cursor_shape = CursorShape::Block;
        let mut cursor_blink = true;
        let mut cursor_blink_rate = CursorBlinkRate::default();
        let mut cursor_style_source = CursorStyleSource::default();
        let mut session_tab_position = NativeSessionTabPosition::default();
        let mut session_tab_label_style = NativeSessionTabLabelStyle::default();
        let mut session_tab_display_policy = NativeSessionTabDisplayPolicy::default();
        let mut font_paths = Vec::new();
        let mut restore_state_path = None;
        let mut wittyrc_path = None;
        let mut no_wittyrc = false;
        let mut window_config_path = None;
        let mut no_window_config = false;
        let mut explicit = AppOptionsExplicit::default();
        let mut launcher_args = Vec::new();
        let mut launcher_only_args_seen = false;
        let mut launch_command_args_seen = false;
        let mut wittyrc_args_seen = false;
        let mut window_config_args_seen = false;
        let mut real_tui_smoke_case = None;
        let mut profile_store_command = None;
        let mut font_list_filter = None;
        let mut font_list_args_seen = false;
        let mut profile_store_path = None;
        let mut ssh_profile_json = None;
        let mut ssh_profile_id = None;
        let mut set_default_profile = false;
        let mut confirm_profile_store_import = false;
        let mut openssh_import_conflict_policy = None;
        let mut openssh_import_profile_ids = Vec::new();
        let mut non_profile_mode_seen = false;
        let mut window_only_args_seen = false;
        let mut restore_state_args_seen = false;
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--web" => {
                    mode = AppMode::Web;
                    non_profile_mode_seen = true;
                }
                "--window" => {
                    mode = AppMode::Window;
                    non_profile_mode_seen = true;
                }
                "--pty-smoke" => {
                    mode = AppMode::PtySmoke;
                    non_profile_mode_seen = true;
                }
                "--incremental-smoke" => {
                    mode = AppMode::IncrementalSmoke;
                    non_profile_mode_seen = true;
                }
                "--selection-copy-smoke" => {
                    mode = AppMode::SelectionCopySmoke;
                    non_profile_mode_seen = true;
                }
                "--primary-selection-smoke" => {
                    mode = AppMode::PrimarySelectionSmoke;
                    non_profile_mode_seen = true;
                }
                "--primary-selection-gui-smoke" => {
                    mode = AppMode::PrimarySelectionGuiSmoke;
                    non_profile_mode_seen = true;
                }
                "--native-search-smoke" => {
                    mode = AppMode::NativeSearchSmoke;
                    non_profile_mode_seen = true;
                }
                "--native-command-block-smoke" => {
                    mode = AppMode::NativeCommandBlockSmoke;
                    non_profile_mode_seen = true;
                }
                "--openssh-profile-smoke" => {
                    mode = AppMode::OpenSshProfileSmoke;
                    non_profile_mode_seen = true;
                }
                "--real-tui-smoke" => {
                    mode = AppMode::RealTuiSmoke;
                    non_profile_mode_seen = true;
                    real_tui_smoke_case =
                        Some(args.next().context(
                            "--real-tui-smoke requires a case id like less-basic-restore",
                        )?);
                }
                "--renderer-backend-info" => {
                    mode = AppMode::RendererBackendInfo;
                    non_profile_mode_seen = true;
                }
                "--renderer-no-surface-diagnostics" => {
                    mode = AppMode::RendererNoSurfaceDiagnostics;
                    non_profile_mode_seen = true;
                }
                "--keyboard-protocol-diagnostics" => {
                    mode = AppMode::KeyboardProtocolDiagnostics;
                    non_profile_mode_seen = true;
                }
                "--keyboard-protocol-capture" => {
                    mode = AppMode::KeyboardProtocolCapture;
                    non_profile_mode_seen = true;
                }
                "--font-list" => {
                    mode = AppMode::FontList;
                    non_profile_mode_seen = true;
                    font_list_args_seen = true;
                }
                "--font-list-filter" => {
                    let Some(value) = args.next() else {
                        bail!("--font-list-filter requires a value");
                    };
                    if font_list_filter.is_some() {
                        bail!("only one --font-list-filter value is allowed");
                    }
                    font_list_filter = Some(parse_font_list_filter(&value)?);
                    font_list_args_seen = true;
                }
                "--wittyrc-template" => {
                    mode = AppMode::WittyrcTemplate;
                    non_profile_mode_seen = true;
                }
                "--wittyrc-default-path" => {
                    mode = AppMode::WittyrcDefaultPath;
                    non_profile_mode_seen = true;
                }
                "--wittyrc-init" => {
                    mode = AppMode::WittyrcInit;
                    non_profile_mode_seen = true;
                }
                "--wittyrc-check" => {
                    mode = AppMode::WittyrcCheck;
                    non_profile_mode_seen = true;
                }
                "--wittyrc-effective" => {
                    mode = AppMode::WittyrcEffective;
                    non_profile_mode_seen = true;
                }
                "--window-config-template" => {
                    mode = AppMode::WindowConfigTemplate;
                    non_profile_mode_seen = true;
                }
                "--window-config-default-path" => {
                    mode = AppMode::WindowConfigDefaultPath;
                    non_profile_mode_seen = true;
                }
                "--window-config-init" => {
                    mode = AppMode::WindowConfigInit;
                    non_profile_mode_seen = true;
                }
                "--window-config-check" => {
                    mode = AppMode::WindowConfigCheck;
                    non_profile_mode_seen = true;
                }
                "--window-config-effective" => {
                    mode = AppMode::WindowConfigEffective;
                    non_profile_mode_seen = true;
                }
                "--window-command-palette" => {
                    window_smoke.open_command_palette = true;
                    window_only_args_seen = true;
                }
                "--window-diagnostics" => {
                    window_smoke.show_diagnostics = true;
                    window_only_args_seen = true;
                }
                "--window-startup-report" => {
                    window_smoke.report_startup = true;
                    window_only_args_seen = true;
                }
                "--window-exit-after-ms" => {
                    let Some(milliseconds) = args.next() else {
                        bail!("--window-exit-after-ms requires a value");
                    };
                    let milliseconds = milliseconds
                        .parse::<u64>()
                        .context("--window-exit-after-ms must be an integer")?;
                    window_smoke.exit_after = Some(Duration::from_millis(milliseconds));
                    window_only_args_seen = true;
                }
                "--window-last-active-close" => {
                    let Some(value) = args.next() else {
                        bail!("--window-last-active-close requires a value");
                    };
                    window_smoke.last_active_close_policy =
                        parse_window_last_active_close_policy(&value)?;
                    explicit.window_last_active_close_policy = true;
                    window_only_args_seen = true;
                }
                "--window-title" => {
                    let Some(value) = args.next() else {
                        bail!("--window-title requires a value");
                    };
                    if window_title.is_some() {
                        bail!("only one --window-title value is allowed");
                    }
                    window_title = Some(parse_window_title(&value, "--window-title")?);
                    explicit.window_title = true;
                    window_only_args_seen = true;
                }
                "--window-cols" => {
                    let Some(value) = args.next() else {
                        bail!("--window-cols requires a value");
                    };
                    if explicit.window_cols {
                        bail!("only one --window-cols value is allowed");
                    }
                    let cols = parse_window_cols(&value, "--window-cols")?;
                    let current_size = window_smoke
                        .initial_size
                        .unwrap_or(GridSize::new(DEFAULT_WINDOW_ROWS, DEFAULT_WINDOW_COLS));
                    window_smoke.initial_size = Some(GridSize::new(current_size.rows, cols));
                    explicit.window_cols = true;
                    window_only_args_seen = true;
                }
                "--window-rows" => {
                    let Some(value) = args.next() else {
                        bail!("--window-rows requires a value");
                    };
                    if explicit.window_rows {
                        bail!("only one --window-rows value is allowed");
                    }
                    let rows = parse_window_rows(&value, "--window-rows")?;
                    let current_size = window_smoke
                        .initial_size
                        .unwrap_or(GridSize::new(DEFAULT_WINDOW_ROWS, DEFAULT_WINDOW_COLS));
                    window_smoke.initial_size = Some(GridSize::new(rows, current_size.cols));
                    explicit.window_rows = true;
                    window_only_args_seen = true;
                }
                "--restore-state" => {
                    let Some(value) = args.next() else {
                        bail!("--restore-state requires a path");
                    };
                    if restore_state_path.is_some() {
                        bail!("only one --restore-state value is allowed");
                    }
                    restore_state_path = Some(parse_restore_state_path(&value)?);
                    restore_state_args_seen = true;
                    window_only_args_seen = true;
                }
                "--mouse-selection-override" => {
                    let Some(value) = args.next() else {
                        bail!("--mouse-selection-override requires a value");
                    };
                    mouse_selection_override =
                        MouseSelectionOverridePolicy::parse_config_value(&value)?;
                    mouse_selection_override_explicit = true;
                    explicit.mouse_selection_override = true;
                }
                "--osc52-clipboard" => {
                    let Some(value) = args.next() else {
                        bail!("--osc52-clipboard requires a value");
                    };
                    osc52_clipboard_policy = parse_osc52_clipboard_policy(&value)?;
                    osc52_clipboard_policy_explicit = true;
                    explicit.osc52_clipboard_policy = true;
                }
                "--scrollback-lines" => {
                    let Some(value) = args.next() else {
                        bail!("--scrollback-lines requires a value");
                    };
                    max_scrollback_lines = value
                        .parse::<usize>()
                        .context("--scrollback-lines must be an integer")?;
                    max_scrollback_lines_explicit = true;
                    explicit.max_scrollback_lines = true;
                }
                "--font-family" => {
                    let Some(value) = args.next() else {
                        bail!("--font-family requires a value");
                    };
                    if font_family.is_some() {
                        bail!("only one --font-family value is allowed");
                    }
                    let parsed = parse_font_family(&value)?;
                    font_family = Some(parsed);
                    explicit.font_family = true;
                    window_only_args_seen = true;
                }
                "--font-size" => {
                    let Some(value) = args.next() else {
                        bail!("--font-size requires a value");
                    };
                    if font_size.is_some() {
                        bail!("only one --font-size value is allowed");
                    }
                    font_size = Some(parse_font_size(&value, "--font-size")?);
                    explicit.font_size = true;
                    window_only_args_seen = true;
                }
                "--terminal-padding" => {
                    let Some(value) = args.next() else {
                        bail!("--terminal-padding requires a value");
                    };
                    if terminal_padding.is_some() {
                        bail!("only one --terminal-padding value is allowed");
                    }
                    terminal_padding = Some(parse_terminal_padding(&value, "--terminal-padding")?);
                    explicit.terminal_padding = true;
                    window_only_args_seen = true;
                }
                "--background-opacity" => {
                    let Some(value) = args.next() else {
                        bail!("--background-opacity requires a value");
                    };
                    if background_opacity.is_some() {
                        bail!("only one --background-opacity value is allowed");
                    }
                    background_opacity =
                        Some(parse_background_opacity(&value, "--background-opacity")?);
                    explicit.background_opacity = true;
                    window_only_args_seen = true;
                }
                "--background-image" => {
                    let Some(value) = args.next() else {
                        bail!("--background-image requires a value");
                    };
                    if explicit.background_image {
                        bail!("only one --background-image value is allowed");
                    }
                    background_image = parse_background_image_value(&value, "--background-image")?;
                    explicit.background_image = true;
                    window_only_args_seen = true;
                }
                "--background-image-fit" => {
                    let Some(value) = args.next() else {
                        bail!("--background-image-fit requires a value");
                    };
                    if background_image_fit.is_some() {
                        bail!("only one --background-image-fit value is allowed");
                    }
                    background_image_fit = Some(parse_background_image_fit(
                        &value,
                        "--background-image-fit",
                    )?);
                    explicit.background_image_fit = true;
                    window_only_args_seen = true;
                }
                "--background-overlay-color" => {
                    let Some(value) = args.next() else {
                        bail!("--background-overlay-color requires a value");
                    };
                    if background_overlay_color.is_some() {
                        bail!("only one --background-overlay-color value is allowed");
                    }
                    background_overlay_color =
                        Some(parse_background_overlay_color(&value, "--background-overlay-color")?);
                    explicit.background_overlay_color = true;
                    window_only_args_seen = true;
                }
                "--background-overlay-opacity" => {
                    let Some(value) = args.next() else {
                        bail!("--background-overlay-opacity requires a value");
                    };
                    if background_overlay_opacity.is_some() {
                        bail!("only one --background-overlay-opacity value is allowed");
                    }
                    background_overlay_opacity = Some(parse_background_overlay_opacity(
                        &value,
                        "--background-overlay-opacity",
                    )?);
                    explicit.background_overlay_opacity = true;
                    window_only_args_seen = true;
                }
                "--cursor-shape" => {
                    let Some(value) = args.next() else {
                        bail!("--cursor-shape requires a value");
                    };
                    if explicit.cursor_shape {
                        bail!("only one --cursor-shape value is allowed");
                    }
                    cursor_shape = parse_cursor_shape(&value, "--cursor-shape")?;
                    explicit.cursor_shape = true;
                    window_only_args_seen = true;
                }
                "--cursor-blink" => {
                    let Some(value) = args.next() else {
                        bail!("--cursor-blink requires a value");
                    };
                    if explicit.cursor_blink {
                        bail!("only one --cursor-blink value is allowed");
                    }
                    cursor_blink = parse_bool_config(&value, "--cursor-blink")?;
                    explicit.cursor_blink = true;
                    window_only_args_seen = true;
                }
                "--cursor-blink-rate" => {
                    let Some(value) = args.next() else {
                        bail!("--cursor-blink-rate requires a value");
                    };
                    if explicit.cursor_blink_rate {
                        bail!("only one --cursor-blink-rate value is allowed");
                    }
                    cursor_blink_rate = CursorBlinkRate::parse_config_value(&value)?;
                    explicit.cursor_blink_rate = true;
                    window_only_args_seen = true;
                }
                "--cursor-style-source" => {
                    let Some(value) = args.next() else {
                        bail!("--cursor-style-source requires a value");
                    };
                    if explicit.cursor_style_source {
                        bail!("only one --cursor-style-source value is allowed");
                    }
                    cursor_style_source = CursorStyleSource::parse_config_value(&value)?;
                    explicit.cursor_style_source = true;
                    window_only_args_seen = true;
                }
                "--session-tab-position" => {
                    let Some(value) = args.next() else {
                        bail!("--session-tab-position requires a value");
                    };
                    if explicit.session_tab_position {
                        bail!("only one --session-tab-position value is allowed");
                    }
                    session_tab_position =
                        parse_session_tab_position(&value, "--session-tab-position")?;
                    explicit.session_tab_position = true;
                    window_only_args_seen = true;
                }
                "--session-tab-label" => {
                    let Some(value) = args.next() else {
                        bail!("--session-tab-label requires a value");
                    };
                    if explicit.session_tab_label_style {
                        bail!("only one --session-tab-label value is allowed");
                    }
                    session_tab_label_style =
                        parse_session_tab_label_style(&value, "--session-tab-label")?;
                    explicit.session_tab_label_style = true;
                    window_only_args_seen = true;
                }
                "--session-tab-show-single" => {
                    let Some(value) = args.next() else {
                        bail!("--session-tab-show-single requires a value");
                    };
                    if explicit.session_tab_show_single {
                        bail!("only one --session-tab-show-single value is allowed");
                    }
                    session_tab_display_policy.show_single =
                        parse_bool_config(&value, "--session-tab-show-single")?;
                    explicit.session_tab_show_single = true;
                    window_only_args_seen = true;
                }
                "--session-tab-show-multiple" => {
                    let Some(value) = args.next() else {
                        bail!("--session-tab-show-multiple requires a value");
                    };
                    if explicit.session_tab_show_multiple {
                        bail!("only one --session-tab-show-multiple value is allowed");
                    }
                    session_tab_display_policy.show_multiple =
                        parse_bool_config(&value, "--session-tab-show-multiple")?;
                    explicit.session_tab_show_multiple = true;
                    window_only_args_seen = true;
                }
                "--font-path" => {
                    let Some(value) = args.next() else {
                        bail!("--font-path requires a value");
                    };
                    font_paths.push(parse_font_path(&value)?);
                    explicit.font_paths = true;
                    window_only_args_seen = true;
                }
                "--wittyrc" => {
                    let Some(value) = args.next() else {
                        bail!("--wittyrc requires a value");
                    };
                    if wittyrc_path.is_some() {
                        bail!("only one --wittyrc value is allowed");
                    }
                    wittyrc_path = Some(parse_wittyrc_path(&value)?);
                    wittyrc_args_seen = true;
                }
                "--no-wittyrc" => {
                    no_wittyrc = true;
                    wittyrc_args_seen = true;
                }
                "--window-config" => {
                    let Some(value) = args.next() else {
                        bail!("--window-config requires a value");
                    };
                    if window_config_path.is_some() {
                        bail!("only one --window-config value is allowed");
                    }
                    window_config_path = Some(parse_window_config_path(&value)?);
                    window_config_args_seen = true;
                }
                "--no-window-config" => {
                    no_window_config = true;
                    window_only_args_seen = true;
                }
                "--web-root" | "--ui-bind" | "--gateway-bind" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    launcher_args.push(arg);
                    launcher_args.push(value);
                    launcher_only_args_seen = true;
                }
                "--program" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    program = Some(parse_launch_program(&value, "--program")?);
                    launcher_args.push(arg);
                    launcher_args.push(value);
                    explicit.program = true;
                    launch_command_args_seen = true;
                }
                "--arg" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    program_args.push(value.clone());
                    launcher_args.push(arg);
                    launcher_args.push(value);
                    explicit.args = true;
                    launch_command_args_seen = true;
                }
                "--cwd" => {
                    let value = args.next().context("--cwd requires a path")?;
                    if cwd.is_some() {
                        bail!("only one --cwd value is allowed");
                    }
                    cwd = Some(parse_cwd_path(&value)?);
                    explicit.cwd = true;
                    window_only_args_seen = true;
                }
                "--env" => {
                    let value = args.next().context("--env requires KEY=VALUE")?;
                    let pair = parse_launch_env_pair(&value, "--env")?;
                    set_launch_env_pair(&mut launch_env, pair);
                    explicit.launch_env = true;
                    window_only_args_seen = true;
                }
                "--profile-picker" => {
                    launcher_args.push(arg);
                    launcher_only_args_seen = true;
                }
                "--profile-picker-import-openssh" | "--profile-import-openssh" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    launcher_args.push(arg);
                    launcher_args.push(value);
                    launcher_only_args_seen = true;
                }
                "--ssh-profile-json" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    if ssh_profile_json.is_some() {
                        bail!("only one --ssh-profile-json value is allowed");
                    }
                    ssh_profile_json = Some(PathBuf::from(&value));
                    launcher_args.push(arg);
                    launcher_args.push(value);
                }
                "--profile-store" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    if profile_store_path.is_some() {
                        bail!("only one --profile-store value is allowed");
                    }
                    profile_store_path = Some(PathBuf::from(&value));
                    launcher_args.push(arg);
                    launcher_args.push(value);
                }
                "--ssh-profile-id" => {
                    let value = args
                        .next()
                        .with_context(|| format!("{arg} requires a value"))?;
                    if ssh_profile_id.is_some() {
                        bail!("only one --ssh-profile-id value is allowed");
                    }
                    ssh_profile_id = Some(value.clone());
                    launcher_args.push(arg);
                    launcher_args.push(value);
                }
                "--open-browser" => {
                    launcher_args.push(arg);
                    launcher_only_args_seen = true;
                }
                "--profile-store-list" => {
                    mode = AppMode::ProfileStore;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::List,
                    )?;
                }
                "--profile-store-add" => {
                    mode = AppMode::ProfileStore;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::Add {
                            profile_json: PathBuf::new(),
                            default_policy: ProfileStoreDefaultPolicy::SetIfEmpty,
                        },
                    )?;
                }
                "--profile-store-update" => {
                    mode = AppMode::ProfileStore;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::Update {
                            profile_json: PathBuf::new(),
                        },
                    )?;
                }
                "--profile-store-remove" => {
                    mode = AppMode::ProfileStore;
                    let id = args
                        .next()
                        .context("--profile-store-remove requires a profile id")?;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::Remove { id },
                    )?;
                }
                "--profile-store-check-launch" => {
                    mode = AppMode::ProfileStore;
                    let id = args
                        .next()
                        .context("--profile-store-check-launch requires a profile id")?;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::CheckLaunch { id },
                    )?;
                }
                "--profile-store-set-default" => {
                    mode = AppMode::ProfileStore;
                    let id = args
                        .next()
                        .context("--profile-store-set-default requires a profile id")?;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::SetDefault { id },
                    )?;
                }
                "--profile-store-clear-default" => {
                    mode = AppMode::ProfileStore;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::ClearDefault,
                    )?;
                }
                "--profile-store-import-openssh-preview" => {
                    mode = AppMode::ProfileStore;
                    let config_path = args
                        .next()
                        .context("--profile-store-import-openssh-preview requires a path")?;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::ImportOpenSshPreview {
                            config_path: PathBuf::from(config_path),
                        },
                    )?;
                }
                "--profile-store-import-openssh" => {
                    mode = AppMode::ProfileStore;
                    let config_path = args
                        .next()
                        .context("--profile-store-import-openssh requires a path")?;
                    set_profile_store_command(
                        &mut profile_store_command,
                        ProfileStoreCommand::ImportOpenSsh {
                            config_path: PathBuf::from(config_path),
                            selection: OpenSshImportSelection::all(),
                            conflict_policy: OpenSshImportConflictPolicy::Reject,
                        },
                    )?;
                }
                "--confirm" => {
                    if confirm_profile_store_import {
                        bail!("only one --confirm is allowed");
                    }
                    confirm_profile_store_import = true;
                }
                "--conflict" => {
                    let value = args
                        .next()
                        .context("--conflict requires reject or replace")?;
                    if openssh_import_conflict_policy.is_some() {
                        bail!("only one --conflict value is allowed");
                    }
                    openssh_import_conflict_policy =
                        Some(OpenSshImportConflictPolicy::parse_cli_value(&value)?);
                }
                "--import-profile-id" => {
                    let value = args
                        .next()
                        .context("--import-profile-id requires a profile id")?;
                    openssh_import_profile_ids.push(value);
                }
                "--set-default" => {
                    set_default_profile = true;
                }
                "--wasm-plugin" => {
                    let Some(path) = args.next() else {
                        bail!("--wasm-plugin requires a path");
                    };
                    wasm_plugins.push(PathBuf::from(path));
                }
                "--plugin-dir" => {
                    let Some(path) = args.next() else {
                        bail!("--plugin-dir requires a path");
                    };
                    wasm_plugins.extend(discover_wasm_plugins(path)?);
                }
                _ => bail!("unknown argument {arg}"),
            }
        }

        let profile_store_command_requested = profile_store_command.is_some();
        if profile_store_command_requested && mode != AppMode::ProfileStore {
            bail!("profile store commands cannot be combined with other modes");
        }
        let profile_store = if mode == AppMode::ProfileStore {
            if non_profile_mode_seen {
                bail!("profile store commands cannot be combined with other modes");
            }
            if launcher_only_args_seen {
                bail!("launcher options cannot be combined with profile store commands");
            }
            if launch_command_args_seen {
                bail!("launch command options cannot be combined with profile store commands");
            }
            if window_only_args_seen {
                bail!("window options cannot be combined with profile store commands");
            }
            if ssh_profile_id.is_some() {
                bail!("--ssh-profile-id cannot be combined with profile store commands");
            }
            let command = finish_profile_store_command(
                profile_store_command,
                ssh_profile_json,
                set_default_profile,
                confirm_profile_store_import,
                openssh_import_conflict_policy,
                openssh_import_profile_ids,
            )?;
            Some(ProfileStoreCliOptions {
                command,
                store_path: profile_store_path,
            })
        } else {
            if set_default_profile {
                bail!("--set-default requires --profile-store-add");
            }
            if confirm_profile_store_import {
                bail!("--confirm requires --profile-store-import-openssh");
            }
            if openssh_import_conflict_policy.is_some() {
                bail!("--conflict requires --profile-store-import-openssh");
            }
            if !openssh_import_profile_ids.is_empty() {
                bail!("--import-profile-id requires --profile-store-import-openssh");
            }
            None
        };

        if mode != AppMode::Web && mode != AppMode::ProfileStore && launcher_only_args_seen {
            bail!("launcher options require --web");
        }
        if font_list_args_seen && mode != AppMode::FontList {
            bail!("font list options cannot be combined with other modes");
        }
        if wittyrc_args_seen
            && !matches!(
                mode,
                AppMode::Window
                    | AppMode::WindowConfigEffective
                    | AppMode::WittyrcInit
                    | AppMode::WittyrcCheck
                    | AppMode::WittyrcEffective
            )
        {
            bail!("wittyrc options require --window, --window-config-effective, --wittyrc-init, --wittyrc-check, or --wittyrc-effective");
        }
        if no_wittyrc
            && !matches!(
                mode,
                AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
            )
        {
            bail!(
                "--no-wittyrc requires --window, --window-config-effective, or --wittyrc-effective"
            );
        }
        if no_wittyrc && wittyrc_path.is_some() {
            bail!("--wittyrc cannot be combined with --no-wittyrc");
        }
        if launch_command_args_seen
            && !matches!(
                mode,
                AppMode::Web
                    | AppMode::Window
                    | AppMode::WindowConfigEffective
                    | AppMode::WittyrcEffective
            )
        {
            bail!("--program and --arg require --window, --web, --window-config-effective, or --wittyrc-effective");
        }
        if program.is_none() && !program_args.is_empty() {
            bail!("--arg requires --program");
        }
        if window_only_args_seen
            && !matches!(
                mode,
                AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
            )
        {
            bail!("window options require --window, --window-config-effective, or --wittyrc-effective");
        }
        if restore_state_args_seen && mode != AppMode::Window {
            bail!("--restore-state requires --window");
        }
        if window_config_args_seen
            && !matches!(
                mode,
                AppMode::Window
                    | AppMode::WindowConfigCheck
                    | AppMode::WindowConfigEffective
                    | AppMode::WindowConfigInit
                    | AppMode::WittyrcEffective
            )
        {
            bail!("--window-config requires --window, --window-config-check, --window-config-effective, --window-config-init, or --wittyrc-effective");
        }
        if no_window_config && window_config_path.is_some() {
            bail!("--window-config cannot be combined with --no-window-config");
        }
        if mode == AppMode::Web && profile_store_command_requested {
            bail!("profile store commands cannot be combined with --web");
        }
        if mouse_selection_override_explicit
            && !matches!(
                mode,
                AppMode::Web
                    | AppMode::Window
                    | AppMode::WindowConfigEffective
                    | AppMode::WittyrcEffective
            )
        {
            bail!(
                "--mouse-selection-override requires --window, --web, --window-config-effective, or --wittyrc-effective"
            );
        }
        if osc52_clipboard_policy_explicit
            && !matches!(
                mode,
                AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
            )
        {
            bail!(
                "--osc52-clipboard requires --window, --window-config-effective, or --wittyrc-effective"
            );
        }
        if max_scrollback_lines_explicit
            && !matches!(
                mode,
                AppMode::Web
                    | AppMode::Window
                    | AppMode::WindowConfigEffective
                    | AppMode::WittyrcEffective
            )
        {
            bail!("--scrollback-lines requires --window, --web, --window-config-effective, or --wittyrc-effective");
        }
        if mode == AppMode::Web && mouse_selection_override_explicit {
            launcher_args.push("--mouse-selection-override".to_owned());
            launcher_args.push(mouse_selection_override.as_config_value().to_owned());
        }
        if mode == AppMode::Web && max_scrollback_lines_explicit {
            launcher_args.push("--scrollback-lines".to_owned());
            launcher_args.push(max_scrollback_lines.to_string());
        }
        if mode != AppMode::Web {
            launcher_args.clear();
        }
        if matches!(
            mode,
            AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
        ) {
            apply_window_font_env_defaults(&mut font_family, &mut font_paths, env_var)?;
        }

        Ok(Self {
            mode,
            wasm_plugins,
            window_smoke,
            window_title,
            program,
            args: program_args,
            cwd,
            launch_env,
            mouse_selection_override,
            osc52_clipboard_policy,
            max_scrollback_lines,
            font_family,
            font_size,
            terminal_padding,
            background_opacity,
            background_image,
            background_image_fit,
            background_overlay_color,
            background_overlay_opacity,
            terminal_color_theme,
            cursor_shape,
            cursor_blink,
            cursor_blink_rate,
            cursor_style_source,
            session_tab_position,
            session_tab_label_style,
            session_tab_display_policy,
            font_paths,
            restore_state_path,
            wittyrc_path,
            no_wittyrc,
            window_config_path,
            no_window_config,
            explicit,
            launcher_args,
            real_tui_smoke_case,
            profile_store,
            font_list_filter,
        })
    }

    fn apply_native_window_config_defaults(
        &mut self,
        env_var: impl Fn(&str) -> Option<OsString>,
        default_config_path: impl Fn() -> Result<PathBuf>,
        load_config: impl Fn(&Path) -> Result<Option<NativeWindowConfig>>,
    ) -> Result<NativeWindowConfigLoadReport> {
        if !matches!(
            self.mode,
            AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
        ) {
            return Ok(NativeWindowConfigLoadReport::disabled());
        }
        if self.no_window_config {
            return Ok(NativeWindowConfigLoadReport::disabled());
        }

        let Some(config_ref) = self.native_window_config_ref(env_var, default_config_path)? else {
            return Ok(NativeWindowConfigLoadReport::disabled());
        };
        let config = load_config(&config_ref.path)
            .with_context(|| format!("load native window config {}", config_ref.path.display()))?;
        let Some(config) = config else {
            if config_ref.required {
                bail!(
                    "native window config file does not exist: {}",
                    config_ref.path.display()
                );
            }
            return Ok(NativeWindowConfigLoadReport {
                path: Some(config_ref.path),
                required: config_ref.required,
                status: NativeWindowConfigLoadStatus::Missing,
            });
        };

        let mut applied = self.clone();
        applied
            .apply_native_window_config(config)
            .with_context(|| format!("apply native window config {}", config_ref.path.display()))?;
        *self = applied;
        Ok(NativeWindowConfigLoadReport {
            path: Some(config_ref.path),
            required: config_ref.required,
            status: NativeWindowConfigLoadStatus::Loaded,
        })
    }

    fn native_window_config_ref(
        &self,
        env_var: impl Fn(&str) -> Option<OsString>,
        default_config_path: impl Fn() -> Result<PathBuf>,
    ) -> Result<Option<NativeWindowConfigRef>> {
        if let Some(path) = &self.window_config_path {
            return Ok(Some(NativeWindowConfigRef {
                path: path.clone(),
                required: true,
            }));
        }
        if let Some(path) = env_var(WITTY_WINDOW_CONFIG_ENV) {
            return Ok(Some(NativeWindowConfigRef {
                path: parse_window_config_os_path(path, WITTY_WINDOW_CONFIG_ENV)?,
                required: true,
            }));
        }
        Ok(Some(NativeWindowConfigRef {
            path: default_config_path()?,
            required: false,
        }))
    }

    fn apply_native_window_config(&mut self, config: NativeWindowConfig) -> Result<()> {
        if !self.explicit.window_last_active_close_policy {
            if let Some(value) = config.window_last_active_close {
                self.window_smoke.last_active_close_policy =
                    parse_window_last_active_close_policy_config(&value)?;
            }
        }
        if !self.explicit.window_title && self.window_title.is_none() {
            if let Some(value) = config.window_title {
                self.window_title = Some(parse_window_title(&value, "window_title")?);
            }
        }
        if !self.explicit.program && self.program.is_none() {
            if let Some(value) = config.program {
                self.program = Some(parse_launch_program(&value, "program")?);
            }
        }
        if !self.explicit.program && !self.explicit.args && self.args.is_empty() {
            if !config.args.is_empty() {
                self.args = config.args;
            }
        }
        if self.program.is_none() && !self.args.is_empty() {
            bail!("args requires program");
        }
        if !self.explicit.launch_env && self.launch_env.is_empty() && !config.env.is_empty() {
            self.launch_env = parse_launch_env_config(config.env)?;
        }
        if !self.explicit.mouse_selection_override {
            if let Some(value) = config.mouse_selection_override {
                self.mouse_selection_override =
                    parse_mouse_selection_override_config_value(&value)?;
            }
        }
        if !self.explicit.osc52_clipboard_policy {
            if let Some(value) = config.osc52_clipboard {
                self.osc52_clipboard_policy = parse_osc52_clipboard_policy_config(&value)?;
            }
        }
        if !self.explicit.max_scrollback_lines {
            if let Some(value) = config.scrollback_lines {
                self.max_scrollback_lines = value;
            }
        }
        if !self.explicit.font_family && self.font_family.is_none() {
            if let Some(value) = config.font_family {
                self.font_family = Some(parse_font_family_config(&value)?);
            }
        }
        if !self.explicit.font_size && self.font_size.is_none() {
            if let Some(value) = config.font_size {
                self.font_size = Some(validate_font_size(value, "font_size")?);
            }
        }
        if !self.explicit.terminal_padding && self.terminal_padding.is_none() {
            if let Some(value) = config.terminal_padding {
                self.terminal_padding = Some(validate_terminal_padding(value, "terminal_padding")?);
            }
        }
        if !self.explicit.background_opacity && self.background_opacity.is_none() {
            if let Some(value) = config.background_opacity {
                self.background_opacity =
                    Some(validate_background_opacity(value, "background_opacity")?);
            }
        }
        if !self.explicit.background_image && self.background_image.is_none() {
            if let Some(path) = config.background_image {
                self.background_image =
                    Some(validate_background_image_path(path, "background_image")?);
            }
        }
        if !self.explicit.background_image_fit && self.background_image_fit.is_none() {
            if let Some(value) = config.background_image_fit {
                self.background_image_fit =
                    Some(parse_background_image_fit(&value, "background_image_fit")?);
            }
        }
        if !self.explicit.background_overlay_color && self.background_overlay_color.is_none() {
            if let Some(value) = config.background_overlay_color {
                self.background_overlay_color = Some(parse_background_overlay_color(
                    &value,
                    "background_overlay_color",
                )?);
            }
        }
        if !self.explicit.background_overlay_opacity
            && self.background_overlay_opacity.is_none()
        {
            if let Some(value) = config.background_overlay_opacity {
                self.background_overlay_opacity =
                    Some(validate_background_overlay_opacity(value, "background_overlay_opacity")?);
            }
        }
        if !self.explicit.cursor_shape {
            if let Some(value) = config.cursor_shape {
                self.cursor_shape = parse_cursor_shape(&value, "cursor_shape")?;
            }
        }
        if !self.explicit.cursor_blink {
            if let Some(value) = config.cursor_blink {
                self.cursor_blink = value;
            }
        }
        if !self.explicit.cursor_blink_rate {
            if let Some(value) = config.cursor_blink_rate {
                self.cursor_blink_rate = CursorBlinkRate::parse_config_value(&value)
                    .with_context(|| "cursor_blink_rate")?;
            }
        }
        if !self.explicit.cursor_style_source {
            if let Some(value) = config.cursor_style_source {
                self.cursor_style_source = CursorStyleSource::parse_config_value(&value)
                    .with_context(|| "cursor_style_source")?;
            }
        }
        if !self.explicit.session_tab_position {
            if let Some(value) = config.session_tab_position {
                self.session_tab_position =
                    parse_session_tab_position(&value, "session_tab_position")?;
            }
        }
        if !self.explicit.session_tab_label_style {
            if let Some(value) = config.session_tab_label {
                self.session_tab_label_style =
                    parse_session_tab_label_style(&value, "session_tab_label")?;
            }
        }
        if !self.explicit.session_tab_show_single {
            if let Some(value) = config.session_tab_show_single {
                self.session_tab_display_policy.show_single = value;
            }
        }
        if !self.explicit.session_tab_show_multiple {
            if let Some(value) = config.session_tab_show_multiple {
                self.session_tab_display_policy.show_multiple = value;
            }
        }
        if !self.explicit.font_paths && self.font_paths.is_empty() && !config.font_paths.is_empty()
        {
            self.font_paths = config
                .font_paths
                .into_iter()
                .map(|path| validate_font_path(path, "font_paths"))
                .collect::<Result<Vec<_>>>()?;
        }
        if !self.explicit.cwd && self.cwd.is_none() {
            if let Some(path) = config.cwd {
                self.cwd = Some(validate_cwd_path(path, "cwd")?);
            }
        }
        if (!self.explicit.window_cols && config.window_cols.is_some())
            || (!self.explicit.window_rows && config.window_rows.is_some())
        {
            let current_size = self
                .window_smoke
                .initial_size
                .unwrap_or(GridSize::new(DEFAULT_WINDOW_ROWS, DEFAULT_WINDOW_COLS));
            let cols = match (self.explicit.window_cols, config.window_cols) {
                (true, _) => current_size.cols,
                (false, Some(cols)) => validate_window_cols(cols, "window_cols")?,
                (false, None) => current_size.cols,
            };
            let rows = match (self.explicit.window_rows, config.window_rows) {
                (true, _) => current_size.rows,
                (false, Some(rows)) => validate_window_rows(rows, "window_rows")?,
                (false, None) => current_size.rows,
            };
            self.window_smoke.initial_size = Some(GridSize::new(rows, cols));
        }
        Ok(())
    }

    fn apply_wittyrc_defaults(
        &mut self,
        default_config_path: impl Fn() -> Result<PathBuf>,
        load_config: impl Fn(&Path) -> Result<Option<WittyrcConfig>>,
    ) -> Result<WittyrcConfigLoadReport> {
        if !matches!(
            self.mode,
            AppMode::Window | AppMode::WindowConfigEffective | AppMode::WittyrcEffective
        ) {
            return Ok(WittyrcConfigLoadReport::disabled());
        }
        if self.no_wittyrc {
            return Ok(WittyrcConfigLoadReport::disabled());
        }

        let config_ref = self.wittyrc_config_ref(default_config_path)?;
        let config = load_config(&config_ref.path)
            .with_context(|| format!("load wittyrc {}", config_ref.path.display()))?;
        let Some(config) = config else {
            if config_ref.required {
                bail!("wittyrc file does not exist: {}", config_ref.path.display());
            }
            return Ok(WittyrcConfigLoadReport {
                path: Some(config_ref.path),
                required: config_ref.required,
                status: WittyrcConfigLoadStatus::Missing,
            });
        };

        let mut applied = self.clone();
        applied
            .apply_wittyrc_config(config)
            .with_context(|| format!("apply wittyrc {}", config_ref.path.display()))?;
        *self = applied;
        Ok(WittyrcConfigLoadReport {
            path: Some(config_ref.path),
            required: config_ref.required,
            status: WittyrcConfigLoadStatus::Loaded,
        })
    }

    fn wittyrc_config_ref(
        &self,
        default_config_path: impl Fn() -> Result<PathBuf>,
    ) -> Result<WittyrcConfigRef> {
        if let Some(path) = &self.wittyrc_path {
            return Ok(WittyrcConfigRef {
                path: path.clone(),
                required: true,
            });
        }
        Ok(WittyrcConfigRef {
            path: default_config_path()?,
            required: false,
        })
    }

    fn apply_wittyrc_config(&mut self, config: WittyrcConfig) -> Result<()> {
        if !self.explicit.font_family && self.font_family.is_none() {
            if let Some(value) = config.font_family {
                self.font_family = Some(parse_wittyrc_font_family(&value)?);
            }
        }
        if !self.explicit.font_size && self.font_size.is_none() {
            if let Some(value) = config.font_size {
                self.font_size = Some(validate_font_size(value, "font-size")?);
            }
        }
        if !self.explicit.terminal_padding && self.terminal_padding.is_none() {
            if let Some(value) = config.terminal_padding {
                self.terminal_padding = Some(validate_terminal_padding(value, "terminal-padding")?);
                self.explicit.terminal_padding = true;
            }
        }
        if !self.explicit.background_opacity && self.background_opacity.is_none() {
            if let Some(value) = config.background_opacity {
                self.background_opacity =
                    Some(validate_background_opacity(value, "background-opacity")?);
                self.explicit.background_opacity = true;
            }
        }
        if !self.explicit.background_image && self.background_image.is_none() {
            if let Some(value) = config.background_image {
                self.background_image = parse_background_image_value(&value, "background-image")?;
                self.explicit.background_image = true;
            }
        }
        if !self.explicit.background_image_fit && self.background_image_fit.is_none() {
            if let Some(value) = config.background_image_fit {
                self.background_image_fit =
                    Some(parse_background_image_fit(&value, "background-image-fit")?);
                self.explicit.background_image_fit = true;
            }
        }
        if !self.explicit.background_overlay_color && self.background_overlay_color.is_none() {
            if let Some(value) = config.background_overlay_color {
                self.background_overlay_color = Some(parse_background_overlay_color(
                    &value,
                    "background-overlay-color",
                )?);
                self.explicit.background_overlay_color = true;
            }
        }
        if !self.explicit.background_overlay_opacity
            && self.background_overlay_opacity.is_none()
        {
            if let Some(value) = config.background_overlay_opacity {
                self.background_overlay_opacity = Some(validate_background_overlay_opacity(
                    value,
                    "background-overlay-opacity",
                )?);
                self.explicit.background_overlay_opacity = true;
            }
        }
        self.terminal_color_theme = parse_wittyrc_terminal_color_theme(
            self.terminal_color_theme,
            config.theme_foreground,
            config.theme_background,
            config.theme_cursor,
            config.theme_palette,
        )?;
        if !self.explicit.cursor_shape {
            if let Some(value) = config.cursor_shape {
                self.cursor_shape = parse_cursor_shape(&value, "cursor-shape")?;
                self.explicit.cursor_shape = true;
            }
        }
        if !self.explicit.cursor_blink {
            if let Some(value) = config.cursor_blink {
                self.cursor_blink = value;
                self.explicit.cursor_blink = true;
            }
        }
        if !self.explicit.cursor_blink_rate {
            if let Some(value) = config.cursor_blink_rate {
                self.cursor_blink_rate =
                    CursorBlinkRate::parse_config_value(&value).with_context(|| {
                        "cursor-blink-rate"
                    })?;
                self.explicit.cursor_blink_rate = true;
            }
        }
        if !self.explicit.cursor_style_source {
            if let Some(value) = config.cursor_style_source {
                self.cursor_style_source =
                    CursorStyleSource::parse_config_value(&value).with_context(|| {
                        "cursor-style-source"
                    })?;
                self.explicit.cursor_style_source = true;
            }
        }
        if !self.explicit.window_last_active_close_policy {
            if let Some(value) = config.window_last_active_close {
                self.window_smoke.last_active_close_policy =
                    parse_window_last_active_close_policy_value(
                        &value,
                        "window-last-active-close",
                    )?;
                self.explicit.window_last_active_close_policy = true;
            }
        }
        if !self.explicit.session_tab_position {
            if let Some(value) = config.session_tab_position {
                self.session_tab_position =
                    parse_session_tab_position(&value, "session-tab-position")?;
                self.explicit.session_tab_position = true;
            }
        }
        if !self.explicit.session_tab_label_style {
            if let Some(value) = config.session_tab_label {
                self.session_tab_label_style =
                    parse_session_tab_label_style(&value, "session-tab-label")?;
                self.explicit.session_tab_label_style = true;
            }
        }
        if !self.explicit.session_tab_show_single {
            if let Some(value) = config.session_tab_show_single {
                self.session_tab_display_policy.show_single = value;
                self.explicit.session_tab_show_single = true;
            }
        }
        if !self.explicit.session_tab_show_multiple {
            if let Some(value) = config.session_tab_show_multiple {
                self.session_tab_display_policy.show_multiple = value;
                self.explicit.session_tab_show_multiple = true;
            }
        }
        if !self.explicit.osc52_clipboard_policy {
            if let Some(value) = config.osc52_clipboard {
                self.osc52_clipboard_policy =
                    parse_osc52_clipboard_policy_value(&value, "osc52-clipboard")?;
                self.explicit.osc52_clipboard_policy = true;
            }
        }
        Ok(())
    }
}

fn apply_window_font_env_defaults(
    font_family: &mut Option<String>,
    font_paths: &mut Vec<PathBuf>,
    env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<()> {
    if font_family.is_none() {
        if let Some(value) = env_var(WITTY_FONT_FAMILY_ENV) {
            *font_family = Some(parse_font_family_env(value)?);
        }
    }
    if font_paths.is_empty() {
        if let Some(value) = env_var(WITTY_FONT_PATHS_ENV) {
            font_paths.extend(parse_font_paths_env(value)?);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeWindowConfigRef {
    path: PathBuf,
    required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeWindowConfigLoadReport {
    path: Option<PathBuf>,
    required: bool,
    status: NativeWindowConfigLoadStatus,
}

impl NativeWindowConfigLoadReport {
    fn disabled() -> Self {
        Self {
            path: None,
            required: false,
            status: NativeWindowConfigLoadStatus::Disabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeWindowConfigLoadStatus {
    Disabled,
    Missing,
    Loaded,
}

impl NativeWindowConfigLoadStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Missing => "missing",
            Self::Loaded => "loaded",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WittyrcConfigRef {
    path: PathBuf,
    required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WittyrcConfigLoadReport {
    path: Option<PathBuf>,
    required: bool,
    status: WittyrcConfigLoadStatus,
}

impl WittyrcConfigLoadReport {
    fn disabled() -> Self {
        Self {
            path: None,
            required: false,
            status: WittyrcConfigLoadStatus::Disabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WittyrcConfigLoadStatus {
    Disabled,
    Missing,
    Loaded,
    Failed,
}

impl WittyrcConfigLoadStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Missing => "missing",
            Self::Loaded => "loaded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct NativeWindowConfig {
    #[serde(default)]
    window_title: Option<String>,
    #[serde(default)]
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    font_family: Option<String>,
    #[serde(default)]
    font_size: Option<u16>,
    #[serde(default)]
    terminal_padding: Option<u16>,
    #[serde(default)]
    background_opacity: Option<f32>,
    #[serde(default)]
    background_image: Option<PathBuf>,
    #[serde(default)]
    background_image_fit: Option<String>,
    #[serde(default)]
    background_overlay_color: Option<String>,
    #[serde(default)]
    background_overlay_opacity: Option<f32>,
    #[serde(default)]
    cursor_shape: Option<String>,
    #[serde(default)]
    cursor_blink: Option<bool>,
    #[serde(default)]
    cursor_blink_rate: Option<String>,
    #[serde(default)]
    cursor_style_source: Option<String>,
    #[serde(default)]
    session_tab_position: Option<String>,
    #[serde(default)]
    session_tab_label: Option<String>,
    #[serde(default)]
    session_tab_show_single: Option<bool>,
    #[serde(default)]
    session_tab_show_multiple: Option<bool>,
    #[serde(default)]
    font_paths: Vec<PathBuf>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    scrollback_lines: Option<usize>,
    #[serde(default)]
    mouse_selection_override: Option<String>,
    #[serde(default)]
    osc52_clipboard: Option<String>,
    #[serde(default)]
    window_last_active_close: Option<String>,
    #[serde(default)]
    window_cols: Option<u16>,
    #[serde(default)]
    window_rows: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct WittyrcConfig {
    #[serde(default, rename = "font-family")]
    font_family: Option<String>,
    #[serde(default, rename = "font-size")]
    font_size: Option<u16>,
    #[serde(default, rename = "terminal-padding")]
    terminal_padding: Option<u16>,
    #[serde(default, rename = "background-opacity")]
    background_opacity: Option<f32>,
    #[serde(default, rename = "background-image")]
    background_image: Option<String>,
    #[serde(default, rename = "background-image-fit")]
    background_image_fit: Option<String>,
    #[serde(default, rename = "background-overlay-color")]
    background_overlay_color: Option<String>,
    #[serde(default, rename = "background-overlay-opacity")]
    background_overlay_opacity: Option<f32>,
    #[serde(default, rename = "theme-foreground")]
    theme_foreground: Option<String>,
    #[serde(default, rename = "theme-background")]
    theme_background: Option<String>,
    #[serde(default, rename = "theme-cursor")]
    theme_cursor: Option<String>,
    #[serde(default, rename = "theme-palette")]
    theme_palette: Vec<String>,
    #[serde(default, rename = "window-last-active-close")]
    window_last_active_close: Option<String>,
    #[serde(default, rename = "cursor-shape")]
    cursor_shape: Option<String>,
    #[serde(default, rename = "cursor-blink")]
    cursor_blink: Option<bool>,
    #[serde(default, rename = "cursor-blink-rate")]
    cursor_blink_rate: Option<String>,
    #[serde(default, rename = "cursor-style-source")]
    cursor_style_source: Option<String>,
    #[serde(default, rename = "session-tab-position")]
    session_tab_position: Option<String>,
    #[serde(default, rename = "session-tab-label")]
    session_tab_label: Option<String>,
    #[serde(default, rename = "session-tab-show-single")]
    session_tab_show_single: Option<bool>,
    #[serde(default, rename = "session-tab-show-multiple")]
    session_tab_show_multiple: Option<bool>,
    #[serde(default, rename = "osc52-clipboard")]
    osc52_clipboard: Option<String>,
}

fn default_native_window_config_path() -> Result<PathBuf> {
    let profile_store_path = default_profile_store_path()?;
    let parent = profile_store_path
        .parent()
        .context("default profile store path has no parent")?;
    Ok(parent.join(NATIVE_WINDOW_CONFIG_FILE_NAME))
}

fn default_wittyrc_path() -> Result<PathBuf> {
    default_wittyrc_path_with_home(|| env::var_os("HOME"))
}

fn default_wittyrc_path_with_home(home_var: impl FnOnce() -> Option<OsString>) -> Result<PathBuf> {
    let home = home_var().context("HOME is not set; cannot resolve default .wittyrc path")?;
    if home.is_empty() {
        bail!("HOME is empty; cannot resolve default .wittyrc path");
    }
    Ok(PathBuf::from(home).join(WITTYRC_CONFIG_FILE_NAME))
}

fn read_native_window_config(path: &Path) -> Result<Option<NativeWindowConfig>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
    };
    if !metadata.is_file() {
        bail!(
            "native window config path is not a file: {}",
            path.display()
        );
    }
    if metadata.len() > NATIVE_WINDOW_CONFIG_MAX_JSON_BYTES {
        bail!(
            "native window config exceeds {} bytes: {}",
            NATIVE_WINDOW_CONFIG_MAX_JSON_BYTES,
            path.display()
        );
    }

    let json = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let config =
        serde_json::from_str(&json).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(config))
}

fn read_wittyrc_config(path: &Path) -> Result<Option<WittyrcConfig>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
    };
    if !metadata.is_file() {
        bail!("wittyrc path is not a file: {}", path.display());
    }
    if metadata.len() > WITTYRC_CONFIG_MAX_TOML_BYTES {
        bail!(
            "wittyrc exceeds {} bytes: {}",
            WITTYRC_CONFIG_MAX_TOML_BYTES,
            path.display()
        );
    }

    let toml_text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let config = toml::from_str(&toml_text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(config))
}

fn native_window_config_template() -> String {
    let template = serde_json::json!({
        "font_family": RECOMMENDED_TERMINAL_FONT_FAMILY,
        "font_size": 16,
        "terminal_padding": 0,
        "background_opacity": 1.0,
        "background_image": null,
        "background_image_fit": "cover",
        "background_overlay_color": "#000000",
        "background_overlay_opacity": 0.0,
        "cursor_shape": "block",
        "cursor_blink": true,
        "cursor_blink_rate": "normal",
        "cursor_style_source": "program",
        "session_tab_position": "top",
        "session_tab_label": "index",
        "session_tab_show_single": false,
        "session_tab_show_multiple": false,
        "window_title": "Witty",
        "program": null,
        "args": [],
        "env": {
            "WITTY_SESSION": "daily"
        },
        "font_paths": [],
        "cwd": "~",
        "window_cols": 120,
        "window_rows": 36,
        "scrollback_lines": 20000,
        "mouse_selection_override": "shift-select",
        "osc52_clipboard": "disabled",
        "window_last_active_close": "close-window"
    });
    let mut text = serde_json::to_string_pretty(&template)
        .expect("native window config template should serialize");
    text.push('\n');
    text
}

fn wittyrc_template() -> &'static str {
    WITTYRC_TEMPLATE
}

fn native_window_config_default_path_line(
    default_config_path: impl Fn() -> Result<PathBuf>,
) -> Result<String> {
    let path = default_config_path()?;
    Ok(format!("{}\n", path.display()))
}

fn wittyrc_default_path_line(default_config_path: impl Fn() -> Result<PathBuf>) -> Result<String> {
    let path = default_config_path()?;
    Ok(format!("{}\n", path.display()))
}

fn init_native_window_config(default_config_path: impl Fn() -> Result<PathBuf>) -> Result<String> {
    let path = default_config_path()?;
    init_native_window_config_at_path(&path)
}

fn init_native_window_config_for_options(
    options: &AppOptions,
    default_config_path: impl Fn() -> Result<PathBuf>,
) -> Result<String> {
    match &options.window_config_path {
        Some(path) => init_native_window_config_at_path(path),
        None => init_native_window_config(default_config_path),
    }
}

fn init_wittyrc(default_config_path: impl Fn() -> Result<PathBuf>) -> Result<String> {
    let path = default_config_path()?;
    init_wittyrc_at_path(&path)
}

fn init_wittyrc_for_options(
    options: &AppOptions,
    default_config_path: impl Fn() -> Result<PathBuf>,
) -> Result<String> {
    match &options.wittyrc_path {
        Some(path) => init_wittyrc_at_path(path),
        None => init_wittyrc(default_config_path),
    }
}

fn init_native_window_config_at_path(path: &Path) -> Result<String> {
    write_native_window_config_template(path)?;
    Ok(format!(
        "created native window config: {}\n",
        path.display()
    ))
}

fn init_wittyrc_at_path(path: &Path) -> Result<String> {
    write_wittyrc_template(path)?;
    Ok(format!("created wittyrc: {}\n", path.display()))
}

fn write_native_window_config_template(path: &Path) -> Result<()> {
    let parent = path.parent().with_context(|| {
        format!(
            "native window config path has no parent: {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create native window config {}", path.display()))?;
    file.write_all(native_window_config_template().as_bytes())
        .with_context(|| format!("write native window config {}", path.display()))
}

fn write_wittyrc_template(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create wittyrc {}", path.display()))?;
    file.write_all(wittyrc_template().as_bytes())
        .with_context(|| format!("write wittyrc {}", path.display()))
}

fn check_native_window_config(
    options: &AppOptions,
    env_var: impl Fn(&str) -> Option<OsString>,
    default_config_path: impl Fn() -> Result<PathBuf>,
    load_config: impl Fn(&Path) -> Result<Option<NativeWindowConfig>>,
) -> Result<String> {
    let config_ref = options
        .native_window_config_ref(env_var, default_config_path)?
        .expect("native window config ref should always resolve");
    let config = load_config(&config_ref.path)
        .with_context(|| format!("load native window config {}", config_ref.path.display()))?;
    let Some(config) = config else {
        if config_ref.required {
            bail!(
                "native window config file does not exist: {}",
                config_ref.path.display()
            );
        }
        return Ok(format!(
            "native window config missing: {}\n",
            config_ref.path.display()
        ));
    };

    let mut check_options = AppOptions::parse_with_env(
        ["--window".to_owned(), "--no-window-config".to_owned()],
        |_| None,
    )?;
    check_options
        .apply_native_window_config(config)
        .with_context(|| format!("apply native window config {}", config_ref.path.display()))?;
    Ok(format!(
        "native window config ok: {}\n",
        config_ref.path.display()
    ))
}

fn check_wittyrc_config(
    options: &AppOptions,
    default_config_path: impl Fn() -> Result<PathBuf>,
    load_config: impl Fn(&Path) -> Result<Option<WittyrcConfig>>,
) -> Result<String> {
    let config_ref = options.wittyrc_config_ref(default_config_path)?;
    let config = load_config(&config_ref.path)
        .with_context(|| format!("load wittyrc {}", config_ref.path.display()))?;
    let Some(config) = config else {
        if config_ref.required {
            bail!("wittyrc file does not exist: {}", config_ref.path.display());
        }
        return Ok(format!("wittyrc missing: {}\n", config_ref.path.display()));
    };

    let mut check_options =
        AppOptions::parse_with_env(["--window".to_owned(), "--no-wittyrc".to_owned()], |_| None)?;
    check_options
        .apply_wittyrc_config(config)
        .with_context(|| format!("apply wittyrc {}", config_ref.path.display()))?;
    Ok(format!("wittyrc ok: {}\n", config_ref.path.display()))
}

fn native_window_effective_config_summary(
    options: &AppOptions,
    config_load: &NativeWindowConfigLoadReport,
    wittyrc_load: &WittyrcConfigLoadReport,
) -> Result<String> {
    let font_config = renderer_font_config_for_options(options);
    let visual_config = renderer_visual_config_for_options(options, None);
    let grid = options
        .window_smoke
        .initial_size
        .unwrap_or(GridSize::new(DEFAULT_WINDOW_ROWS, DEFAULT_WINDOW_COLS));
    let backend_policy = native_wgpu_backend_policy();
    let env_keys = options
        .launch_env
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    let config_path = config_load
        .path
        .as_ref()
        .map(|path| path.display().to_string());
    let cwd = options.cwd.as_ref().map(|path| path.display().to_string());

    let value = serde_json::json!({
        "event": "witty.native_window_config_effective",
        "opens_window": false,
        "starts_pty": false,
        "reads_font_files": false,
        "renderer": "wgpu",
        "native_backend_policy": backend_policy.label(),
        "opengl_only": backend_policy.is_opengl_only(),
        "vulkan_enabled_by_witty": false,
        "chromium": false,
        "config": {
            "status": config_load.status.as_str(),
            "path": config_path,
            "required": config_load.required,
        },
        "wittyrc": {
            "status": wittyrc_load.status.as_str(),
            "path": wittyrc_load
                .path
                .as_ref()
                .map(|path| path.display().to_string()),
            "required": wittyrc_load.required,
        },
        "window_title": options
            .window_title
            .as_deref()
            .unwrap_or(DEFAULT_WINDOW_TITLE),
        "window_title_configured": options.window_title.is_some(),
        "program_configured": options.program.is_some(),
        "program": options.program.as_deref(),
        "arg_count": options.args.len(),
        "cwd": cwd,
        "env_keys": env_keys,
        "font_family": font_config.family(),
        "font_size": font_config.font_size(),
        "terminal_padding": font_config.terminal_padding(),
        "background_opacity": visual_config.background_opacity(),
        "background_image": options
            .background_image
            .as_ref()
            .map(|path| path.display().to_string()),
        "background_image_fit": visual_config.background_image_fit().as_config_value(),
        "background_overlay_color": terminal_color_config_value(visual_config.background_overlay_color()),
        "background_overlay_opacity": visual_config.background_overlay_opacity(),
        "terminal_theme": terminal_theme_summary_value(options.terminal_color_theme),
        "cursor_shape": cursor_shape_config_value(options.cursor_shape),
        "cursor_blink": options.cursor_blink,
        "cursor_blink_rate": options.cursor_blink_rate.as_config_value(),
        "cursor_style_source": options.cursor_style_source.as_config_value(),
        "session_tab_position": options.session_tab_position.as_config_value(),
        "session_tab_label": options.session_tab_label_style.as_config_value(),
        "session_tab_show_single": options.session_tab_display_policy.show_single,
        "session_tab_show_multiple": options.session_tab_display_policy.show_multiple,
        "font_source_count": options.font_paths.len(),
        "window_cols": grid.cols,
        "window_rows": grid.rows,
        "scrollback_lines": options.max_scrollback_lines,
        "mouse_selection_override": options.mouse_selection_override.as_config_value(),
        "osc52_clipboard": options.osc52_clipboard_policy,
        "window_last_active_close": options.window_smoke.last_active_close_policy.as_config_value(),
    });
    let mut text = serde_json::to_string_pretty(&value)
        .context("serialize native window effective config summary")?;
    text.push('\n');
    Ok(text)
}

fn wittyrc_effective_config_summary(
    options: &AppOptions,
    wittyrc_load: &WittyrcConfigLoadReport,
    window_config_load: &NativeWindowConfigLoadReport,
) -> Result<String> {
    let font_config = renderer_font_config_for_options(options);
    let visual_config = renderer_visual_config_for_options(options, None);
    let value = serde_json::json!({
        "event": "witty.wittyrc_effective",
        "opens_window": false,
        "starts_pty": false,
        "reads_font_files": false,
        "wittyrc": {
            "status": wittyrc_load.status.as_str(),
            "path": wittyrc_load
                .path
                .as_ref()
                .map(|path| path.display().to_string()),
            "required": wittyrc_load.required,
        },
        "window_config": {
            "status": window_config_load.status.as_str(),
            "path": window_config_load
                .path
                .as_ref()
                .map(|path| path.display().to_string()),
            "required": window_config_load.required,
        },
        "font_family": font_config.family(),
        "font_size": font_config.font_size(),
        "terminal_padding": font_config.terminal_padding(),
        "background_opacity": visual_config.background_opacity(),
        "background_image": options
            .background_image
            .as_ref()
            .map(|path| path.display().to_string()),
        "background_image_fit": visual_config.background_image_fit().as_config_value(),
        "background_overlay_color": terminal_color_config_value(visual_config.background_overlay_color()),
        "background_overlay_opacity": visual_config.background_overlay_opacity(),
        "terminal_theme": terminal_theme_summary_value(options.terminal_color_theme),
        "cursor_shape": cursor_shape_config_value(options.cursor_shape),
        "cursor_blink": options.cursor_blink,
        "cursor_blink_rate": options.cursor_blink_rate.as_config_value(),
        "cursor_style_source": options.cursor_style_source.as_config_value(),
        "session_tab_position": options.session_tab_position.as_config_value(),
        "session_tab_label": options.session_tab_label_style.as_config_value(),
        "session_tab_show_single": options.session_tab_display_policy.show_single,
        "session_tab_show_multiple": options.session_tab_display_policy.show_multiple,
        "font_source_count": options.font_paths.len(),
        "window_last_active_close": options.window_smoke.last_active_close_policy.as_config_value(),
    });
    let mut text = serde_json::to_string_pretty(&value)
        .context("serialize wittyrc effective config summary")?;
    text.push('\n');
    Ok(text)
}

fn renderer_font_config_for_options(options: &AppOptions) -> RendererFontConfig {
    let config = match options.font_size {
        Some(font_size) => {
            RendererFontConfig::with_font_size(options.font_family.clone(), font_size)
        }
        None => RendererFontConfig::new(options.font_family.clone()),
    };
    match options.terminal_padding {
        Some(padding) => config.with_terminal_padding(f32::from(padding)),
        None => config,
    }
}

fn renderer_visual_config_for_options(
    options: &AppOptions,
    background_image: Option<witty_render_wgpu::RendererBackgroundImage>,
) -> RendererVisualConfig {
    let config = match options.background_opacity {
        Some(opacity) => RendererVisualConfig::default().with_background_opacity(opacity),
        None => RendererVisualConfig::default(),
    };
    let config = match options.background_image_fit {
        Some(fit) => config.with_background_image_fit(fit),
        None => config,
    };
    let config = match options.background_overlay_color {
        Some(color) => config.with_background_overlay_color(color),
        None => config,
    };
    let config = match options.background_overlay_opacity {
        Some(opacity) => config.with_background_overlay_opacity(opacity),
        None => config,
    };
    config.with_background_image(background_image)
}

fn set_profile_store_command(
    command: &mut Option<ProfileStoreCommand>,
    next: ProfileStoreCommand,
) -> Result<()> {
    if command.is_some() {
        bail!("only one profile store command is allowed");
    }
    *command = Some(next);
    Ok(())
}

fn finish_profile_store_command(
    command: Option<ProfileStoreCommand>,
    ssh_profile_json: Option<PathBuf>,
    set_default: bool,
    confirm_import: bool,
    import_conflict_policy: Option<OpenSshImportConflictPolicy>,
    import_profile_ids: Vec<String>,
) -> Result<ProfileStoreCommand> {
    match command.context("profile store mode requires a profile store command")? {
        ProfileStoreCommand::List => {
            reject_openssh_import_write_options(
                confirm_import,
                import_conflict_policy,
                &import_profile_ids,
            )?;
            if ssh_profile_json.is_some() {
                bail!("--profile-store-list cannot be combined with --ssh-profile-json");
            }
            if set_default {
                bail!("--set-default requires --profile-store-add");
            }
            Ok(ProfileStoreCommand::List)
        }
        ProfileStoreCommand::Add { .. } => {
            reject_openssh_import_write_options(
                confirm_import,
                import_conflict_policy,
                &import_profile_ids,
            )?;
            let profile_json =
                ssh_profile_json.context("--profile-store-add requires --ssh-profile-json")?;
            let default_policy = if set_default {
                ProfileStoreDefaultPolicy::SetToAdded
            } else {
                ProfileStoreDefaultPolicy::SetIfEmpty
            };
            Ok(ProfileStoreCommand::Add {
                profile_json,
                default_policy,
            })
        }
        ProfileStoreCommand::Update { .. } => {
            reject_openssh_import_write_options(
                confirm_import,
                import_conflict_policy,
                &import_profile_ids,
            )?;
            if set_default {
                bail!("--set-default requires --profile-store-add");
            }
            let profile_json =
                ssh_profile_json.context("--profile-store-update requires --ssh-profile-json")?;
            Ok(ProfileStoreCommand::Update { profile_json })
        }
        ProfileStoreCommand::Remove { id } => finish_profile_store_id_command(
            ProfileStoreCommand::Remove { id },
            ssh_profile_json,
            set_default,
            confirm_import,
            import_conflict_policy,
            &import_profile_ids,
        ),
        ProfileStoreCommand::CheckLaunch { id } => finish_profile_store_id_command(
            ProfileStoreCommand::CheckLaunch { id },
            ssh_profile_json,
            set_default,
            confirm_import,
            import_conflict_policy,
            &import_profile_ids,
        ),
        ProfileStoreCommand::SetDefault { id } => finish_profile_store_id_command(
            ProfileStoreCommand::SetDefault { id },
            ssh_profile_json,
            set_default,
            confirm_import,
            import_conflict_policy,
            &import_profile_ids,
        ),
        ProfileStoreCommand::ClearDefault => finish_profile_store_id_command(
            ProfileStoreCommand::ClearDefault,
            ssh_profile_json,
            set_default,
            confirm_import,
            import_conflict_policy,
            &import_profile_ids,
        ),
        ProfileStoreCommand::ImportOpenSshPreview { config_path } => {
            reject_openssh_import_write_options(
                confirm_import,
                import_conflict_policy,
                &import_profile_ids,
            )?;
            if ssh_profile_json.is_some() {
                bail!(
                    "--profile-store-import-openssh-preview cannot be combined with --ssh-profile-json"
                );
            }
            if set_default {
                bail!("--set-default requires --profile-store-add");
            }
            Ok(ProfileStoreCommand::ImportOpenSshPreview { config_path })
        }
        ProfileStoreCommand::ImportOpenSsh { config_path, .. } => {
            if ssh_profile_json.is_some() {
                bail!("--profile-store-import-openssh cannot be combined with --ssh-profile-json");
            }
            if set_default {
                bail!("--set-default requires --profile-store-add");
            }
            if !confirm_import {
                bail!("--profile-store-import-openssh requires --confirm");
            }
            let selection = if import_profile_ids.is_empty() {
                OpenSshImportSelection::all()
            } else {
                OpenSshImportSelection::profile_ids(import_profile_ids)
            };
            Ok(ProfileStoreCommand::ImportOpenSsh {
                config_path,
                selection,
                conflict_policy: import_conflict_policy
                    .unwrap_or(OpenSshImportConflictPolicy::Reject),
            })
        }
    }
}

fn finish_profile_store_id_command(
    command: ProfileStoreCommand,
    ssh_profile_json: Option<PathBuf>,
    set_default: bool,
    confirm_import: bool,
    import_conflict_policy: Option<OpenSshImportConflictPolicy>,
    import_profile_ids: &[String],
) -> Result<ProfileStoreCommand> {
    reject_openssh_import_write_options(
        confirm_import,
        import_conflict_policy,
        import_profile_ids,
    )?;
    if ssh_profile_json.is_some() {
        bail!("this profile store command cannot be combined with --ssh-profile-json");
    }
    if set_default {
        bail!("--set-default requires --profile-store-add");
    }
    Ok(command)
}

fn reject_openssh_import_write_options(
    confirm_import: bool,
    import_conflict_policy: Option<OpenSshImportConflictPolicy>,
    import_profile_ids: &[String],
) -> Result<()> {
    if confirm_import {
        bail!("--confirm requires --profile-store-import-openssh");
    }
    if import_conflict_policy.is_some() {
        bail!("--conflict requires --profile-store-import-openssh");
    }
    if !import_profile_ids.is_empty() {
        bail!("--import-profile-id requires --profile-store-import-openssh");
    }
    Ok(())
}

fn parse_osc52_clipboard_policy(value: &str) -> Result<Osc52ClipboardPolicy> {
    parse_osc52_clipboard_policy_value(value, "--osc52-clipboard")
}

fn parse_osc52_clipboard_policy_config(value: &str) -> Result<Osc52ClipboardPolicy> {
    parse_osc52_clipboard_policy_value(value, "osc52_clipboard")
}

fn parse_osc52_clipboard_policy_value(value: &str, name: &str) -> Result<Osc52ClipboardPolicy> {
    match value {
        "disabled" => Ok(Osc52ClipboardPolicy::Disabled),
        "confirm" => Ok(Osc52ClipboardPolicy::Confirm),
        "allow" => Ok(Osc52ClipboardPolicy::Allow),
        _ => bail!("{name} must be disabled, confirm, or allow"),
    }
}

fn parse_window_last_active_close_policy(value: &str) -> Result<WindowLastActiveClosePolicy> {
    parse_window_last_active_close_policy_value(value, "--window-last-active-close")
}

fn parse_window_last_active_close_policy_config(
    value: &str,
) -> Result<WindowLastActiveClosePolicy> {
    parse_window_last_active_close_policy_value(value, "window_last_active_close")
}

fn parse_window_last_active_close_policy_value(
    value: &str,
    name: &str,
) -> Result<WindowLastActiveClosePolicy> {
    if let Some(policy) = WindowLastActiveClosePolicy::parse_config_value(value) {
        return Ok(policy);
    }

    bail!(
        "{name} must be one of: {}",
        WindowLastActiveClosePolicy::config_values().join(", ")
    )
}

fn parse_mouse_selection_override_config_value(
    value: &str,
) -> Result<MouseSelectionOverridePolicy> {
    MouseSelectionOverridePolicy::parse_config_value(value)
        .with_context(|| format!("mouse_selection_override has invalid value {value:?}"))
}

fn parse_window_title(value: &str, name: &str) -> Result<String> {
    let title = value.trim();
    if title.is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(title.to_owned())
}

fn parse_launch_program(value: &str, name: &str) -> Result<String> {
    let program = value.trim();
    if program.is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(program.to_owned())
}

fn parse_launch_env_pair(value: &str, name: &str) -> Result<(String, String)> {
    let Some((key, env_value)) = value.split_once('=') else {
        bail!("{name} requires KEY=VALUE");
    };
    let key = parse_launch_env_key(key, name)?;
    Ok((key, env_value.to_owned()))
}

fn parse_launch_env_config(env: BTreeMap<String, String>) -> Result<Vec<(String, String)>> {
    let mut parsed = Vec::new();
    for (key, value) in env {
        let key = parse_launch_env_key(&key, "env")?;
        set_launch_env_pair(&mut parsed, (key, value));
    }
    Ok(parsed)
}

fn parse_launch_env_key(key: &str, name: &str) -> Result<String> {
    if key.is_empty() || key.trim() != key {
        bail!("{name} env key cannot be empty or padded");
    }
    if key.contains('=') || key.contains('\0') {
        bail!("{name} env key cannot contain '=' or NUL");
    }
    Ok(key.to_owned())
}

fn set_launch_env_pair(env: &mut Vec<(String, String)>, pair: (String, String)) {
    if let Some(existing) = env.iter_mut().find(|(key, _)| *key == pair.0) {
        existing.1 = pair.1;
    } else {
        env.push(pair);
    }
}

fn parse_font_family(value: &str) -> Result<String> {
    parse_font_family_value(value, "--font-family")
}

fn parse_font_family_config(value: &str) -> Result<String> {
    parse_font_family_value(value, "font_family")
}

fn parse_wittyrc_font_family(value: &str) -> Result<String> {
    parse_font_family_value(value, "font-family")
}

fn parse_font_family_env(value: OsString) -> Result<String> {
    let Ok(value) = value.into_string() else {
        bail!("{WITTY_FONT_FAMILY_ENV} must be valid UTF-8");
    };
    parse_font_family_value(&value, WITTY_FONT_FAMILY_ENV)
}

fn parse_font_family_value(value: &str, name: &str) -> Result<String> {
    let config = RendererFontConfig::new(Some(value.to_owned()));
    config
        .family()
        .map(ToOwned::to_owned)
        .with_context(|| format!("{name} cannot be empty"))
}

fn parse_font_list_filter(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--font-list-filter cannot be empty");
    }
    Ok(value.to_owned())
}

fn parse_font_size(value: &str, name: &str) -> Result<u16> {
    let size = value
        .parse::<u16>()
        .with_context(|| format!("{name} must be an integer"))?;
    validate_font_size(size, name)
}

fn validate_font_size(size: u16, name: &str) -> Result<u16> {
    if !(MIN_TERMINAL_FONT_SIZE..=MAX_TERMINAL_FONT_SIZE).contains(&size) {
        bail!("{name} must be between {MIN_TERMINAL_FONT_SIZE} and {MAX_TERMINAL_FONT_SIZE}");
    }
    Ok(size)
}

fn parse_terminal_padding(value: &str, name: &str) -> Result<u16> {
    let padding = value
        .parse::<u16>()
        .with_context(|| format!("{name} must be an integer"))?;
    validate_terminal_padding(padding, name)
}

fn validate_terminal_padding(padding: u16, name: &str) -> Result<u16> {
    if !(MIN_TERMINAL_PADDING..=MAX_TERMINAL_PADDING).contains(&padding) {
        bail!("{name} must be between {MIN_TERMINAL_PADDING} and {MAX_TERMINAL_PADDING}");
    }
    Ok(padding)
}

fn parse_background_opacity(value: &str, name: &str) -> Result<f32> {
    let opacity = value
        .parse::<f32>()
        .with_context(|| format!("{name} must be a number between 0.0 and 1.0"))?;
    validate_background_opacity(opacity, name)
}

fn validate_background_opacity(opacity: f32, name: &str) -> Result<f32> {
    if !opacity.is_finite() || !(MIN_BACKGROUND_OPACITY..=MAX_BACKGROUND_OPACITY).contains(&opacity)
    {
        bail!("{name} must be between {MIN_BACKGROUND_OPACITY} and {MAX_BACKGROUND_OPACITY}");
    }
    Ok(opacity)
}

fn parse_background_image_value(value: &str, name: &str) -> Result<Option<PathBuf>> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{name} cannot be empty");
    }
    if value.eq_ignore_ascii_case("null") || value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    Ok(Some(validate_background_image_path(
        PathBuf::from(value),
        name,
    )?))
}

fn validate_background_image_path(path: PathBuf, name: &str) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    expand_cwd_home_path(path, name, || env::var_os("HOME"))
}

fn parse_background_image_fit(value: &str, name: &str) -> Result<RendererBackgroundImageFit> {
    let value = value.trim();
    match value {
        "cover" | "scale-crop" | "scale-and-crop" => Ok(RendererBackgroundImageFit::Cover),
        _ => bail!(
            "{name} must be one of: {}",
            RendererBackgroundImageFit::config_values().join(", ")
        ),
    }
}

fn parse_background_overlay_color(value: &str, name: &str) -> Result<Rgba> {
    parse_terminal_theme_color(value, name)
}

fn parse_background_overlay_opacity(value: &str, name: &str) -> Result<f32> {
    let opacity = value
        .parse::<f32>()
        .with_context(|| format!("{name} must be a number between 0.0 and 1.0"))?;
    validate_background_overlay_opacity(opacity, name)
}

fn validate_background_overlay_opacity(opacity: f32, name: &str) -> Result<f32> {
    validate_background_opacity(opacity, name)
}

fn parse_wittyrc_terminal_color_theme(
    current: TerminalColorTheme,
    foreground: Option<String>,
    background: Option<String>,
    cursor: Option<String>,
    palette: Vec<String>,
) -> Result<TerminalColorTheme> {
    let mut theme = current;
    if let Some(value) = foreground {
        theme.foreground = parse_terminal_theme_color(&value, "theme-foreground")?;
    }
    if let Some(value) = background {
        theme.background = parse_terminal_theme_color(&value, "theme-background")?;
    }
    if let Some(value) = cursor {
        theme.cursor_color = parse_optional_terminal_theme_color(&value, "theme-cursor")?;
    }
    if !palette.is_empty() {
        if palette.len() != TerminalColorTheme::ANSI_COLOR_COUNT {
            bail!(
                "theme-palette must contain exactly {} colors",
                TerminalColorTheme::ANSI_COLOR_COUNT
            );
        }
        for (index, value) in palette.iter().enumerate() {
            theme.palette[index] =
                parse_terminal_theme_color(value, &format!("theme-palette[{index}]"))?;
        }
    }
    Ok(theme)
}

fn parse_optional_terminal_theme_color(value: &str, name: &str) -> Result<Option<Rgba>> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{name} cannot be empty");
    }
    if value.eq_ignore_ascii_case("null") || value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    Ok(Some(parse_terminal_theme_color(value, name)?))
}

fn parse_terminal_theme_color(value: &str, name: &str) -> Result<Rgba> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{name} cannot be empty");
    }
    parse_terminal_color(value)
        .with_context(|| format!("{name} must be a color like #rrggbb or rgb:rrrr/gggg/bbbb"))
}

fn terminal_color_config_value(color: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn terminal_optional_color_config_value(color: Option<Rgba>) -> Option<String> {
    color.map(terminal_color_config_value)
}

fn terminal_palette_config_value(
    palette: [Rgba; TerminalColorTheme::ANSI_COLOR_COUNT],
) -> Vec<String> {
    palette
        .into_iter()
        .map(terminal_color_config_value)
        .collect()
}

fn terminal_theme_summary_value(theme: TerminalColorTheme) -> serde_json::Value {
    serde_json::json!({
        "foreground": terminal_color_config_value(theme.foreground),
        "background": terminal_color_config_value(theme.background),
        "cursor": terminal_optional_color_config_value(theme.cursor_color),
        "palette": terminal_palette_config_value(theme.palette),
    })
}

fn parse_cursor_shape(value: &str, name: &str) -> Result<CursorShape> {
    match value.trim().to_ascii_lowercase().as_str() {
        "block" | "box" => Ok(CursorShape::Block),
        "underline" | "horizontal" | "line" => Ok(CursorShape::Underline),
        "bar" | "vertical" | "beam" => Ok(CursorShape::Bar),
        _ => bail!("{name} must be one of: block, underline, bar"),
    }
}

fn cursor_shape_config_value(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Block => "block",
        CursorShape::Underline => "underline",
        CursorShape::Bar => "bar",
    }
}

fn parse_session_tab_position(value: &str, name: &str) -> Result<NativeSessionTabPosition> {
    let value = value.trim();
    if let Some(position) = NativeSessionTabPosition::parse_config_value(value) {
        return Ok(position);
    }

    bail!(
        "{name} must be one of: {}",
        NativeSessionTabPosition::config_values().join(", ")
    )
}

fn parse_session_tab_label_style(value: &str, name: &str) -> Result<NativeSessionTabLabelStyle> {
    let value = value.trim();
    if let Some(style) = NativeSessionTabLabelStyle::parse_config_value(value) {
        return Ok(style);
    }

    bail!(
        "{name} must be one of: {}",
        NativeSessionTabLabelStyle::config_values().join(", ")
    )
}

fn parse_bool_config(value: &str, name: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => bail!("{name} must be one of: true, false, yes, no, 1, 0"),
    }
}

fn parse_window_cols(value: &str, name: &str) -> Result<u16> {
    let cols = value
        .parse::<u16>()
        .with_context(|| format!("{name} must be an integer"))?;
    validate_window_cols(cols, name)
}

fn parse_window_rows(value: &str, name: &str) -> Result<u16> {
    let rows = value
        .parse::<u16>()
        .with_context(|| format!("{name} must be an integer"))?;
    validate_window_rows(rows, name)
}

fn validate_window_cols(cols: u16, name: &str) -> Result<u16> {
    if !(MIN_WINDOW_COLS..=MAX_WINDOW_COLS).contains(&cols) {
        bail!("{name} must be between {MIN_WINDOW_COLS} and {MAX_WINDOW_COLS}");
    }
    Ok(cols)
}

fn validate_window_rows(rows: u16, name: &str) -> Result<u16> {
    if !(MIN_WINDOW_ROWS..=MAX_WINDOW_ROWS).contains(&rows) {
        bail!("{name} must be between {MIN_WINDOW_ROWS} and {MAX_WINDOW_ROWS}");
    }
    Ok(rows)
}

fn parse_font_path(value: &str) -> Result<PathBuf> {
    validate_font_path(PathBuf::from(value), "--font-path")
}

fn parse_window_config_path(value: &str) -> Result<PathBuf> {
    parse_window_config_os_path(OsString::from(value), "--window-config")
}

fn parse_restore_state_path(value: &str) -> Result<PathBuf> {
    parse_window_config_os_path(OsString::from(value), "--restore-state")
}

fn parse_wittyrc_path(value: &str) -> Result<PathBuf> {
    parse_window_config_os_path(OsString::from(value), "--wittyrc")
}

fn parse_cwd_path(value: &str) -> Result<PathBuf> {
    validate_cwd_path(PathBuf::from(value), "--cwd")
}

fn validate_cwd_path(path: PathBuf, name: &str) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    expand_cwd_home_path(path, name, || env::var_os("HOME"))
}

fn expand_cwd_home_path(
    path: PathBuf,
    name: &str,
    home_var: impl FnOnce() -> Option<OsString>,
) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    if text == "~" || text.starts_with("~/") {
        let home = home_var().with_context(|| format!("{name} uses ~ but HOME is not set"))?;
        if home.is_empty() {
            bail!("{name} uses ~ but HOME is empty");
        }
        let mut expanded = PathBuf::from(home);
        if let Some(rest) = text.strip_prefix("~/") {
            expanded.push(rest);
        }
        return Ok(expanded);
    }
    if text.starts_with('~') {
        bail!("{name} only supports ~ or ~/... home expansion");
    }
    Ok(path)
}

fn parse_window_config_os_path(value: OsString, name: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(path)
}

fn validate_font_path(path: PathBuf, name: &str) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
        bail!("{name} cannot contain empty paths");
    }
    Ok(path)
}

fn parse_font_paths_env(value: OsString) -> Result<Vec<PathBuf>> {
    if value.as_os_str().is_empty() {
        bail!("{WITTY_FONT_PATHS_ENV} cannot be empty");
    }
    let mut paths = Vec::new();
    for path in env::split_paths(&value) {
        paths.push(validate_font_path(path, WITTY_FONT_PATHS_ENV)?);
    }
    if paths.is_empty() {
        bail!("{WITTY_FONT_PATHS_ENV} cannot be empty");
    }
    Ok(paths)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppMode {
    Smoke,
    Web,
    Window,
    PtySmoke,
    IncrementalSmoke,
    SelectionCopySmoke,
    PrimarySelectionSmoke,
    PrimarySelectionGuiSmoke,
    NativeSearchSmoke,
    NativeCommandBlockSmoke,
    OpenSshProfileSmoke,
    RealTuiSmoke,
    RendererBackendInfo,
    RendererNoSurfaceDiagnostics,
    KeyboardProtocolDiagnostics,
    KeyboardProtocolCapture,
    FontList,
    WittyrcTemplate,
    WittyrcDefaultPath,
    WittyrcInit,
    WittyrcCheck,
    WittyrcEffective,
    WindowConfigTemplate,
    WindowConfigDefaultPath,
    WindowConfigInit,
    WindowConfigCheck,
    WindowConfigEffective,
    ProfileStore,
}

fn mode_supports_wasm_plugin_startup(mode: &AppMode) -> bool {
    matches!(mode, AppMode::Smoke | AppMode::Window)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProfileStoreCliOptions {
    command: ProfileStoreCommand,
    store_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileStoreCommand {
    List,
    Add {
        profile_json: PathBuf,
        default_policy: ProfileStoreDefaultPolicy,
    },
    Update {
        profile_json: PathBuf,
    },
    Remove {
        id: String,
    },
    CheckLaunch {
        id: String,
    },
    SetDefault {
        id: String,
    },
    ClearDefault,
    ImportOpenSshPreview {
        config_path: PathBuf,
    },
    ImportOpenSsh {
        config_path: PathBuf,
        selection: OpenSshImportSelection,
        conflict_policy: OpenSshImportConflictPolicy,
    },
}

struct ResolvedProfileStorePath {
    path: PathBuf,
    explicit: bool,
}

fn run_profile_store_cli(options: &ProfileStoreCliOptions) -> Result<String> {
    run_profile_store_cli_with_default_path(options, default_profile_store_path)
}

fn run_profile_store_cli_with_default_path(
    options: &ProfileStoreCliOptions,
    default_profile_store: impl FnOnce() -> Result<PathBuf>,
) -> Result<String> {
    if let ProfileStoreCommand::ImportOpenSshPreview { config_path } = &options.command {
        return profile_store_import_openssh_preview_output(
            config_path,
            options.store_path.as_deref(),
        );
    }

    let resolved =
        resolve_profile_store_cli_path(options.store_path.as_deref(), default_profile_store)?;
    match &options.command {
        ProfileStoreCommand::List => profile_store_list_output(&resolved),
        ProfileStoreCommand::CheckLaunch { id } => profile_store_check_launch_output(&resolved, id),
        ProfileStoreCommand::Add {
            profile_json,
            default_policy,
        } => {
            let profile = load_profile_store_cli_ssh_profile(profile_json)?;
            let report = edit_profile_store(
                &resolved.path,
                ProfileStoreEditOpenMode::CreateIfMissing,
                |store| store.add_profile(profile, *default_policy),
            )?;
            Ok(profile_store_edit_report_output(&report))
        }
        ProfileStoreCommand::Update { profile_json } => {
            let profile = load_profile_store_cli_ssh_profile(profile_json)?;
            let profile_id = profile.id.clone();
            let report = edit_profile_store(
                &resolved.path,
                ProfileStoreEditOpenMode::Existing,
                |store| store.update_profile(&profile_id, profile),
            )?;
            Ok(profile_store_edit_report_output(&report))
        }
        ProfileStoreCommand::Remove { id } => {
            let report = edit_profile_store(
                &resolved.path,
                ProfileStoreEditOpenMode::Existing,
                |store| store.remove_profile(id),
            )?;
            Ok(profile_store_edit_report_output(&report))
        }
        ProfileStoreCommand::SetDefault { id } => {
            let report = edit_profile_store(
                &resolved.path,
                ProfileStoreEditOpenMode::Existing,
                |store| store.set_default_profile(Some(id)),
            )?;
            Ok(profile_store_edit_report_output(&report))
        }
        ProfileStoreCommand::ClearDefault => {
            let report = edit_profile_store(
                &resolved.path,
                ProfileStoreEditOpenMode::Existing,
                |store| store.set_default_profile(None),
            )?;
            Ok(profile_store_edit_report_output(&report))
        }
        ProfileStoreCommand::ImportOpenSsh {
            config_path,
            selection,
            conflict_policy,
        } => {
            profile_store_import_openssh_output(config_path, &resolved, selection, *conflict_policy)
        }
        ProfileStoreCommand::ImportOpenSshPreview { .. } => {
            unreachable!("OpenSSH import preview returns before resolving a default store path")
        }
    }
}

fn resolve_profile_store_cli_path(
    store_path: Option<&Path>,
    default_profile_store: impl FnOnce() -> Result<PathBuf>,
) -> Result<ResolvedProfileStorePath> {
    if let Some(path) = store_path {
        return Ok(ResolvedProfileStorePath {
            path: path.to_path_buf(),
            explicit: true,
        });
    }

    Ok(ResolvedProfileStorePath {
        path: default_profile_store()?,
        explicit: false,
    })
}

fn profile_store_list_output(resolved: &ResolvedProfileStorePath) -> Result<String> {
    let store = load_profile_store_for_read(resolved)?;
    format_profile_store_list(&store)
}

fn load_profile_store_for_read(resolved: &ResolvedProfileStorePath) -> Result<ProfileStoreV1> {
    if !resolved.explicit
        && !resolved
            .path
            .try_exists()
            .with_context(|| format!("check profile store {}", resolved.path.display()))?
    {
        Ok(ProfileStoreV1::new())
    } else {
        read_profile_store(&resolved.path)
    }
}

fn format_profile_store_list(store: &ProfileStoreV1) -> Result<String> {
    let mut output = String::from("default\tid\tname\tlaunchability\ttags\n");
    for profile in &store.profiles {
        let default_marker = if store.default_profile_id.as_deref() == Some(profile.id.as_str()) {
            "*"
        } else {
            ""
        };
        output.push_str(default_marker);
        output.push('\t');
        output.push_str(&profile.id);
        output.push('\t');
        output.push_str(&profile.name);
        output.push('\t');
        output.push_str(profile_launchability_label(profile)?);
        output.push('\t');
        output.push_str(&profile.tags.join(","));
        output.push('\n');
    }
    Ok(output)
}

fn profile_launchability_label(profile: &SshProfile) -> Result<&'static str> {
    match profile.launchability()? {
        SshProfileLaunchability::Launchable => Ok("launchable"),
        SshProfileLaunchability::RequiresCredentialResolver => Ok("requires-credential-resolver"),
    }
}

fn profile_store_check_launch_output(
    resolved: &ResolvedProfileStorePath,
    id: &str,
) -> Result<String> {
    let store = load_profile_store_for_read(resolved)?;
    let profile = store
        .profile(id)
        .with_context(|| format!("profile id {id} not found"))?;
    let launchability = profile.launchability()?;
    if launchability != SshProfileLaunchability::Launchable {
        bail!(
            "profile id {id} is not launchable: {}",
            profile_launchability_label(profile)?
        );
    }

    let openssh = profile.to_openssh_profile()?;
    Ok(format!(
        "profile launch check: id={} launchability={} default={} request_tty={} term={} identity_file={} config_file={} jump_host={} extra_args={} remote_command_args={}\n",
        profile.id,
        profile_launchability_label(profile)?,
        store.default_profile_id.as_deref() == Some(profile.id.as_str()),
        openssh.request_tty,
        if openssh.term.is_some() { "set" } else { "unset" },
        openssh.identity_file.is_some(),
        openssh.config_file.is_some(),
        openssh.jump_host.is_some(),
        openssh.extra_args.len(),
        openssh.remote_command.len()
    ))
}

fn profile_store_edit_report_output(report: &ProfileStoreEditReport) -> String {
    format!(
        "profile store updated: changed={} profiles={} default_changed={} bytes={} created_parent_dir={}\n",
        report.mutation.changed,
        report.mutation.profile_count,
        report.mutation.default_profile_changed,
        report.write.bytes_written,
        report.write.created_parent_dir
    )
}

fn profile_store_import_openssh_output(
    config_path: &Path,
    resolved: &ResolvedProfileStorePath,
    selection: &OpenSshImportSelection,
    conflict_policy: OpenSshImportConflictPolicy,
) -> Result<String> {
    let config = std::fs::read_to_string(config_path)
        .with_context(|| format!("read OpenSSH config {}", config_path.display()))?;
    let preview = parse_openssh_import_preview(&config, Some(config_path.to_path_buf()));
    let mut import_report = None;
    let edit_report = edit_profile_store(
        &resolved.path,
        ProfileStoreEditOpenMode::CreateIfMissing,
        |store| {
            let report = apply_openssh_import_preview(store, &preview, selection, conflict_policy)?;
            let mutation = report.mutation;
            import_report = Some(report);
            Ok(mutation)
        },
    )?;
    let import_report =
        import_report.expect("successful OpenSSH import edit should produce an import report");
    Ok(profile_store_import_openssh_apply_report_output(
        &edit_report,
        &import_report,
    ))
}

fn profile_store_import_openssh_apply_report_output(
    edit_report: &ProfileStoreEditReport,
    import_report: &OpenSshImportApplyReport,
) -> String {
    format!(
        "OpenSSH import applied: changed={} profiles={} default_changed={} bytes={} created_parent_dir={} selected={} added={} replaced={} warnings={}\n",
        edit_report.mutation.changed,
        edit_report.mutation.profile_count,
        edit_report.mutation.default_profile_changed,
        edit_report.write.bytes_written,
        edit_report.write.created_parent_dir,
        import_report.selected,
        import_report.added,
        import_report.replaced,
        import_report.total_warning_count()
    )
}

fn profile_store_import_openssh_preview_output(
    config_path: &Path,
    store_path: Option<&Path>,
) -> Result<String> {
    let config = std::fs::read_to_string(config_path)
        .with_context(|| format!("read OpenSSH config {}", config_path.display()))?;
    let mut preview = parse_openssh_import_preview(&config, Some(config_path.to_path_buf()));
    if let Some(store_path) = store_path {
        let store = read_profile_store(store_path)?;
        preview.mark_conflicts_from_store(&store);
    }
    format_openssh_import_preview(&preview)
}

fn format_openssh_import_preview(preview: &OpenSshImportPreview) -> Result<String> {
    let mut output = String::from("id\tname\tlaunchability\tconflict\twarnings\n");
    for candidate in &preview.candidates {
        output.push_str(&candidate.profile.id);
        output.push('\t');
        output.push_str(&candidate.profile.name);
        output.push('\t');
        output.push_str(profile_launchability_label(&candidate.profile)?);
        output.push('\t');
        output.push_str(openssh_import_conflict_label(candidate.conflict.as_ref()));
        output.push('\t');
        output.push_str(&candidate.warnings.len().to_string());
        output.push('\n');
    }
    output.push_str(&format!(
        "# summary candidates={} conflicts={} warnings={}\n",
        preview.candidates.len(),
        preview.conflict_count(),
        preview.total_warning_count()
    ));
    Ok(output)
}

fn openssh_import_conflict_label(conflict: Option<&OpenSshImportConflict>) -> &'static str {
    match conflict {
        Some(OpenSshImportConflict::ExistingProfileId { .. }) => "existing-profile-id",
        None => "none",
    }
}

fn load_profile_store_cli_ssh_profile(path: &Path) -> Result<SshProfile> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("read SSH profile JSON {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("parse SSH profile JSON {}", path.display()))
}

fn discover_wasm_plugins(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let dir = dir.as_ref();
    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read plugin dir {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "wasm")
        {
            plugins.push(path);
        }
    }
    plugins.sort();
    Ok(plugins)
}

fn install_wasm_plugins<T>(app: &mut TerminalApp<T>, plugin_paths: &[PathBuf]) -> Result<()>
where
    T: TerminalTransport,
{
    if plugin_paths.is_empty() {
        return Ok(());
    }

    let runtime = WasmPluginRuntime::new()?;
    for path in plugin_paths {
        app.install_wasm_plugin_from_file(&runtime, path)
            .with_context(|| format!("failed to install Wasm plugin {}", path.display()))?;
    }

    Ok(())
}

struct BuiltInCommandsPlugin;

impl BuiltInPlugin for BuiltInCommandsPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "builtin".to_owned(),
            name: "Built-in Commands".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            runtime: PluginRuntime::BuiltIn,
            permissions: PluginPermissions::default(),
        }
    }

    fn commands(&self) -> Vec<CommandRegistration> {
        vec![CommandRegistration {
            id: "witty.about".to_owned(),
            title: "About Witty".to_owned(),
            source_plugin: "builtin".to_owned(),
        }]
    }

    fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
        let PluginEvent::CommandInvoked(invocation) = event else {
            return Ok(Vec::new());
        };
        if invocation.command_id != "witty.about" {
            return Ok(Vec::new());
        }

        Ok(vec![PluginAction::ShowMessage {
            message: "Witty Rust/wgpu prototype".to_owned(),
        }])
    }
}

fn run_pty_smoke() -> anyhow::Result<()> {
    let size = GridSize::new(24, 80);
    #[cfg(unix)]
    let resized_size = GridSize::new(30, 100);
    let mut transport = LocalPtyTransport::spawn(pty_smoke_config(size))?;
    let mut terminal = BasicTerminal::new(size);
    let mut output = Vec::new();
    let mut host_reply_bytes = 0;
    let mut exit_code = None;
    let mut exit_seen_at = None;
    let deadline = Instant::now() + Duration::from_secs(5);

    #[cfg(unix)]
    {
        transport.resize(resized_size)?;
        transport.write(
            b"printf 'Witty PTY interactive smoke\\r\\n'; printf 'TERM=%s COLORTERM=%s\\r\\n' \"$TERM\" \"$COLORTERM\"; stty size; exit 0\r",
        )?;
    }

    while Instant::now() < deadline {
        while let Some(event) = transport.poll_event()? {
            match event {
                TransportEvent::Output(bytes) => {
                    output.extend_from_slice(&bytes);
                    terminal.feed(&bytes);
                    host_reply_bytes +=
                        apply_pty_smoke_host_actions(&mut terminal, &mut transport)?;
                }
                TransportEvent::Exit { code } => {
                    if exit_seen_at.is_none() {
                        exit_seen_at = Some(Instant::now());
                    }
                    exit_code = code;
                }
                TransportEvent::Error(err) => bail!("pty smoke error: {err}"),
            }
        }

        if exit_seen_at.is_some_and(|seen_at| seen_at.elapsed() >= Duration::from_millis(100)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let Some(code) = exit_code else {
        bail!(
            "pty smoke timed out; output_tail={:?}",
            pty_smoke_output_tail(&output)
        );
    };
    if code != 0 {
        bail!("pty smoke exited with code {code}");
    }

    let output_text = String::from_utf8_lossy(&output);
    #[cfg(unix)]
    {
        if !output_text.contains("Witty PTY interactive smoke") {
            bail!("pty smoke did not echo interactive command output");
        }
        if !output_text.contains("TERM=xterm-256color COLORTERM=truecolor") {
            bail!("pty smoke did not observe default terminal environment");
        }
        let expected_size = format!("{} {}", resized_size.rows, resized_size.cols);
        if !output_text.contains(&expected_size) {
            bail!("pty smoke resize not observed by shell; expected stty size {expected_size}");
        }
    }
    #[cfg(windows)]
    {
        if !output_text.contains("Witty PTY smoke") {
            bail!("pty smoke did not capture command output");
        }
    }

    let planner = FramePlanner::new(CellMetrics::default());
    let snapshot = terminal.take_snapshot();
    let frame = planner.plan(&snapshot);
    println!(
        "PTY smoke exit={code}; bytes={}; host_reply_bytes={host_reply_bytes}; {} glyphs planned",
        output.len(),
        frame.glyphs.len()
    );

    Ok(())
}

fn apply_pty_smoke_host_actions<T: TerminalTransport>(
    terminal: &mut BasicTerminal,
    transport: &mut T,
) -> Result<usize> {
    let mut reply_bytes = 0;
    for action in terminal.drain_host_actions() {
        if let TerminalHostAction::TerminalReply(reply) = action {
            reply_bytes += reply.bytes.len();
            transport.write(&reply.bytes)?;
        }
    }
    Ok(reply_bytes)
}

fn pty_smoke_output_tail(output: &[u8]) -> String {
    let start = output.len().saturating_sub(512);
    String::from_utf8_lossy(&output[start..]).into_owned()
}

fn run_incremental_smoke() -> anyhow::Result<()> {
    let stats = incremental_smoke_stats()?;
    println!(
        "Incremental smoke frames={}; first rebuilt={}; second reused={}/rebuilt={}; third reused={}/rebuilt={}",
        stats.len(),
        stats[0].rebuilt_rows,
        stats[1].reused_rows,
        stats[1].rebuilt_rows,
        stats[2].reused_rows,
        stats[2].rebuilt_rows
    );
    println!(
        "Incremental smoke stats={}",
        incremental_smoke_stats_json(&stats)?
    );
    Ok(())
}

fn run_selection_copy_smoke() -> anyhow::Result<()> {
    let copied = window::selection_copy_regression_smoke()?;
    println!(
        "Selection copy smoke copied {} bytes: {:?}",
        copied.len(),
        copied
    );
    Ok(())
}

fn run_primary_selection_smoke() -> anyhow::Result<()> {
    let copied = window::primary_selection_boundary_smoke()?;
    println!(
        "Primary selection smoke copied {} bytes: {:?}",
        copied.len(),
        copied
    );
    Ok(())
}

fn run_primary_selection_gui_smoke() -> anyhow::Result<()> {
    let smoke = window::primary_selection_gui_smoke()?;
    println!(
        "Primary selection GUI smoke copied {} bytes: {:?}; pasted {} bytes",
        smoke.copied.len(),
        smoke.copied,
        smoke.pasted.len()
    );
    Ok(())
}

fn run_native_search_smoke() -> anyhow::Result<()> {
    let smoke = window::native_search_smoke()?;
    println!(
        "Native search smoke query={:?} matches={} active={:?} visible_highlights={} active_visible={} status={}",
        smoke.query,
        smoke.match_count,
        smoke.active_index,
        smoke.visible_highlights,
        smoke.active_visible,
        smoke.status
    );
    Ok(())
}

fn run_native_command_block_smoke() -> anyhow::Result<()> {
    let smoke = window::native_command_block_smoke()?;
    println!(
        "Native command block smoke completed={} selected={:?} overlay_rects={} frame_backgrounds={} folded_hidden_rows={} folded_second_compact_row={:?} folded_gutter_selected_id={:?}",
        smoke.completed_blocks,
        smoke.selected_id,
        smoke.overlay_rects,
        smoke.frame_backgrounds,
        smoke.folded_hidden_rows,
        smoke.folded_second_compact_row,
        smoke.folded_gutter_selected_id
    );
    Ok(())
}

fn run_openssh_profile_smoke() -> anyhow::Result<()> {
    let smoke = run_openssh_config_dump_smoke()?;
    println!(
        "OpenSSH profile smoke destination={} exit={:?} output_bytes={}",
        smoke.destination,
        smoke.exit_code,
        smoke.output.len()
    );
    Ok(())
}

fn run_renderer_backend_info() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&renderer_backend_info_json())?
    );
    Ok(())
}

fn renderer_backend_info_json() -> serde_json::Value {
    let policy = native_wgpu_backend_policy();
    serde_json::json!({
        "renderer": "wgpu",
        "native_backend_policy": policy.label(),
        "opengl_only": policy.is_opengl_only(),
        "honors_wgpu_backend_env": policy.honors_wgpu_backend_env(),
        "opens_window": false,
        "enumerates_adapter": false,
    })
}

fn run_renderer_no_surface_diagnostics() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&renderer_no_surface_diagnostics_json())?
    );
    Ok(())
}

fn run_font_list(filter: Option<&str>) -> anyhow::Result<()> {
    print!("{}", font_list_text(available_font_families(), filter));
    Ok(())
}

fn font_list_text(families: impl IntoIterator<Item = String>, filter: Option<&str>) -> String {
    let filter = filter.map(|value| value.to_lowercase());
    let families = families
        .into_iter()
        .filter(|family| {
            filter
                .as_ref()
                .is_none_or(|needle| family.to_lowercase().contains(needle))
        })
        .collect::<BTreeSet<_>>();
    if families.is_empty() {
        return String::new();
    }
    let mut text = families.into_iter().collect::<Vec<_>>().join("\n");
    text.push('\n');
    text
}

fn run_keyboard_protocol_diagnostics() -> anyhow::Result<()> {
    print!("{}", keyboard_protocol_diagnostics_text()?);
    Ok(())
}

fn keyboard_protocol_diagnostics_text() -> serde_json::Result<String> {
    let mut text = serde_json::to_string_pretty(&keyboard_protocol_diagnostics_json())?;
    text.push('\n');
    Ok(text)
}

fn keyboard_protocol_diagnostics_json() -> serde_json::Value {
    let cases = keyboard_protocol_diagnostic_specs()
        .into_iter()
        .map(keyboard_protocol_diagnostic_case_json)
        .collect::<Vec<_>>();

    serde_json::json!({
        "diagnostic": "keyboard-protocol",
        "version": env!("CARGO_PKG_VERSION"),
        "opensWindow": false,
        "startsPty": false,
        "cases": cases,
    })
}

#[derive(Clone, Copy)]
struct KeyboardProtocolDiagnosticSpec {
    id: &'static str,
    description: &'static str,
    flags: u16,
    key: TerminalKey<'static>,
    text: Option<&'static str>,
    modifiers: TerminalKeyModifiers,
    keypad_key: Option<TerminalKeypadKey>,
    base_layout_key: Option<char>,
    modifier_key: Option<TerminalModifierKey>,
    event_type: TerminalKeyEventType,
}

fn keyboard_protocol_diagnostic_specs() -> Vec<KeyboardProtocolDiagnosticSpec> {
    let no_modifiers = TerminalKeyModifiers::default();
    let control = TerminalKeyModifiers {
        control: true,
        ..TerminalKeyModifiers::default()
    };
    let shift = TerminalKeyModifiers {
        shift: true,
        ..TerminalKeyModifiers::default()
    };
    vec![
        KeyboardProtocolDiagnosticSpec {
            id: "legacy-ctrl-i",
            description: "Legacy Ctrl-I remains indistinguishable from Tab.",
            flags: 0,
            key: TerminalKey::Character("i"),
            text: Some("i"),
            modifiers: control,
            keypad_key: None,
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-disambiguate-ctrl-i",
            description: "Kitty flag 1 disambiguates Ctrl-I from Tab.",
            flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            key: TerminalKey::Character("i"),
            text: Some("i"),
            modifiers: control,
            keypad_key: None,
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-event-ctrl-i",
            description: "Kitty flags 1|2 include a press event type for Ctrl-I.",
            flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            key: TerminalKey::Character("i"),
            text: Some("i"),
            modifiers: control,
            keypad_key: None,
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-all-ctrl-enter",
            description: "Kitty flag 8 reports Ctrl-Enter as CSI-u.",
            flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
            key: TerminalKey::Named(TerminalNamedKey::Enter),
            text: None,
            modifiers: control,
            keypad_key: None,
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-associated-shift-a-repeat",
            description: "Kitty flags 8|16|4|2 combine associated text, alternate keys, and repeat event type.",
            flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
                | KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            key: TerminalKey::Character("A"),
            text: Some("A"),
            modifiers: shift,
            keypad_key: None,
            base_layout_key: Some('a'),
            modifier_key: None,
            event_type: TerminalKeyEventType::Repeat,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-keypad-1",
            description: "Kitty flag 1 distinguishes keypad 1 from top-row 1.",
            flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            key: TerminalKey::Character("1"),
            text: Some("1"),
            modifiers: no_modifiers,
            keypad_key: Some(TerminalKeypadKey::Digit(1)),
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-keypad-left-numlock-off",
            description: "Kitty flag 1 reports NumLock-off keypad navigation separately.",
            flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            key: TerminalKey::Named(TerminalNamedKey::ArrowLeft),
            text: None,
            modifiers: no_modifiers,
            keypad_key: Some(TerminalKeypadKey::Left),
            base_layout_key: None,
            modifier_key: None,
            event_type: TerminalKeyEventType::Press,
        },
        KeyboardProtocolDiagnosticSpec {
            id: "kitty-right-ctrl-release",
            description: "Kitty flags 8|2 report sided modifier release events.",
            flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            key: TerminalKey::Unidentified,
            text: None,
            modifiers: no_modifiers,
            keypad_key: None,
            base_layout_key: None,
            modifier_key: Some(TerminalModifierKey::RightControl),
            event_type: TerminalKeyEventType::Release,
        },
    ]
}

fn keyboard_protocol_diagnostic_case_json(
    spec: KeyboardProtocolDiagnosticSpec,
) -> serde_json::Value {
    let modes = TerminalInputModes {
        kitty_keyboard_flags: spec.flags,
        ..TerminalInputModes::default()
    };
    let bytes = encode_terminal_key_input(
        TerminalKeyInput {
            key: spec.key,
            text: spec.text,
            modifiers: spec.modifiers,
            keypad_key: spec.keypad_key,
            base_layout_key: spec.base_layout_key,
            modifier_key: spec.modifier_key,
            event_type: spec.event_type,
        },
        modes,
    );

    serde_json::json!({
        "id": spec.id,
        "description": spec.description,
        "flags": spec.flags,
        "flagNames": keyboard_protocol_flag_names(spec.flags),
        "key": keyboard_protocol_key_label(spec.key),
        "text": spec.text,
        "modifiers": keyboard_protocol_modifier_names(spec.modifiers),
        "keypadKey": spec.keypad_key.map(keyboard_protocol_keypad_label),
        "baseLayoutKey": spec.base_layout_key.map(|ch| ch.to_string()),
        "modifierKey": spec.modifier_key.map(keyboard_protocol_modifier_key_label),
        "eventType": keyboard_protocol_event_type_label(spec.event_type),
        "suppressed": bytes.is_none(),
        "bytesHex": bytes.as_deref().map(bytes_hex),
        "bytesEscaped": bytes.as_deref().map(escaped_bytes),
    })
}

fn keyboard_protocol_flag_names(flags: u16) -> Vec<&'static str> {
    [
        (
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            "DISAMBIGUATE_ESC_CODES",
        ),
        (KITTY_KEYBOARD_REPORT_EVENT_TYPES, "REPORT_EVENT_TYPES"),
        (
            KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS,
            "REPORT_ALTERNATE_KEYS",
        ),
        (
            KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
            "REPORT_ALL_KEYS_AS_ESC_CODES",
        ),
        (
            KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT,
            "REPORT_ASSOCIATED_TEXT",
        ),
    ]
    .into_iter()
    .filter_map(|(flag, name)| (flags & flag != 0).then_some(name))
    .collect()
}

fn keyboard_protocol_key_label(key: TerminalKey<'_>) -> String {
    match key {
        TerminalKey::Named(key) => format!("{key:?}"),
        TerminalKey::Character(value) => format!("Character({value})"),
        TerminalKey::Unidentified => "Unidentified".to_owned(),
    }
}

fn keyboard_protocol_modifier_names(modifiers: TerminalKeyModifiers) -> Vec<&'static str> {
    let mut names = Vec::new();
    if modifiers.control {
        names.push("Control");
    }
    if modifiers.shift {
        names.push("Shift");
    }
    if modifiers.alt {
        names.push("Alt");
    }
    if modifiers.meta {
        names.push("Super");
    }
    if modifiers.hyper {
        names.push("Hyper");
    }
    if modifiers.kitty_meta {
        names.push("Meta");
    }
    names
}

fn keyboard_protocol_keypad_label(keypad_key: TerminalKeypadKey) -> &'static str {
    match keypad_key {
        TerminalKeypadKey::Digit(0) => "KP_0",
        TerminalKeypadKey::Digit(1) => "KP_1",
        TerminalKeypadKey::Digit(2) => "KP_2",
        TerminalKeypadKey::Digit(3) => "KP_3",
        TerminalKeypadKey::Digit(4) => "KP_4",
        TerminalKeypadKey::Digit(5) => "KP_5",
        TerminalKeypadKey::Digit(6) => "KP_6",
        TerminalKeypadKey::Digit(7) => "KP_7",
        TerminalKeypadKey::Digit(8) => "KP_8",
        TerminalKeypadKey::Digit(9) => "KP_9",
        TerminalKeypadKey::Digit(_) => "KP_DIGIT_INVALID",
        TerminalKeypadKey::Decimal => "KP_DECIMAL",
        TerminalKeypadKey::Comma => "KP_COMMA",
        TerminalKeypadKey::Add => "KP_ADD",
        TerminalKeypadKey::Subtract => "KP_SUBTRACT",
        TerminalKeypadKey::Multiply => "KP_MULTIPLY",
        TerminalKeypadKey::Divide => "KP_DIVIDE",
        TerminalKeypadKey::Enter => "KP_ENTER",
        TerminalKeypadKey::Equal => "KP_EQUAL",
        TerminalKeypadKey::Left => "KP_LEFT",
        TerminalKeypadKey::Right => "KP_RIGHT",
        TerminalKeypadKey::Up => "KP_UP",
        TerminalKeypadKey::Down => "KP_DOWN",
        TerminalKeypadKey::PageUp => "KP_PAGE_UP",
        TerminalKeypadKey::PageDown => "KP_PAGE_DOWN",
        TerminalKeypadKey::Home => "KP_HOME",
        TerminalKeypadKey::End => "KP_END",
        TerminalKeypadKey::Insert => "KP_INSERT",
        TerminalKeypadKey::Delete => "KP_DELETE",
        TerminalKeypadKey::Begin => "KP_BEGIN",
    }
}

fn keyboard_protocol_modifier_key_label(modifier_key: TerminalModifierKey) -> &'static str {
    match modifier_key {
        TerminalModifierKey::LeftShift => "LEFT_SHIFT",
        TerminalModifierKey::RightShift => "RIGHT_SHIFT",
        TerminalModifierKey::LeftAlt => "LEFT_ALT",
        TerminalModifierKey::RightAlt => "RIGHT_ALT",
        TerminalModifierKey::LeftControl => "LEFT_CONTROL",
        TerminalModifierKey::RightControl => "RIGHT_CONTROL",
        TerminalModifierKey::LeftSuper => "LEFT_SUPER",
        TerminalModifierKey::RightSuper => "RIGHT_SUPER",
        TerminalModifierKey::LeftHyper => "LEFT_HYPER",
        TerminalModifierKey::RightHyper => "RIGHT_HYPER",
        TerminalModifierKey::LeftMeta => "LEFT_META",
        TerminalModifierKey::RightMeta => "RIGHT_META",
    }
}

fn keyboard_protocol_event_type_label(event_type: TerminalKeyEventType) -> &'static str {
    match event_type {
        TerminalKeyEventType::Press => "press",
        TerminalKeyEventType::Repeat => "repeat",
        TerminalKeyEventType::Release => "release",
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escaped_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            b'\x1b' => "\\x1b".to_owned(),
            b'\r' => "\\r".to_owned(),
            b'\n' => "\\n".to_owned(),
            b'\t' => "\\t".to_owned(),
            0x20..=0x7e => char::from(*byte).to_string(),
            _ => format!("\\x{byte:02x}"),
        })
        .collect()
}

fn run_keyboard_protocol_capture() -> anyhow::Result<()> {
    if !io::stdin().is_terminal() {
        bail!("--keyboard-protocol-capture requires stdin to be a terminal");
    }

    let mut stdout = io::stdout();
    writeln!(stdout, "Witty keyboard protocol capture")?;
    writeln!(stdout, "Press keys to print the bytes sent by this terminal.")?;
    writeln!(stdout, "Press Ctrl-C to exit.")?;
    stdout.flush()?;

    let _raw_mode = SttyRawMode::enter()?;
    let mut stdin = io::stdin();
    let mut index = 1usize;
    loop {
        let bytes = read_keyboard_protocol_capture_event(&mut stdin)?;
        if bytes.contains(&0x03) {
            break;
        }
        writeln!(
            stdout,
            "{}",
            keyboard_protocol_capture_event_line(index, &bytes)
        )?;
        stdout.flush()?;
        index += 1;
    }

    writeln!(stdout, "\nkeyboard protocol capture ended")?;
    Ok(())
}

struct SttyRawMode {
    saved_state: String,
}

impl SttyRawMode {
    fn enter() -> anyhow::Result<Self> {
        let saved_state = stty_saved_state()?;
        run_stty(["raw", "-echo", "min", "0", "time", "1"])?;
        Ok(Self { saved_state })
    }
}

impl Drop for SttyRawMode {
    fn drop(&mut self) {
        if !self.saved_state.is_empty() {
            let _ = run_stty([self.saved_state.as_str()]);
        }
    }
}

fn stty_saved_state() -> anyhow::Result<String> {
    let output = std::process::Command::new("stty")
        .arg("-g")
        .stdin(std::process::Stdio::inherit())
        .output()
        .context("run stty -g")?;
    if !output.status.success() {
        bail!(
            "stty -g failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_stty<I, S>(args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new("stty")
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .output()
        .context("run stty")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "stty failed with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn read_keyboard_protocol_capture_event(reader: &mut impl io::Read) -> io::Result<Vec<u8>> {
    let mut event = Vec::new();
    let mut buf = [0u8; 64];
    let mut empty_reads_after_event = 0usize;
    loop {
        match reader.read(&mut buf) {
            Ok(0) if event.is_empty() => {}
            Ok(0) => {
                empty_reads_after_event += 1;
                if empty_reads_after_event >= 2 {
                    return Ok(event);
                }
            }
            Ok(n) => {
                event.extend_from_slice(&buf[..n]);
                empty_reads_after_event = 0;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

fn keyboard_protocol_capture_event_line(index: usize, bytes: &[u8]) -> String {
    format!(
        "event {index}: bytesHex={} bytesEscaped={}",
        bytes_hex(bytes),
        escaped_bytes(bytes)
    )
}

fn renderer_no_surface_diagnostics_json() -> serde_json::Value {
    let size = GridSize::new(4, 16);
    let mut terminal = BasicTerminal::new(size);
    let mut planner = RetainedFramePlanner::new(CellMetrics::default());

    let empty = planner.plan(&terminal.take_snapshot());
    terminal.feed(b"alpha \x1b[31mred\x1b[0m\r\nbeta wide \xe4\xb8\xad");
    terminal.set_selection(Some(CellRange {
        start: CellPoint::new(0, 0),
        end: CellPoint::new(1, 4),
    }));
    let populated = planner.plan(&terminal.take_snapshot());

    serde_json::json!({
        "diagnostic": "renderer-no-surface",
        "renderer": "wgpu",
        "nativeBackend": renderer_backend_info_json(),
        "opensWindow": false,
        "createsSurface": false,
        "requestsAdapter": false,
        "createsDevice": false,
        "frameCount": 2,
        "frames": [
            frame_stats_json_value("empty", empty.stats),
            frame_stats_json_value("populated", populated.stats),
        ],
    })
}

fn incremental_smoke_stats() -> anyhow::Result<Vec<FrameStats>> {
    let size = GridSize::new(3, 8);
    let mut terminal = BasicTerminal::new(size);
    let mut planner = RetainedFramePlanner::new(CellMetrics::default());

    let first = planner.plan(&terminal.take_snapshot());
    ensure_frame_stats("first", first.stats, 0, 3)?;

    terminal.feed(b"hi");
    let second = planner.plan(&terminal.take_snapshot());
    ensure_frame_stats("second", second.stats, 2, 1)?;

    terminal.feed(b"\r\nok");
    let third = planner.plan(&terminal.take_snapshot());
    ensure_frame_stats("third", third.stats, 1, 2)?;

    Ok(vec![first.stats, second.stats, third.stats])
}

fn incremental_smoke_stats_json(stats: &[FrameStats]) -> serde_json::Result<String> {
    let frames = stats
        .iter()
        .enumerate()
        .map(|(index, stats)| frame_stats_json_value(incremental_smoke_frame_label(index), *stats))
        .collect::<Vec<_>>();

    serde_json::to_string(&serde_json::json!({
        "smoke": "incremental",
        "frameCount": stats.len(),
        "frames": frames,
    }))
}

fn incremental_smoke_frame_label(index: usize) -> &'static str {
    match index {
        0 => "first",
        1 => "second",
        2 => "third",
        _ => "frame",
    }
}

fn frame_stats_json_value(label: &str, stats: FrameStats) -> serde_json::Value {
    serde_json::json!({
        "label": label,
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

fn ensure_frame_stats(
    label: &str,
    stats: FrameStats,
    expected_reused_rows: usize,
    expected_rebuilt_rows: usize,
) -> anyhow::Result<()> {
    if stats.reused_rows != expected_reused_rows || stats.rebuilt_rows != expected_rebuilt_rows {
        bail!(
            "{label} frame row reuse mismatch: reused={} rebuilt={} expected reused={} rebuilt={}",
            stats.reused_rows,
            stats.rebuilt_rows,
            expected_reused_rows,
            expected_rebuilt_rows
        );
    }
    Ok(())
}

#[cfg(unix)]
fn pty_smoke_config(size: GridSize) -> LocalPtyConfig {
    LocalPtyConfig::new(size)
}

#[cfg(windows)]
fn pty_smoke_config(size: GridSize) -> LocalPtyConfig {
    let mut config = LocalPtyConfig::command(size, "cmd.exe");
    config.args(["/C", "echo Witty PTY smoke"]);
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_options_parse_with_env(
        args: impl IntoIterator<Item = &'static str>,
        env_values: Vec<(&'static str, std::ffi::OsString)>,
    ) -> Result<AppOptions> {
        AppOptions::parse_with_env(args.into_iter().map(str::to_owned), |name| {
            env_values
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| value.clone()))
        })
    }

    #[test]
    fn app_options_parse_explicit_wasm_plugins() {
        let options = AppOptions::parse([
            "--wasm-plugin".to_owned(),
            "one.wasm".to_owned(),
            "--wasm-plugin".to_owned(),
            "two.wasm".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Smoke);
        assert_eq!(options.window_smoke, WindowSmokeOptions::default());
        assert_eq!(
            options.mouse_selection_override,
            MouseSelectionOverridePolicy::ShiftSelect
        );
        assert_eq!(options.max_scrollback_lines, DEFAULT_MAX_SCROLLBACK_LINES);
        assert_eq!(options.font_family, None);
        assert_eq!(options.font_size, None);
        assert_eq!(options.terminal_padding, None);
        assert_eq!(options.background_opacity, None);
        assert_eq!(options.background_image, None);
        assert_eq!(options.background_image_fit, None);
        assert_eq!(options.background_overlay_color, None);
        assert_eq!(options.background_overlay_opacity, None);
        assert!(options.font_paths.is_empty());
        assert!(options.launcher_args.is_empty());
        assert_eq!(
            options.wasm_plugins,
            vec![PathBuf::from("one.wasm"), PathBuf::from("two.wasm")]
        );
    }

    #[test]
    fn wasm_plugin_startup_is_limited_to_smoke_and_window_modes() {
        assert!(mode_supports_wasm_plugin_startup(&AppMode::Smoke));
        assert!(mode_supports_wasm_plugin_startup(&AppMode::Window));

        for mode in [
            AppMode::Web,
            AppMode::PtySmoke,
            AppMode::IncrementalSmoke,
            AppMode::SelectionCopySmoke,
            AppMode::PrimarySelectionSmoke,
            AppMode::PrimarySelectionGuiSmoke,
            AppMode::NativeSearchSmoke,
            AppMode::NativeCommandBlockSmoke,
            AppMode::OpenSshProfileSmoke,
            AppMode::RealTuiSmoke,
            AppMode::RendererBackendInfo,
            AppMode::RendererNoSurfaceDiagnostics,
            AppMode::KeyboardProtocolDiagnostics,
            AppMode::KeyboardProtocolCapture,
            AppMode::FontList,
            AppMode::WittyrcTemplate,
            AppMode::WittyrcDefaultPath,
            AppMode::WittyrcInit,
            AppMode::WittyrcCheck,
            AppMode::WittyrcEffective,
            AppMode::WindowConfigTemplate,
            AppMode::WindowConfigDefaultPath,
            AppMode::WindowConfigInit,
            AppMode::WindowConfigCheck,
            AppMode::WindowConfigEffective,
            AppMode::ProfileStore,
        ] {
            assert!(!mode_supports_wasm_plugin_startup(&mode));
        }
    }

    #[test]
    fn app_options_parse_window_wasm_plugin_startup() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--wasm-plugin".to_owned(),
            "plugin.wasm".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(options.wasm_plugins, vec![PathBuf::from("plugin.wasm")]);
    }

    #[test]
    fn app_options_default_osc52_clipboard_policy_is_disabled() {
        let options = AppOptions::parse(Vec::<String>::new()).unwrap();

        assert_eq!(
            options.osc52_clipboard_policy,
            Osc52ClipboardPolicy::Disabled
        );
    }

    #[test]
    fn app_options_parse_modes() {
        assert_eq!(
            AppOptions::parse(["--web".to_owned()]).unwrap().mode,
            AppMode::Web
        );
        assert_eq!(
            AppOptions::parse(["--window".to_owned()]).unwrap().mode,
            AppMode::Window
        );
        assert_eq!(
            AppOptions::parse(["--pty-smoke".to_owned()]).unwrap().mode,
            AppMode::PtySmoke
        );
        assert_eq!(
            AppOptions::parse(["--incremental-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::IncrementalSmoke
        );
        assert_eq!(
            AppOptions::parse(["--selection-copy-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::SelectionCopySmoke
        );
        assert_eq!(
            AppOptions::parse(["--primary-selection-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::PrimarySelectionSmoke
        );
        assert_eq!(
            AppOptions::parse(["--primary-selection-gui-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::PrimarySelectionGuiSmoke
        );
        assert_eq!(
            AppOptions::parse(["--native-search-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::NativeSearchSmoke
        );
        assert_eq!(
            AppOptions::parse(["--native-command-block-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::NativeCommandBlockSmoke
        );
        assert_eq!(
            AppOptions::parse(["--openssh-profile-smoke".to_owned()])
                .unwrap()
                .mode,
            AppMode::OpenSshProfileSmoke
        );
        let real_tui = AppOptions::parse([
            "--real-tui-smoke".to_owned(),
            "less-basic-restore".to_owned(),
        ])
        .unwrap();
        assert_eq!(real_tui.mode, AppMode::RealTuiSmoke);
        assert_eq!(
            real_tui.real_tui_smoke_case.as_deref(),
            Some("less-basic-restore")
        );
        assert_eq!(
            AppOptions::parse(["--renderer-backend-info".to_owned()])
                .unwrap()
                .mode,
            AppMode::RendererBackendInfo
        );
        assert_eq!(
            AppOptions::parse(["--renderer-no-surface-diagnostics".to_owned()])
                .unwrap()
                .mode,
            AppMode::RendererNoSurfaceDiagnostics
        );
        assert_eq!(
            AppOptions::parse(["--keyboard-protocol-diagnostics".to_owned()])
                .unwrap()
                .mode,
            AppMode::KeyboardProtocolDiagnostics
        );
        assert_eq!(
            AppOptions::parse(["--keyboard-protocol-capture".to_owned()])
                .unwrap()
                .mode,
            AppMode::KeyboardProtocolCapture
        );
        assert_eq!(
            AppOptions::parse(["--font-list".to_owned()]).unwrap().mode,
            AppMode::FontList
        );
        assert_eq!(
            AppOptions::parse(["--wittyrc-template".to_owned()])
                .unwrap()
                .mode,
            AppMode::WittyrcTemplate
        );
        assert_eq!(
            AppOptions::parse(["--wittyrc-default-path".to_owned()])
                .unwrap()
                .mode,
            AppMode::WittyrcDefaultPath
        );
        assert_eq!(
            AppOptions::parse(["--wittyrc-init".to_owned()])
                .unwrap()
                .mode,
            AppMode::WittyrcInit
        );
        assert_eq!(
            AppOptions::parse(["--wittyrc-check".to_owned()])
                .unwrap()
                .mode,
            AppMode::WittyrcCheck
        );
        assert_eq!(
            AppOptions::parse(["--wittyrc-effective".to_owned()])
                .unwrap()
                .mode,
            AppMode::WittyrcEffective
        );
        assert_eq!(
            AppOptions::parse(["--window-config-template".to_owned()])
                .unwrap()
                .mode,
            AppMode::WindowConfigTemplate
        );
        assert_eq!(
            AppOptions::parse(["--window-config-default-path".to_owned()])
                .unwrap()
                .mode,
            AppMode::WindowConfigDefaultPath
        );
        assert_eq!(
            AppOptions::parse(["--window-config-init".to_owned()])
                .unwrap()
                .mode,
            AppMode::WindowConfigInit
        );
        assert_eq!(
            AppOptions::parse(["--window-config-check".to_owned()])
                .unwrap()
                .mode,
            AppMode::WindowConfigCheck
        );
        assert_eq!(
            AppOptions::parse(["--window-config-effective".to_owned()])
                .unwrap()
                .mode,
            AppMode::WindowConfigEffective
        );
    }

    #[test]
    fn app_options_parse_window_defaults_last_active_close_to_close_window() {
        let options = AppOptions::parse(["--window".to_owned()]).unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::CloseWindow
        );
    }

    #[test]
    fn app_options_parse_window_restore_state_path() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--restore-state".to_owned(),
            "/tmp/restart-state.v1.42.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(
            options.restore_state_path,
            Some(PathBuf::from("/tmp/restart-state.v1.42.json"))
        );
        assert!(AppOptions::parse([
            "--restore-state".to_owned(),
            "/tmp/restart-state.v1.42.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--restore-state".to_owned(),
            " ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--restore-state".to_owned(),
            "/tmp/a.json".to_owned(),
            "--restore-state".to_owned(),
            "/tmp/b.json".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn app_options_parse_font_list_filter() {
        let options = AppOptions::parse([
            "--font-list".to_owned(),
            "--font-list-filter".to_owned(),
            "  nerd  ".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::FontList);
        assert_eq!(options.font_list_filter.as_deref(), Some("nerd"));
        assert!(AppOptions::parse(["--font-list-filter".to_owned(), "nerd".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--font-list".to_owned(),
            "--font-list-filter".to_owned(),
            "  ".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--font-list".to_owned(),
            "--font-list-filter".to_owned(),
            "nerd".to_owned(),
            "--window".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn app_options_parse_window_smoke_flags() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--window-command-palette".to_owned(),
            "--window-diagnostics".to_owned(),
            "--window-startup-report".to_owned(),
            "--window-exit-after-ms".to_owned(),
            "1500".to_owned(),
            "--window-last-active-close".to_owned(),
            "close-window".to_owned(),
            "--window-title".to_owned(),
            "  Project Shell  ".to_owned(),
            "--window-cols".to_owned(),
            "120".to_owned(),
            "--window-rows".to_owned(),
            "36".to_owned(),
            "--mouse-selection-override".to_owned(),
            "disabled".to_owned(),
            "--scrollback-lines".to_owned(),
            "2500".to_owned(),
            "--font-family".to_owned(),
            "JetBrainsMono Nerd Font".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned(),
            "--terminal-padding".to_owned(),
            "8".to_owned(),
            "--background-opacity".to_owned(),
            "0.82".to_owned(),
            "--background-image".to_owned(),
            "~/Pictures/witty.png".to_owned(),
            "--background-image-fit".to_owned(),
            "scale-crop".to_owned(),
            "--background-overlay-color".to_owned(),
            "#0a141e".to_owned(),
            "--background-overlay-opacity".to_owned(),
            "0.24".to_owned(),
            "--cursor-shape".to_owned(),
            "bar".to_owned(),
            "--cursor-blink".to_owned(),
            "false".to_owned(),
            "--cursor-blink-rate".to_owned(),
            "slow".to_owned(),
            "--cursor-style-source".to_owned(),
            "config".to_owned(),
            "--session-tab-position".to_owned(),
            "top".to_owned(),
            "--session-tab-label".to_owned(),
            "index-profile".to_owned(),
            "--session-tab-show-single".to_owned(),
            "yes".to_owned(),
            "--session-tab-show-multiple".to_owned(),
            "true".to_owned(),
            "--font-path".to_owned(),
            "/fonts/JetBrainsMonoNerdFont-Regular.ttf".to_owned(),
            "--font-path".to_owned(),
            "/fonts/SymbolsNerdFontMono-Regular.ttf".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(
            options.mouse_selection_override,
            MouseSelectionOverridePolicy::Disabled
        );
        assert_eq!(options.window_title.as_deref(), Some("Project Shell"));
        assert_eq!(options.max_scrollback_lines, 2500);
        assert_eq!(
            options.font_family.as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(options.font_size, Some(16));
        assert_eq!(options.terminal_padding, Some(8));
        assert_eq!(options.background_opacity, Some(0.82));
        assert_eq!(
            options.background_image,
            Some(
                PathBuf::from(
                    env::var_os("HOME").expect("HOME should be set for background image test")
                )
                .join("Pictures/witty.png")
            )
        );
        assert_eq!(
            options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
        assert_eq!(
            options.background_overlay_color,
            Some(Rgba::rgb(0x0a, 0x14, 0x1e))
        );
        assert_eq!(options.background_overlay_opacity, Some(0.24));
        assert_eq!(options.cursor_shape, CursorShape::Bar);
        assert!(!options.cursor_blink);
        assert_eq!(options.cursor_blink_rate, CursorBlinkRate::Slow);
        assert_eq!(options.cursor_style_source, CursorStyleSource::Config);
        assert_eq!(options.session_tab_position, NativeSessionTabPosition::Top);
        assert_eq!(
            options.session_tab_label_style,
            NativeSessionTabLabelStyle::IndexProfile
        );
        assert_eq!(
            options.session_tab_display_policy,
            NativeSessionTabDisplayPolicy {
                show_single: true,
                show_multiple: true,
            }
        );
        assert_eq!(
            options.font_paths,
            vec![
                PathBuf::from("/fonts/JetBrainsMonoNerdFont-Regular.ttf"),
                PathBuf::from("/fonts/SymbolsNerdFontMono-Regular.ttf"),
            ]
        );
        assert_eq!(
            options.window_smoke,
            WindowSmokeOptions {
                open_command_palette: true,
                show_diagnostics: true,
                report_startup: true,
                exit_after: Some(Duration::from_millis(1500)),
                last_active_close_policy: WindowLastActiveClosePolicy::CloseWindow,
                initial_size: Some(GridSize::new(36, 120)),
            }
        );
    }

    #[test]
    fn app_options_apply_native_window_config_defaults() {
        let mut options = AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "/configs/window.v1.json".to_owned(),
        ])
        .unwrap();
        let config = NativeWindowConfig {
            window_title: Some("  Project Shell  ".to_owned()),
            program: Some("tmux".to_owned()),
            args: vec!["new-session".to_owned(), "-A".to_owned()],
            env: BTreeMap::from([("WITTY_SESSION".to_owned(), "config".to_owned())]),
            font_family: Some("  Hack Nerd Font  ".to_owned()),
            font_size: Some(18),
            terminal_padding: Some(6),
            background_opacity: Some(0.75),
            background_image: Some(PathBuf::from("/images/witty.png")),
            background_image_fit: Some("cover".to_owned()),
            background_overlay_color: Some("#101820".to_owned()),
            background_overlay_opacity: Some(0.22),
            cursor_shape: Some("bar".to_owned()),
            cursor_blink: Some(false),
            cursor_blink_rate: Some("slow".to_owned()),
            cursor_style_source: Some("config".to_owned()),
            session_tab_position: Some("top".to_owned()),
            session_tab_label: Some("index-profile".to_owned()),
            session_tab_show_single: Some(true),
            session_tab_show_multiple: Some(true),
            font_paths: vec![
                PathBuf::from("/fonts/HackNerdFont-Regular.ttf"),
                PathBuf::from("/fonts/SymbolsNerdFontMono-Regular.ttf"),
            ],
            cwd: Some(PathBuf::from("/work/project")),
            scrollback_lines: Some(12345),
            mouse_selection_override: Some("disabled".to_owned()),
            osc52_clipboard: Some("allow".to_owned()),
            window_last_active_close: Some("close-window".to_owned()),
            window_cols: Some(120),
            window_rows: Some(36),
        };

        options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |path| {
                    assert_eq!(path, Path::new("/configs/window.v1.json"));
                    Ok(Some(config.clone()))
                },
            )
            .unwrap();

        assert_eq!(options.window_title.as_deref(), Some("Project Shell"));
        assert_eq!(options.program.as_deref(), Some("tmux"));
        assert_eq!(options.args, ["new-session", "-A"]);
        assert_eq!(
            options.launch_env,
            [("WITTY_SESSION".to_owned(), "config".to_owned())]
        );
        assert_eq!(options.font_family.as_deref(), Some("Hack Nerd Font"));
        assert_eq!(options.font_size, Some(18));
        assert_eq!(options.terminal_padding, Some(6));
        assert_eq!(options.background_opacity, Some(0.75));
        assert_eq!(
            options.background_image.as_deref(),
            Some(Path::new("/images/witty.png"))
        );
        assert_eq!(
            options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
        assert_eq!(
            options.background_overlay_color,
            Some(Rgba::rgb(0x10, 0x18, 0x20))
        );
        assert_eq!(options.background_overlay_opacity, Some(0.22));
        assert_eq!(options.cursor_shape, CursorShape::Bar);
        assert!(!options.cursor_blink);
        assert_eq!(options.cursor_blink_rate, CursorBlinkRate::Slow);
        assert_eq!(options.session_tab_position, NativeSessionTabPosition::Top);
        assert_eq!(
            options.session_tab_label_style,
            NativeSessionTabLabelStyle::IndexProfile
        );
        assert_eq!(
            options.session_tab_display_policy,
            NativeSessionTabDisplayPolicy {
                show_single: true,
                show_multiple: true,
            }
        );
        assert_eq!(
            options.font_paths,
            vec![
                PathBuf::from("/fonts/HackNerdFont-Regular.ttf"),
                PathBuf::from("/fonts/SymbolsNerdFontMono-Regular.ttf"),
            ]
        );
        assert_eq!(options.cwd.as_deref(), Some(Path::new("/work/project")));
        assert_eq!(options.max_scrollback_lines, 12345);
        assert_eq!(
            options.mouse_selection_override,
            MouseSelectionOverridePolicy::Disabled
        );
        assert_eq!(options.osc52_clipboard_policy, Osc52ClipboardPolicy::Allow);
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::CloseWindow
        );
        assert_eq!(
            options.window_smoke.initial_size,
            Some(GridSize::new(36, 120))
        );
    }

    #[test]
    fn app_options_native_window_config_respects_cli_and_env_precedence() {
        let env_paths = std::env::join_paths([PathBuf::from("/env/Symbols.ttf")]).unwrap();
        let mut options = app_options_parse_with_env(
            [
                "--window",
                "--window-config",
                "/configs/window.v1.json",
                "--window-last-active-close",
                "block",
                "--window-title",
                "CLI Project",
                "--program",
                "/bin/zsh",
                "--arg",
                "-l",
                "--mouse-selection-override",
                "shift-select",
                "--osc52-clipboard",
                "disabled",
                "--scrollback-lines",
                "2500",
                "--font-size",
                "16",
                "--terminal-padding",
                "2",
                "--background-opacity",
                "0.9",
                "--background-image",
                "null",
                "--background-overlay-color",
                "#112233",
                "--background-overlay-opacity",
                "0.2",
                "--cwd",
                "/cli/project",
                "--env",
                "TERM=xterm-witty",
                "--env",
                "WITTY_SESSION=daily",
                "--window-cols",
                "100",
            ],
            vec![
                (
                    WITTY_FONT_FAMILY_ENV,
                    std::ffi::OsString::from("JetBrainsMono Nerd Font"),
                ),
                (WITTY_FONT_PATHS_ENV, env_paths),
            ],
        )
        .unwrap();
        let config = NativeWindowConfig {
            window_title: Some("Config Project".to_owned()),
            program: Some("tmux".to_owned()),
            args: vec!["new-session".to_owned()],
            env: BTreeMap::from([
                ("TERM".to_owned(), "xterm-config".to_owned()),
                ("WITTY_SESSION".to_owned(), "config".to_owned()),
            ]),
            font_family: Some("Hack Nerd Font".to_owned()),
            font_size: Some(18),
            terminal_padding: Some(8),
            background_opacity: Some(0.7),
            background_image: Some(PathBuf::from("/config/background.png")),
            background_image_fit: Some("cover".to_owned()),
            background_overlay_color: Some("#445566".to_owned()),
            background_overlay_opacity: Some(0.45),
            cursor_shape: Some("underline".to_owned()),
            cursor_blink: Some(true),
            cursor_blink_rate: Some("variable".to_owned()),
            cursor_style_source: Some("program".to_owned()),
            session_tab_position: Some("top".to_owned()),
            session_tab_label: Some("profile".to_owned()),
            session_tab_show_single: Some(true),
            session_tab_show_multiple: Some(true),
            font_paths: vec![PathBuf::from("/config/Hack.ttf")],
            cwd: Some(PathBuf::from("/config/project")),
            scrollback_lines: Some(50000),
            mouse_selection_override: Some("disabled".to_owned()),
            osc52_clipboard: Some("allow".to_owned()),
            window_last_active_close: Some("close-window".to_owned()),
            window_cols: Some(132),
            window_rows: Some(40),
        };

        options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |_| Ok(Some(config.clone())),
            )
            .unwrap();

        assert_eq!(options.window_title.as_deref(), Some("CLI Project"));
        assert_eq!(options.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(options.args, ["-l"]);
        assert_eq!(
            options.launch_env,
            [
                ("TERM".to_owned(), "xterm-witty".to_owned()),
                ("WITTY_SESSION".to_owned(), "daily".to_owned()),
            ]
        );
        assert_eq!(
            options.font_family.as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(options.font_paths, vec![PathBuf::from("/env/Symbols.ttf")]);
        assert_eq!(options.max_scrollback_lines, 2500);
        assert_eq!(options.cwd.as_deref(), Some(Path::new("/cli/project")));
        assert_eq!(options.font_size, Some(16));
        assert_eq!(options.terminal_padding, Some(2));
        assert_eq!(options.background_opacity, Some(0.9));
        assert_eq!(options.background_image, None);
        assert_eq!(
            options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
        assert_eq!(
            options.background_overlay_color,
            Some(Rgba::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(options.background_overlay_opacity, Some(0.2));
        assert_eq!(options.cursor_shape, CursorShape::Underline);
        assert!(options.cursor_blink);
        assert_eq!(options.cursor_blink_rate, CursorBlinkRate::Variable);
        assert_eq!(
            options.mouse_selection_override,
            MouseSelectionOverridePolicy::ShiftSelect
        );
        assert_eq!(
            options.osc52_clipboard_policy,
            Osc52ClipboardPolicy::Disabled
        );
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::Block
        );
        assert_eq!(
            options.window_smoke.initial_size,
            Some(GridSize::new(40, 100))
        );
    }

    #[test]
    fn app_options_native_window_config_default_missing_is_ignored() {
        let mut options = AppOptions::parse(["--window".to_owned()]).unwrap();

        options
            .apply_native_window_config_defaults(
                |_| None,
                || Ok(PathBuf::from("/configs/window.v1.json")),
                |_| Ok(None),
            )
            .unwrap();

        assert_eq!(options.font_family, None);
        assert_eq!(options.font_size, None);
        assert_eq!(options.terminal_padding, None);
        assert_eq!(options.background_opacity, None);
        assert_eq!(options.background_image, None);
        assert_eq!(options.background_image_fit, None);
        assert!(options.font_paths.is_empty());
        assert_eq!(options.cwd, None);
        assert_eq!(options.window_title, None);
        assert_eq!(options.program, None);
        assert!(options.args.is_empty());
        assert!(options.launch_env.is_empty());
        assert_eq!(options.max_scrollback_lines, DEFAULT_MAX_SCROLLBACK_LINES);
        assert_eq!(options.window_smoke.initial_size, None);
    }

    #[test]
    fn wittyrc_template_is_bundled_toml_with_maple_font_family_and_exit_policy() {
        let template = wittyrc_template();
        let config: WittyrcConfig = toml::from_str(template).unwrap();

        assert_eq!(
            config.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );
        assert_eq!(
            config.window_last_active_close.as_deref(),
            Some("close-window")
        );
        assert_eq!(
            config.font_size,
            Some(witty_render_wgpu::DEFAULT_TERMINAL_FONT_SIZE)
        );
        assert_eq!(config.terminal_padding, Some(0));
        assert_eq!(config.background_opacity, Some(1.0));
        assert_eq!(config.background_image.as_deref(), Some("null"));
        assert_eq!(config.background_image_fit.as_deref(), Some("cover"));
        assert_eq!(config.background_overlay_color.as_deref(), Some("#000000"));
        assert_eq!(config.background_overlay_opacity, Some(0.0));
        assert_eq!(config.theme_foreground.as_deref(), Some("#ffffff"));
        assert_eq!(config.theme_background.as_deref(), Some("#000000"));
        assert_eq!(config.theme_cursor.as_deref(), Some("null"));
        assert_eq!(
            config.theme_palette.len(),
            TerminalColorTheme::ANSI_COLOR_COUNT
        );
        assert_eq!(config.cursor_shape.as_deref(), Some("block"));
        assert_eq!(config.cursor_blink, Some(true));
        assert_eq!(config.cursor_blink_rate.as_deref(), Some("normal"));
        assert_eq!(config.cursor_style_source.as_deref(), Some("program"));
        assert_eq!(config.session_tab_position.as_deref(), Some("top"));
        assert_eq!(config.session_tab_label.as_deref(), Some("index"));
        assert_eq!(config.session_tab_show_single, Some(false));
        assert_eq!(config.session_tab_show_multiple, Some(false));
        assert_eq!(config.osc52_clipboard.as_deref(), Some("allow"));
        assert!(template.contains("font-family = \"Maple Mono NF CN\""));
        assert!(template.contains("font-size = 14"));
        assert!(template.contains("terminal-padding = 0"));
        assert!(template.contains("background-opacity = 1.0"));
        assert!(template.contains("background-image = \"null\""));
        assert!(template.contains("background-image-fit = \"cover\""));
        assert!(template.contains("background-overlay-color = \"#000000\""));
        assert!(template.contains("background-overlay-opacity = 0.0"));
        assert!(template.contains("theme-foreground = \"#ffffff\""));
        assert!(template.contains("theme-background = \"#000000\""));
        assert!(template.contains("theme-cursor = \"null\""));
        assert!(template.contains("theme-palette = ["));
        assert!(template.contains("cursor-shape = \"block\""));
        assert!(template.contains("cursor-blink = true"));
        assert!(template.contains("cursor-blink-rate = \"normal\""));
        assert!(template.contains("cursor-style-source = \"program\""));
        assert!(template.contains("session-tab-position = \"top\""));
        assert!(template.contains("session-tab-label = \"index\""));
        assert!(template.contains("session-tab-show-single = false"));
        assert!(template.contains("session-tab-show-multiple = false"));
        assert!(template.contains("osc52-clipboard = \"allow\""));
        assert!(template.contains("window-last-active-close = \"close-window\""));
        assert!(template.ends_with('\n'));
    }

    #[test]
    fn wittyrc_default_path_uses_home_dot_wittyrc() {
        let path = default_wittyrc_path_with_home(|| Some(OsString::from("/home/alice"))).unwrap();
        assert_eq!(path, PathBuf::from("/home/alice/.wittyrc"));

        let line = wittyrc_default_path_line(|| Ok(PathBuf::from("/home/alice/.wittyrc"))).unwrap();
        assert_eq!(line, "/home/alice/.wittyrc\n");

        assert!(default_wittyrc_path_with_home(|| None).is_err());
        assert!(default_wittyrc_path_with_home(|| Some(OsString::new())).is_err());
    }

    #[test]
    fn read_wittyrc_config_reads_toml_and_rejects_unknown_fields() {
        let root = unique_temp_dir("wittyrc-read");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join(".wittyrc");
        let unknown_path = root.join("unknown.wittyrc");
        std::fs::write(
            &config_path,
            r##"font-family = "Maple Mono NF CN"
font-size = 15
terminal-padding = 4
background-opacity = 0.8
background-image = "/images/witty.png"
background-image-fit = "scale-and-crop"
background-overlay-color = "#102030"
background-overlay-opacity = 0.25
theme-foreground = "#dcd7ba"
theme-background = "#1f1f28"
theme-cursor = "#c8c093"
theme-palette = [
  "#090618", "#c34043", "#76946a", "#c0a36e",
  "#7e9cd8", "#957fb8", "#6a9589", "#c8c093",
  "#727169", "#e82424", "#98bb6c", "#e6c384",
  "#7fb4ca", "#938aa9", "#7aa89f", "#dcd7ba",
]
cursor-shape = "underline"
cursor-blink = false
cursor-blink-rate = "slow"
cursor-style-source = "config"
osc52-clipboard = "allow""##,
        )
        .unwrap();
        std::fs::write(&unknown_path, r#"font_family = "Maple Mono NF CN""#).unwrap();

        let config = read_wittyrc_config(&config_path).unwrap().unwrap();
        assert_eq!(
            config.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );
        assert_eq!(config.font_size, Some(15));
        assert_eq!(config.terminal_padding, Some(4));
        assert_eq!(config.background_opacity, Some(0.8));
        assert_eq!(
            config.background_image.as_deref(),
            Some("/images/witty.png")
        );
        assert_eq!(
            config.background_image_fit.as_deref(),
            Some("scale-and-crop")
        );
        assert_eq!(config.background_overlay_color.as_deref(), Some("#102030"));
        assert_eq!(config.background_overlay_opacity, Some(0.25));
        assert_eq!(config.theme_foreground.as_deref(), Some("#dcd7ba"));
        assert_eq!(config.theme_background.as_deref(), Some("#1f1f28"));
        assert_eq!(config.theme_cursor.as_deref(), Some("#c8c093"));
        assert_eq!(
            config.theme_palette.len(),
            TerminalColorTheme::ANSI_COLOR_COUNT
        );
        assert_eq!(config.theme_palette[1], "#c34043");
        assert_eq!(config.cursor_shape.as_deref(), Some("underline"));
        assert_eq!(config.cursor_blink, Some(false));
        assert_eq!(config.cursor_blink_rate.as_deref(), Some("slow"));
        assert_eq!(config.cursor_style_source.as_deref(), Some("config"));
        assert_eq!(config.osc52_clipboard.as_deref(), Some("allow"));
        assert_eq!(config.window_last_active_close, None);
        assert!(read_wittyrc_config(&root.join("missing.wittyrc"))
            .unwrap()
            .is_none());
        assert!(read_wittyrc_config(&unknown_path).is_err());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn app_options_wittyrc_precedence_over_window_config_and_under_env() {
        let mut options = app_options_parse_with_env(
            [
                "--window",
                "--wittyrc",
                "/configs/.wittyrc",
                "--window-config",
                "/configs/window.v1.json",
            ],
            vec![],
        )
        .unwrap();

        let wittyrc_load = options
            .apply_wittyrc_defaults(
                || unreachable!("explicit wittyrc should not use default path"),
                |path| {
                    assert_eq!(path, Path::new("/configs/.wittyrc"));
                    Ok(Some(WittyrcConfig {
                        font_family: Some("Maple Mono NF CN".to_owned()),
                        font_size: Some(15),
                        terminal_padding: Some(4),
                        background_opacity: Some(0.6),
                        background_image: Some("/wittyrc/background.png".to_owned()),
                        background_image_fit: Some("cover".to_owned()),
                        background_overlay_color: Some("#14283c".to_owned()),
                        background_overlay_opacity: Some(0.26),
                        theme_foreground: Some("#dcd7ba".to_owned()),
                        theme_background: Some("#1f1f28".to_owned()),
                        theme_cursor: Some("#c8c093".to_owned()),
                        theme_palette: vec![
                            "#090618".to_owned(),
                            "#c34043".to_owned(),
                            "#76946a".to_owned(),
                            "#c0a36e".to_owned(),
                            "#7e9cd8".to_owned(),
                            "#957fb8".to_owned(),
                            "#6a9589".to_owned(),
                            "#c8c093".to_owned(),
                            "#727169".to_owned(),
                            "#e82424".to_owned(),
                            "#98bb6c".to_owned(),
                            "#e6c384".to_owned(),
                            "#7fb4ca".to_owned(),
                            "#938aa9".to_owned(),
                            "#7aa89f".to_owned(),
                            "#dcd7ba".to_owned(),
                        ],
                        window_last_active_close: Some("block".to_owned()),
                        cursor_shape: Some("bar".to_owned()),
                        cursor_blink: Some(false),
                        cursor_blink_rate: Some("slow".to_owned()),
                        cursor_style_source: Some("config".to_owned()),
                        session_tab_position: Some("bottom".to_owned()),
                        session_tab_label: Some("index".to_owned()),
                        session_tab_show_single: Some(false),
                        session_tab_show_multiple: Some(false),
                        osc52_clipboard: Some("allow".to_owned()),
                        ..WittyrcConfig::default()
                    }))
                },
            )
            .unwrap();
        let window_load = options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |path| {
                    assert_eq!(path, Path::new("/configs/window.v1.json"));
                    Ok(Some(NativeWindowConfig {
                        font_family: Some("Hack Nerd Font".to_owned()),
                        font_size: Some(18),
                        terminal_padding: Some(8),
                        background_opacity: Some(0.8),
                        background_image: Some(PathBuf::from("/window/background.png")),
                        background_overlay_color: Some("#ffffff".to_owned()),
                        background_overlay_opacity: Some(0.9),
                        window_last_active_close: Some("close-window".to_owned()),
                        ..NativeWindowConfig::default()
                    }))
                },
            )
            .unwrap();

        assert_eq!(wittyrc_load.status, WittyrcConfigLoadStatus::Loaded);
        assert_eq!(window_load.status, NativeWindowConfigLoadStatus::Loaded);
        assert_eq!(
            options.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );
        assert_eq!(options.font_size, Some(15));
        assert_eq!(options.terminal_padding, Some(4));
        assert_eq!(options.background_opacity, Some(0.6));
        assert_eq!(
            options.background_image.as_deref(),
            Some(Path::new("/wittyrc/background.png"))
        );
        assert_eq!(
            options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
        assert_eq!(
            options.background_overlay_color,
            Some(Rgba::rgb(0x14, 0x28, 0x3c))
        );
        assert_eq!(options.background_overlay_opacity, Some(0.26));
        assert_eq!(
            options.terminal_color_theme.foreground,
            Rgba::rgb(0xdc, 0xd7, 0xba)
        );
        assert_eq!(
            options.terminal_color_theme.background,
            Rgba::rgb(0x1f, 0x1f, 0x28)
        );
        assert_eq!(
            options.terminal_color_theme.cursor_color,
            Some(Rgba::rgb(0xc8, 0xc0, 0x93))
        );
        assert_eq!(
            options.terminal_color_theme.palette[1],
            Rgba::rgb(0xc3, 0x40, 0x43)
        );
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::Block
        );
        assert_eq!(options.cursor_shape, CursorShape::Bar);
        assert!(!options.cursor_blink);
        assert_eq!(options.cursor_blink_rate, CursorBlinkRate::Slow);
        assert_eq!(options.osc52_clipboard_policy, Osc52ClipboardPolicy::Allow);

        let mut env_options = app_options_parse_with_env(
            ["--window", "--wittyrc", "/configs/.wittyrc"],
            vec![(
                WITTY_FONT_FAMILY_ENV,
                OsString::from("JetBrainsMono Nerd Font"),
            )],
        )
        .unwrap();
        env_options
            .apply_wittyrc_defaults(
                || unreachable!("explicit wittyrc should not use default path"),
                |_| {
                    Ok(Some(WittyrcConfig {
                        font_family: Some("Maple Mono NF CN".to_owned()),
                        font_size: Some(15),
                        terminal_padding: Some(5),
                        background_opacity: Some(0.7),
                        background_image: Some("none".to_owned()),
                        background_image_fit: Some("cover".to_owned()),
                        ..WittyrcConfig::default()
                    }))
                },
            )
            .unwrap();

        assert_eq!(
            env_options.font_family.as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(env_options.font_size, Some(15));
        assert_eq!(env_options.terminal_padding, Some(5));
        assert_eq!(env_options.background_opacity, Some(0.7));
        assert_eq!(env_options.background_image, None);
        assert_eq!(
            env_options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
    }

    #[test]
    fn app_options_no_wittyrc_skips_wittyrc_loading() {
        let mut options = app_options_parse_with_env(["--window", "--no-wittyrc"], vec![]).unwrap();

        let load = options
            .apply_wittyrc_defaults(
                || unreachable!("disabled wittyrc should not use default path"),
                |_| unreachable!("disabled wittyrc should not load config"),
            )
            .unwrap();

        assert_eq!(load, WittyrcConfigLoadReport::disabled());
        assert_eq!(options.font_family, None);
    }

    #[test]
    fn app_options_wittyrc_apply_failure_preserves_existing_options() {
        let mut options = AppOptions::parse([
            "--window".to_owned(),
            "--wittyrc".to_owned(),
            "/configs/.wittyrc".to_owned(),
        ])
        .unwrap();

        let error = options
            .apply_wittyrc_defaults(
                || unreachable!("explicit wittyrc should not use default path"),
                |_| {
                    Ok(Some(WittyrcConfig {
                        font_size: Some(18),
                        cursor_shape: Some("caret".to_owned()),
                        ..WittyrcConfig::default()
                    }))
                },
            )
            .unwrap_err();

        assert!(format!("{error:#}").contains("cursor-shape"));
        assert_eq!(options.font_size, None);
        assert_eq!(options.cursor_shape, CursorShape::Block);
    }

    #[test]
    fn app_options_wittyrc_theme_failure_preserves_existing_options() {
        let mut options = AppOptions::parse([
            "--window".to_owned(),
            "--wittyrc".to_owned(),
            "/configs/.wittyrc".to_owned(),
        ])
        .unwrap();

        let error = options
            .apply_wittyrc_defaults(
                || unreachable!("explicit wittyrc should not use default path"),
                |_| {
                    Ok(Some(WittyrcConfig {
                        font_size: Some(18),
                        theme_foreground: Some("#dcd7ba".to_owned()),
                        theme_palette: vec!["#000000".to_owned()],
                        ..WittyrcConfig::default()
                    }))
                },
            )
            .unwrap_err();

        assert!(format!("{error:#}").contains("theme-palette"));
        assert_eq!(options.font_size, None);
        assert_eq!(options.terminal_color_theme, TerminalColorTheme::default());
    }

    #[test]
    fn wittyrc_startup_error_notice_points_to_check_command() {
        let config_ref = WittyrcConfigRef {
            path: PathBuf::from("/home/alice/.wittyrc"),
            required: false,
        };
        let error = anyhow::anyhow!("parse failed");
        let notice = wittyrc_startup_error_notice(Some(&config_ref), &error);

        assert!(notice.contains("/home/alice/.wittyrc"));
        assert!(notice.contains("witty --wittyrc-check"));
        assert!(notice.contains("parse failed"));
    }

    #[test]
    fn wittyrc_init_creates_template_and_refuses_overwrite() {
        let root = unique_temp_dir("wittyrc-init");
        let path = root.join("home").join(".wittyrc");

        let output = init_wittyrc(|| Ok(path.clone())).unwrap();

        assert_eq!(output, format!("created wittyrc: {}\n", path.display()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), wittyrc_template());
        let config = read_wittyrc_config(&path).unwrap().unwrap();
        assert_eq!(
            config.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );

        let error = init_wittyrc(|| Ok(path.clone())).unwrap_err();
        assert!(format!("{error:#}").contains("create wittyrc"), "{error:#}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn wittyrc_init_for_options_accepts_explicit_wittyrc_path() {
        let root = unique_temp_dir("wittyrc-init-explicit");
        let path = root.join("candidate").join(".wittyrc");
        let options = AppOptions::parse([
            "--wittyrc-init".to_owned(),
            "--wittyrc".to_owned(),
            path.display().to_string(),
        ])
        .unwrap();

        let output = init_wittyrc_for_options(&options, || {
            unreachable!("explicit init should not use default path")
        })
        .unwrap();

        assert_eq!(output, format!("created wittyrc: {}\n", path.display()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), wittyrc_template());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn wittyrc_check_accepts_default_missing_and_explicit_valid_config() {
        let default_options = AppOptions::parse(["--wittyrc-check".to_owned()]).unwrap();
        let default_output = check_wittyrc_config(
            &default_options,
            || Ok(PathBuf::from("/home/alice/.wittyrc")),
            |_| Ok(None),
        )
        .unwrap();

        assert_eq!(default_output, "wittyrc missing: /home/alice/.wittyrc\n");

        let explicit_options = AppOptions::parse([
            "--wittyrc-check".to_owned(),
            "--wittyrc".to_owned(),
            "/candidate/.wittyrc".to_owned(),
        ])
        .unwrap();
        assert_eq!(explicit_options.mode, AppMode::WittyrcCheck);
        assert_eq!(
            explicit_options.wittyrc_path.as_deref(),
            Some(Path::new("/candidate/.wittyrc"))
        );

        let explicit_output = check_wittyrc_config(
            &explicit_options,
            || unreachable!("explicit check should not use default path"),
            |path| {
                assert_eq!(path, Path::new("/candidate/.wittyrc"));
                Ok(Some(WittyrcConfig {
                    font_family: Some("Maple Mono NF CN".to_owned()),
                    ..WittyrcConfig::default()
                }))
            },
        )
        .unwrap();

        assert_eq!(explicit_output, "wittyrc ok: /candidate/.wittyrc\n");
    }

    #[test]
    fn wittyrc_check_explicit_missing_errors() {
        let options = AppOptions::parse([
            "--wittyrc-check".to_owned(),
            "--wittyrc".to_owned(),
            "/missing/.wittyrc".to_owned(),
        ])
        .unwrap();

        let error = check_wittyrc_config(
            &options,
            || unreachable!("explicit check should not use default path"),
            |_| Ok(None),
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("wittyrc file does not exist"),
            "{error:#}"
        );
    }

    #[test]
    fn wittyrc_effective_config_summary_reports_loaded_status_without_window_side_effects() {
        let mut options = app_options_parse_with_env(
            [
                "--wittyrc-effective",
                "--wittyrc",
                "/home/alice/.wittyrc",
                "--window-config",
                "/configs/window.v1.json",
                "--font-size",
                "18",
                "--background-opacity",
                "0.65",
            ],
            vec![],
        )
        .unwrap();
        let wittyrc_load = options
            .apply_wittyrc_defaults(
                || unreachable!("explicit wittyrc should not use default path"),
                |_| {
                    Ok(Some(WittyrcConfig {
                        font_family: Some("Maple Mono NF CN".to_owned()),
                        terminal_padding: Some(7),
                        background_overlay_color: Some("#101820".to_owned()),
                        background_overlay_opacity: Some(0.21),
                        theme_foreground: Some("#dcd7ba".to_owned()),
                        theme_background: Some("#1f1f28".to_owned()),
                        theme_cursor: Some("none".to_owned()),
                        ..WittyrcConfig::default()
                    }))
                },
            )
            .unwrap();
        let window_load = options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |_| {
                    Ok(Some(NativeWindowConfig {
                        font_family: Some("Hack Nerd Font".to_owned()),
                        font_size: Some(16),
                        terminal_padding: Some(3),
                        background_opacity: Some(0.9),
                        background_image: Some(PathBuf::from("/window/background.png")),
                        background_image_fit: Some("cover".to_owned()),
                        ..NativeWindowConfig::default()
                    }))
                },
            )
            .unwrap();

        let summary =
            wittyrc_effective_config_summary(&options, &wittyrc_load, &window_load).unwrap();
        let json: serde_json::Value = serde_json::from_str(&summary).unwrap();

        assert_eq!(json["event"], "witty.wittyrc_effective");
        assert_eq!(json["opens_window"], false);
        assert_eq!(json["starts_pty"], false);
        assert_eq!(json["reads_font_files"], false);
        assert_eq!(json["wittyrc"]["status"], "loaded");
        assert_eq!(json["wittyrc"]["path"], "/home/alice/.wittyrc");
        assert_eq!(json["wittyrc"]["required"], true);
        assert_eq!(json["window_config"]["status"], "loaded");
        assert_eq!(json["window_config"]["path"], "/configs/window.v1.json");
        assert_eq!(json["window_config"]["required"], true);
        assert_eq!(json["font_family"], RECOMMENDED_TERMINAL_FONT_FAMILY);
        assert_eq!(json["font_size"], 18);
        assert_eq!(json["terminal_padding"], 7.0);
        assert!((json["background_opacity"].as_f64().unwrap() - 0.65).abs() < 0.001);
        assert_eq!(json["background_image"], "/window/background.png");
        assert_eq!(json["background_image_fit"], "cover");
        assert_eq!(json["background_overlay_color"], "#101820");
        assert!((json["background_overlay_opacity"].as_f64().unwrap() - 0.21).abs() < 0.001);
        assert_eq!(json["terminal_theme"]["foreground"], "#dcd7ba");
        assert_eq!(json["terminal_theme"]["background"], "#1f1f28");
        assert_eq!(json["terminal_theme"]["cursor"], serde_json::Value::Null);
        assert_eq!(
            json["terminal_theme"]["palette"].as_array().unwrap().len(),
            16
        );
        assert_eq!(json["cursor_shape"], "block");
        assert_eq!(json["cursor_blink"], true);
        assert_eq!(json["cursor_blink_rate"], "normal");
    }

    #[test]
    fn native_window_config_template_is_valid_daily_window_config() {
        let template = native_window_config_template();
        let config: NativeWindowConfig = serde_json::from_str(&template).unwrap();
        let mut options = AppOptions::parse(["--window".to_owned()]).unwrap();

        options.apply_native_window_config(config).unwrap();

        assert_eq!(
            options.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );
        assert_eq!(options.font_size, Some(16));
        assert_eq!(options.terminal_padding, Some(0));
        assert_eq!(options.background_opacity, Some(1.0));
        assert_eq!(options.background_image, None);
        assert_eq!(
            options.background_image_fit,
            Some(RendererBackgroundImageFit::Cover)
        );
        assert_eq!(
            options.background_overlay_color,
            Some(Rgba::rgb(0x00, 0x00, 0x00))
        );
        assert_eq!(options.background_overlay_opacity, Some(0.0));
        assert_eq!(options.cursor_shape, CursorShape::Block);
        assert!(options.cursor_blink);
        assert_eq!(options.session_tab_position, NativeSessionTabPosition::Top);
        assert_eq!(
            options.session_tab_label_style,
            NativeSessionTabLabelStyle::Index
        );
        assert_eq!(
            options.session_tab_display_policy,
            NativeSessionTabDisplayPolicy {
                show_single: false,
                show_multiple: false,
            }
        );
        assert_eq!(options.window_title.as_deref(), Some("Witty"));
        assert_eq!(options.program, None);
        assert!(options.args.is_empty());
        assert_eq!(
            options.launch_env,
            [("WITTY_SESSION".to_owned(), "daily".to_owned())]
        );
        let expected_home =
            PathBuf::from(env::var_os("HOME").expect("HOME should be set for template test"));
        assert_eq!(options.cwd.as_deref(), Some(expected_home.as_path()));
        assert_eq!(
            options.window_smoke.initial_size,
            Some(GridSize::new(36, 120))
        );
        assert_eq!(options.max_scrollback_lines, 20000);
        assert_eq!(
            options.mouse_selection_override,
            MouseSelectionOverridePolicy::ShiftSelect
        );
        assert_eq!(
            options.osc52_clipboard_policy,
            Osc52ClipboardPolicy::Disabled
        );
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::CloseWindow
        );
        assert!(template.ends_with('\n'));
    }

    #[test]
    fn native_window_config_default_path_line_prints_path_with_newline() {
        let line =
            native_window_config_default_path_line(|| Ok(PathBuf::from("/configs/window.v1.json")))
                .unwrap();

        assert_eq!(line, "/configs/window.v1.json\n");
    }

    #[test]
    fn init_native_window_config_creates_parent_and_refuses_overwrite() {
        let root = unique_temp_dir("witty-window-config-init");
        let path = root.join("config").join("witty").join("window.v1.json");

        let output = init_native_window_config(|| Ok(path.clone())).unwrap();

        assert_eq!(
            output,
            format!("created native window config: {}\n", path.display())
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let config: NativeWindowConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(
            config.font_family.as_deref(),
            Some(RECOMMENDED_TERMINAL_FONT_FAMILY)
        );

        let error = init_native_window_config(|| Ok(path.clone())).unwrap_err();
        assert!(
            format!("{error:#}").contains("create native window config"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn init_native_window_config_for_options_accepts_explicit_window_config_path() {
        let root = unique_temp_dir("witty-window-config-init-explicit");
        let path = root.join("candidate").join("window.v1.json");
        let options = AppOptions::parse([
            "--window-config-init".to_owned(),
            "--window-config".to_owned(),
            path.display().to_string(),
        ])
        .unwrap();

        let output = init_native_window_config_for_options(&options, || {
            unreachable!("explicit init should not use default path")
        })
        .unwrap();

        assert_eq!(
            output,
            format!("created native window config: {}\n", path.display())
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let config: NativeWindowConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(config.window_title.as_deref(), Some("Witty"));

        let error = init_native_window_config_for_options(&options, || {
            unreachable!("explicit init should not use default path")
        })
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("create native window config"),
            "{error:#}"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn check_native_window_config_accepts_default_missing_and_explicit_valid_config() {
        let default_options = AppOptions::parse(["--window-config-check".to_owned()]).unwrap();
        let default_output = check_native_window_config(
            &default_options,
            |_| None,
            || Ok(PathBuf::from("/configs/window.v1.json")),
            |_| Ok(None),
        )
        .unwrap();

        assert_eq!(
            default_output,
            "native window config missing: /configs/window.v1.json\n"
        );

        let explicit_options = AppOptions::parse([
            "--window-config-check".to_owned(),
            "--window-config".to_owned(),
            "/candidate/window.v1.json".to_owned(),
        ])
        .unwrap();
        assert_eq!(explicit_options.mode, AppMode::WindowConfigCheck);
        assert_eq!(
            explicit_options.window_config_path.as_deref(),
            Some(Path::new("/candidate/window.v1.json"))
        );

        let explicit_output = check_native_window_config(
            &explicit_options,
            |_| None,
            || unreachable!("explicit check should not use default path"),
            |path| {
                assert_eq!(path, Path::new("/candidate/window.v1.json"));
                Ok(Some(NativeWindowConfig {
                    window_title: Some("Candidate Shell".to_owned()),
                    program: Some("tmux".to_owned()),
                    args: vec!["new-session".to_owned(), "-A".to_owned()],
                    env: BTreeMap::from([("WITTY_SESSION".to_owned(), "candidate".to_owned())]),
                    font_family: Some("JetBrainsMono Nerd Font".to_owned()),
                    font_size: Some(16),
                    terminal_padding: Some(8),
                    background_opacity: Some(0.85),
                    background_image: Some(PathBuf::from("/candidate/background.png")),
                    background_image_fit: Some("cover".to_owned()),
                    background_overlay_color: Some("#000000".to_owned()),
                    background_overlay_opacity: Some(0.25),
                    cursor_shape: Some("block".to_owned()),
                    cursor_blink: Some(true),
                    cursor_blink_rate: Some("slow".to_owned()),
                    cursor_style_source: Some("config".to_owned()),
                    session_tab_position: Some("bottom".to_owned()),
                    session_tab_label: Some("index".to_owned()),
                    session_tab_show_single: Some(false),
                    session_tab_show_multiple: Some(false),
                    font_paths: vec![PathBuf::from("/missing/SymbolsNerdFontMono.ttf")],
                    cwd: Some(PathBuf::from("/work/project")),
                    scrollback_lines: Some(20000),
                    mouse_selection_override: Some("shift-select".to_owned()),
                    osc52_clipboard: Some("disabled".to_owned()),
                    window_last_active_close: Some("block".to_owned()),
                    window_cols: Some(120),
                    window_rows: Some(36),
                }))
            },
        )
        .unwrap();

        assert_eq!(
            explicit_output,
            "native window config ok: /candidate/window.v1.json\n"
        );
    }

    #[test]
    fn check_native_window_config_explicit_missing_errors() {
        let options = AppOptions::parse([
            "--window-config-check".to_owned(),
            "--window-config".to_owned(),
            "/missing/window.v1.json".to_owned(),
        ])
        .unwrap();

        let error = check_native_window_config(
            &options,
            |_| None,
            || unreachable!("explicit check should not use default path"),
            |_| Ok(None),
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("native window config file does not exist"),
            "{error:#}"
        );
    }

    #[test]
    fn native_window_effective_config_summary_applies_config_and_redacts_env_values_and_args() {
        let mut options = app_options_parse_with_env(
            [
                "--window-config-effective",
                "--window-config",
                "/configs/window.v1.json",
                "--font-size",
                "18",
                "--background-image",
                "/cli/background.png",
                "--env",
                "TOKEN=secret-token",
                "--cwd",
                "/cli/project",
                "--osc52-clipboard",
                "allow",
                "--window-cols",
                "100",
            ],
            vec![(
                WITTY_FONT_FAMILY_ENV,
                OsString::from("JetBrainsMono Nerd Font"),
            )],
        )
        .unwrap();

        let load = options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit summary should not use default path"),
                |_| {
                    Ok(Some(NativeWindowConfig {
                        window_title: Some("Config Shell".to_owned()),
                        program: Some("tmux".to_owned()),
                        args: vec!["new-session".to_owned(), "-A".to_owned()],
                        env: BTreeMap::from([("TOKEN".to_owned(), "config-token".to_owned())]),
                        font_family: Some("Config Font".to_owned()),
                        font_size: Some(16),
                        terminal_padding: Some(8),
                        background_opacity: Some(0.78),
                        background_image: Some(PathBuf::from("/config/background.png")),
                        background_image_fit: Some("cover".to_owned()),
                        background_overlay_color: Some("#081018".to_owned()),
                        background_overlay_opacity: Some(0.3),
                        cursor_shape: Some("bar".to_owned()),
                        cursor_blink: Some(false),
                        cursor_blink_rate: Some("variable".to_owned()),
                        cursor_style_source: Some("config".to_owned()),
                        session_tab_position: Some("bottom".to_owned()),
                        session_tab_label: Some("index".to_owned()),
                        session_tab_show_single: Some(false),
                        session_tab_show_multiple: Some(false),
                        font_paths: vec![PathBuf::from("/fonts/Symbols.ttf")],
                        cwd: Some(PathBuf::from("/config/project")),
                        scrollback_lines: Some(20000),
                        mouse_selection_override: Some("disabled".to_owned()),
                        osc52_clipboard: Some("disabled".to_owned()),
                        window_last_active_close: Some("close-window".to_owned()),
                        window_cols: Some(120),
                        window_rows: Some(36),
                    }))
                },
            )
            .unwrap();
        let wittyrc_load = WittyrcConfigLoadReport {
            path: Some(PathBuf::from("/home/alice/.wittyrc")),
            required: false,
            status: WittyrcConfigLoadStatus::Loaded,
        };
        let summary =
            native_window_effective_config_summary(&options, &load, &wittyrc_load).unwrap();
        let json: serde_json::Value = serde_json::from_str(&summary).unwrap();

        assert_eq!(json["event"], "witty.native_window_config_effective");
        assert_eq!(json["opens_window"], false);
        assert_eq!(json["starts_pty"], false);
        assert_eq!(json["reads_font_files"], false);
        assert_eq!(json["config"]["status"], "loaded");
        assert_eq!(json["config"]["path"], "/configs/window.v1.json");
        assert_eq!(json["config"]["required"], true);
        assert_eq!(json["wittyrc"]["status"], "loaded");
        assert_eq!(json["wittyrc"]["path"], "/home/alice/.wittyrc");
        assert_eq!(json["wittyrc"]["required"], false);
        assert_eq!(json["window_title"], "Config Shell");
        assert_eq!(json["program_configured"], true);
        assert_eq!(json["program"], "tmux");
        assert_eq!(json["arg_count"], 2);
        assert_eq!(json["cwd"], "/cli/project");
        assert_eq!(json["env_keys"], serde_json::json!(["TOKEN"]));
        assert_eq!(json["font_family"], "JetBrainsMono Nerd Font");
        assert_eq!(json["font_size"], 18);
        assert_eq!(json["terminal_padding"], 8.0);
        assert!((json["background_opacity"].as_f64().unwrap() - 0.78).abs() < 0.001);
        assert_eq!(json["background_image"], "/cli/background.png");
        assert_eq!(json["background_image_fit"], "cover");
        assert_eq!(json["background_overlay_color"], "#081018");
        assert!((json["background_overlay_opacity"].as_f64().unwrap() - 0.3).abs() < 0.001);
        assert_eq!(json["cursor_shape"], "bar");
        assert_eq!(json["cursor_blink"], false);
        assert_eq!(json["cursor_blink_rate"], "variable");
        assert_eq!(json["font_source_count"], 1);
        assert_eq!(json["window_cols"], 100);
        assert_eq!(json["window_rows"], 36);
        assert_eq!(json["scrollback_lines"], 20000);
        assert_eq!(json["mouse_selection_override"], "disabled");
        assert_eq!(json["osc52_clipboard"], "allow");
        assert_eq!(json["window_last_active_close"], "close-window");
        assert!(!summary.contains("secret-token"), "{summary}");
        assert!(!summary.contains("config-token"), "{summary}");
        assert!(!summary.contains("new-session"), "{summary}");
    }

    #[test]
    fn native_window_effective_config_summary_reports_missing_and_disabled_config() {
        let mut missing_options =
            AppOptions::parse(["--window-config-effective".to_owned()]).unwrap();
        let missing_load = missing_options
            .apply_native_window_config_defaults(
                |_| None,
                || Ok(PathBuf::from("/configs/window.v1.json")),
                |_| Ok(None),
            )
            .unwrap();
        let missing: serde_json::Value = serde_json::from_str(
            &native_window_effective_config_summary(
                &missing_options,
                &missing_load,
                &WittyrcConfigLoadReport::disabled(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(missing["config"]["status"], "missing");
        assert_eq!(missing["config"]["path"], "/configs/window.v1.json");
        assert_eq!(missing["config"]["required"], false);
        assert_eq!(missing["window_title"], DEFAULT_WINDOW_TITLE);

        let mut disabled_options = AppOptions::parse([
            "--window-config-effective".to_owned(),
            "--no-window-config".to_owned(),
        ])
        .unwrap();
        let disabled_load = disabled_options
            .apply_native_window_config_defaults(
                |_| unreachable!("disabled summary should not read env config"),
                || unreachable!("disabled summary should not use default path"),
                |_| unreachable!("disabled summary should not load config"),
            )
            .unwrap();
        let disabled: serde_json::Value = serde_json::from_str(
            &native_window_effective_config_summary(
                &disabled_options,
                &disabled_load,
                &WittyrcConfigLoadReport::disabled(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(disabled["config"]["status"], "disabled");
        assert_eq!(disabled["config"]["path"], serde_json::Value::Null);
        assert_eq!(disabled["config"]["required"], false);
    }

    #[test]
    fn app_options_native_window_config_can_set_partial_initial_size() {
        let mut options = AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "/configs/window.v1.json".to_owned(),
        ])
        .unwrap();
        let config = NativeWindowConfig {
            window_cols: Some(132),
            ..NativeWindowConfig::default()
        };

        options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |_| Ok(Some(config.clone())),
            )
            .unwrap();

        assert_eq!(
            options.window_smoke.initial_size,
            Some(GridSize::new(DEFAULT_WINDOW_ROWS, 132))
        );
    }

    #[test]
    fn app_options_native_window_config_rejects_invalid_initial_size() {
        for config in [
            NativeWindowConfig {
                window_cols: Some(MIN_WINDOW_COLS - 1),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                window_rows: Some(MAX_WINDOW_ROWS + 1),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                cwd: Some(PathBuf::from("   ")),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                window_title: Some("   ".to_owned()),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                program: Some("   ".to_owned()),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                args: vec!["new-session".to_owned()],
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                env: BTreeMap::from([(" ".to_owned(), "bad".to_owned())]),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                terminal_padding: Some(MAX_TERMINAL_PADDING + 1),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                background_opacity: Some(1.1),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                background_overlay_color: Some("blue".to_owned()),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                background_overlay_opacity: Some(1.1),
                ..NativeWindowConfig::default()
            },
            NativeWindowConfig {
                cursor_shape: Some("caret".to_owned()),
                ..NativeWindowConfig::default()
            },
        ] {
            let mut options = AppOptions::parse([
                "--window".to_owned(),
                "--window-config".to_owned(),
                "/configs/window.v1.json".to_owned(),
            ])
            .unwrap();

            assert!(options
                .apply_native_window_config_defaults(
                    |_| None,
                    || unreachable!("explicit window config should not use default path"),
                    |_| Ok(Some(config.clone())),
                )
                .is_err());
        }
    }

    #[test]
    fn app_options_native_window_config_explicit_missing_errors() {
        let mut options = AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "/missing/window.v1.json".to_owned(),
        ])
        .unwrap();

        assert!(options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("explicit window config should not use default path"),
                |_| Ok(None),
            )
            .is_err());
    }

    #[test]
    fn app_options_no_window_config_skips_config_loading() {
        let mut options =
            AppOptions::parse(["--window".to_owned(), "--no-window-config".to_owned()]).unwrap();

        options
            .apply_native_window_config_defaults(
                |_| None,
                || unreachable!("disabled window config should not use default path"),
                |_| unreachable!("disabled window config should not load config"),
            )
            .unwrap();

        assert_eq!(options.font_family, None);
        assert_eq!(options.font_size, None);
        assert!(options.font_paths.is_empty());
        assert_eq!(options.cwd, None);
        assert_eq!(options.window_title, None);
        assert_eq!(options.program, None);
        assert!(options.args.is_empty());
        assert!(options.launch_env.is_empty());
    }

    #[test]
    fn read_native_window_config_reads_json_and_rejects_unknown_fields() {
        let root = unique_temp_dir("witty-window-config");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("window.v1.json");
        let unknown_path = root.join("unknown.v1.json");
        std::fs::write(
            &config_path,
            r##"{
                "window_title": "Project Shell",
                "program": "tmux",
                "args": ["new-session", "-A"],
                "env": {"WITTY_SESSION": "daily", "TERM": "xterm-witty"},
                "font_family": "Hack Nerd Font",
                "font_size": 18,
                "terminal_padding": 4,
                "background_opacity": 0.84,
                "background_image": "/images/background.png",
                "background_image_fit": "cover",
                "background_overlay_color": "#203040",
                "background_overlay_opacity": 0.18,
                "cursor_shape": "bar",
                "cursor_blink": false,
                "cursor_blink_rate": "slow",
                "cursor_style_source": "config",
                "font_paths": ["/fonts/Hack.ttf"],
                "cwd": "/work/project",
                "scrollback_lines": 12000,
                "mouse_selection_override": "disabled",
                "osc52_clipboard": "allow",
                "window_last_active_close": "close-window",
                "window_cols": 120,
                "window_rows": 36
            }"##,
        )
        .unwrap();
        std::fs::write(&unknown_path, r#"{"unknown": true}"#).unwrap();

        let config = read_native_window_config(&config_path).unwrap().unwrap();
        assert_eq!(config.window_title.as_deref(), Some("Project Shell"));
        assert_eq!(config.program.as_deref(), Some("tmux"));
        assert_eq!(config.args, ["new-session", "-A"]);
        assert_eq!(
            config.env,
            BTreeMap::from([
                ("TERM".to_owned(), "xterm-witty".to_owned()),
                ("WITTY_SESSION".to_owned(), "daily".to_owned()),
            ])
        );
        assert_eq!(config.font_family.as_deref(), Some("Hack Nerd Font"));
        assert_eq!(config.font_size, Some(18));
        assert_eq!(config.terminal_padding, Some(4));
        assert_eq!(config.background_opacity, Some(0.84));
        assert_eq!(
            config.background_image.as_deref(),
            Some(Path::new("/images/background.png"))
        );
        assert_eq!(config.background_image_fit.as_deref(), Some("cover"));
        assert_eq!(config.background_overlay_color.as_deref(), Some("#203040"));
        assert_eq!(config.background_overlay_opacity, Some(0.18));
        assert_eq!(config.cursor_shape.as_deref(), Some("bar"));
        assert_eq!(config.cursor_blink, Some(false));
        assert_eq!(config.cursor_blink_rate.as_deref(), Some("slow"));
        assert_eq!(config.cursor_style_source.as_deref(), Some("config"));
        assert_eq!(config.font_paths, vec![PathBuf::from("/fonts/Hack.ttf")]);
        assert_eq!(config.cwd.as_deref(), Some(Path::new("/work/project")));
        assert_eq!(config.scrollback_lines, Some(12000));
        assert_eq!(config.mouse_selection_override.as_deref(), Some("disabled"));
        assert_eq!(config.osc52_clipboard.as_deref(), Some("allow"));
        assert_eq!(
            config.window_last_active_close.as_deref(),
            Some("close-window")
        );
        assert_eq!(config.window_cols, Some(120));
        assert_eq!(config.window_rows, Some(36));
        assert!(read_native_window_config(&root.join("missing.v1.json"))
            .unwrap()
            .is_none());
        assert!(read_native_window_config(&unknown_path).is_err());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn app_options_parse_window_last_active_close_fallback_local_session() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--window-last-active-close".to_owned(),
            "fallback-local-session".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(
            options.window_smoke.last_active_close_policy,
            WindowLastActiveClosePolicy::FallbackLocalSession
        );
    }

    #[test]
    fn cwd_home_expansion_accepts_tilde_and_rejects_user_tilde() {
        assert_eq!(
            expand_cwd_home_path(PathBuf::from("~"), "cwd", || Some(OsString::from(
                "/home/alice"
            )))
            .unwrap(),
            PathBuf::from("/home/alice")
        );
        assert_eq!(
            expand_cwd_home_path(PathBuf::from("~/src"), "cwd", || {
                Some(OsString::from("/home/alice"))
            })
            .unwrap(),
            PathBuf::from("/home/alice/src")
        );
        assert_eq!(
            expand_cwd_home_path(PathBuf::from("./src"), "cwd", || None).unwrap(),
            PathBuf::from("./src")
        );
        assert!(expand_cwd_home_path(PathBuf::from("~/src"), "cwd", || None).is_err());
        assert!(
            expand_cwd_home_path(PathBuf::from("~other/src"), "cwd", || {
                Some(OsString::from("/home/alice"))
            })
            .is_err()
        );
    }

    #[test]
    fn app_options_parse_all_window_last_active_close_policy_values() {
        for expected in WindowLastActiveClosePolicy::all() {
            let options = AppOptions::parse([
                "--window".to_owned(),
                "--window-last-active-close".to_owned(),
                expected.as_config_value().to_owned(),
            ])
            .unwrap();

            assert_eq!(options.mode, AppMode::Window);
            assert_eq!(options.window_smoke.last_active_close_policy, *expected);
        }
    }

    #[test]
    fn window_last_active_close_policy_config_values_are_stable() {
        assert_eq!(
            WindowLastActiveClosePolicy::Block.as_config_value(),
            "block"
        );
        assert_eq!(
            WindowLastActiveClosePolicy::CloseWindow.as_config_value(),
            "close-window"
        );
        assert_eq!(
            WindowLastActiveClosePolicy::FallbackLocalSession.as_config_value(),
            "fallback-local-session"
        );
        assert_eq!(
            WindowLastActiveClosePolicy::config_values(),
            &["block", "close-window", "fallback-local-session"]
        );
        assert_eq!(
            WindowLastActiveClosePolicy::all(),
            &[
                WindowLastActiveClosePolicy::Block,
                WindowLastActiveClosePolicy::CloseWindow,
                WindowLastActiveClosePolicy::FallbackLocalSession,
            ]
        );
        assert_eq!(
            WindowLastActiveClosePolicy::parse_config_value("block"),
            Some(WindowLastActiveClosePolicy::Block)
        );
        assert_eq!(
            WindowLastActiveClosePolicy::parse_config_value("close-window"),
            Some(WindowLastActiveClosePolicy::CloseWindow)
        );
        assert_eq!(
            WindowLastActiveClosePolicy::parse_config_value("fallback-local-session"),
            Some(WindowLastActiveClosePolicy::FallbackLocalSession)
        );
        assert_eq!(
            WindowLastActiveClosePolicy::parse_config_value("fallback"),
            None
        );
        assert_eq!(
            parse_window_last_active_close_policy(
                WindowLastActiveClosePolicy::Block.as_config_value()
            )
            .unwrap(),
            WindowLastActiveClosePolicy::Block
        );
        assert_eq!(
            parse_window_last_active_close_policy(
                WindowLastActiveClosePolicy::CloseWindow.as_config_value()
            )
            .unwrap(),
            WindowLastActiveClosePolicy::CloseWindow
        );
        assert_eq!(
            parse_window_last_active_close_policy(
                WindowLastActiveClosePolicy::FallbackLocalSession.as_config_value()
            )
            .unwrap(),
            WindowLastActiveClosePolicy::FallbackLocalSession
        );
    }

    #[test]
    fn window_last_active_close_policy_all_matches_config_values() {
        let all_values: Vec<_> = WindowLastActiveClosePolicy::all()
            .iter()
            .map(|policy| policy.as_config_value())
            .collect();

        assert_eq!(all_values, WindowLastActiveClosePolicy::config_values());
        for value in WindowLastActiveClosePolicy::config_values() {
            assert_eq!(
                WindowLastActiveClosePolicy::parse_config_value(value)
                    .map(WindowLastActiveClosePolicy::as_config_value),
                Some(*value)
            );
        }
    }

    #[test]
    fn window_last_active_close_policy_error_lists_config_values() {
        let error = parse_window_last_active_close_policy("fallback").unwrap_err();
        let message = error.to_string();

        assert!(message.contains("--window-last-active-close must be one of"));
        for value in WindowLastActiveClosePolicy::config_values() {
            assert!(
                message.contains(value),
                "missing last-active-close value {value:?} in {message:?}"
            );
        }
    }

    #[test]
    fn app_options_parse_font_family_trims_outer_whitespace() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--font-family".to_owned(),
            "  FiraCode Nerd Font  ".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.font_family.as_deref(), Some("FiraCode Nerd Font"));
    }

    #[test]
    fn app_options_parse_window_font_env_defaults() {
        let expected_paths = vec![
            PathBuf::from("/fonts/JetBrainsMonoNerdFont-Regular.ttf"),
            PathBuf::from("/fonts/SymbolsNerdFontMono-Regular.ttf"),
        ];
        let env_paths = std::env::join_paths(expected_paths.iter()).unwrap();
        let options = app_options_parse_with_env(
            ["--window"],
            vec![
                (
                    WITTY_FONT_FAMILY_ENV,
                    std::ffi::OsString::from("  JetBrainsMono Nerd Font  "),
                ),
                (WITTY_FONT_PATHS_ENV, env_paths),
            ],
        )
        .unwrap();

        assert_eq!(
            options.font_family.as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(options.font_paths, expected_paths);
    }

    #[test]
    fn app_options_parse_window_font_cli_overrides_env_defaults() {
        let env_paths = std::env::join_paths([PathBuf::from("/env/Symbols.ttf")]).unwrap();
        let options = app_options_parse_with_env(
            [
                "--window",
                "--font-family",
                "Hack Nerd Font",
                "--font-path",
                "/cli/HackNerdFont-Regular.ttf",
            ],
            vec![
                (
                    WITTY_FONT_FAMILY_ENV,
                    std::ffi::OsString::from("JetBrainsMono Nerd Font"),
                ),
                (WITTY_FONT_PATHS_ENV, env_paths),
            ],
        )
        .unwrap();

        assert_eq!(options.font_family.as_deref(), Some("Hack Nerd Font"));
        assert_eq!(
            options.font_paths,
            vec![PathBuf::from("/cli/HackNerdFont-Regular.ttf")]
        );
    }

    #[test]
    fn app_options_parse_font_env_is_window_only() {
        let options = app_options_parse_with_env(
            ["--web"],
            vec![
                (WITTY_FONT_FAMILY_ENV, std::ffi::OsString::from("   ")),
                (WITTY_FONT_PATHS_ENV, std::ffi::OsString::new()),
            ],
        )
        .unwrap();

        assert_eq!(options.mode, AppMode::Web);
        assert_eq!(options.font_family, None);
        assert!(options.font_paths.is_empty());
    }

    #[test]
    fn app_options_parse_invalid_font_env_errors_for_window() {
        assert!(app_options_parse_with_env(
            ["--window"],
            vec![(WITTY_FONT_FAMILY_ENV, std::ffi::OsString::from("   "))],
        )
        .is_err());
        assert!(app_options_parse_with_env(
            ["--window"],
            vec![(WITTY_FONT_PATHS_ENV, std::ffi::OsString::new())],
        )
        .is_err());
    }

    #[test]
    fn app_options_parse_native_osc52_clipboard_policy() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--osc52-clipboard".to_owned(),
            "allow".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(options.osc52_clipboard_policy, Osc52ClipboardPolicy::Allow);

        let options = AppOptions::parse([
            "--window".to_owned(),
            "--osc52-clipboard".to_owned(),
            "confirm".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            options.osc52_clipboard_policy,
            Osc52ClipboardPolicy::Confirm
        );
    }

    #[test]
    fn app_options_parse_cursor_shape_and_blink() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--cursor-shape".to_owned(),
            "vertical".to_owned(),
            "--cursor-blink".to_owned(),
            "no".to_owned(),
            "--cursor-blink-rate".to_owned(),
            "variable".to_owned(),
            "--cursor-style-source".to_owned(),
            "config".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.cursor_shape, CursorShape::Bar);
        assert!(!options.cursor_blink);
        assert_eq!(options.cursor_blink_rate, CursorBlinkRate::Variable);
        assert_eq!(options.cursor_style_source, CursorStyleSource::Config);

        let underline = AppOptions::parse([
            "--window".to_owned(),
            "--cursor-shape".to_owned(),
            "horizontal".to_owned(),
        ])
        .unwrap();
        assert_eq!(underline.cursor_shape, CursorShape::Underline);
        assert!(underline.cursor_blink);
        assert_eq!(underline.cursor_blink_rate, CursorBlinkRate::Normal);
        assert_eq!(underline.cursor_style_source, CursorStyleSource::Program);
    }

    #[test]
    fn app_options_parse_native_window_launch_command() {
        let options = AppOptions::parse([
            "--window".to_owned(),
            "--program".to_owned(),
            "/bin/zsh".to_owned(),
            "--arg".to_owned(),
            "-l".to_owned(),
            "--arg".to_owned(),
            "-c".to_owned(),
            "--cwd".to_owned(),
            "/work/project".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Window);
        assert_eq!(options.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(options.args, ["-l", "-c"]);
        assert_eq!(options.cwd.as_deref(), Some(Path::new("/work/project")));
        assert!(options.launcher_args.is_empty());
    }

    #[test]
    fn app_options_parse_web_launcher_args() {
        let options = AppOptions::parse([
            "--web".to_owned(),
            "--web-root".to_owned(),
            "target/witty-web-smoke".to_owned(),
            "--program".to_owned(),
            "/bin/sh".to_owned(),
            "--arg".to_owned(),
            "-lc".to_owned(),
            "--arg".to_owned(),
            "printf ok".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
            "--ssh-profile-id".to_owned(),
            "prod".to_owned(),
            "--profile-picker".to_owned(),
            "--profile-picker-import-openssh".to_owned(),
            "picker_ssh_config".to_owned(),
            "--profile-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--open-browser".to_owned(),
            "--mouse-selection-override".to_owned(),
            "disabled".to_owned(),
            "--scrollback-lines".to_owned(),
            "12000".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.mode, AppMode::Web);
        assert_eq!(options.wasm_plugins, Vec::<PathBuf>::new());
        assert_eq!(options.program.as_deref(), Some("/bin/sh"));
        assert_eq!(options.args, ["-lc", "printf ok"]);
        assert_eq!(
            options.launcher_args,
            [
                "--web-root",
                "target/witty-web-smoke",
                "--program",
                "/bin/sh",
                "--arg",
                "-lc",
                "--arg",
                "printf ok",
                "--ssh-profile-json",
                "profile.json",
                "--profile-store",
                "profiles.v1.json",
                "--ssh-profile-id",
                "prod",
                "--profile-picker",
                "--profile-picker-import-openssh",
                "picker_ssh_config",
                "--profile-import-openssh",
                "ssh_config",
                "--open-browser",
                "--mouse-selection-override",
                "disabled",
                "--scrollback-lines",
                "12000",
            ]
        );
    }

    #[test]
    fn profile_store_cli_parse_list_and_add_modes() {
        let list = AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(list.mode, AppMode::ProfileStore);
        assert!(list.launcher_args.is_empty());
        assert_eq!(
            list.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::List,
                store_path: Some(PathBuf::from("profiles.v1.json")),
            })
        );

        let add = AppOptions::parse([
            "--profile-store-add".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
            "--set-default".to_owned(),
        ])
        .unwrap();

        assert_eq!(add.mode, AppMode::ProfileStore);
        assert!(add.launcher_args.is_empty());
        assert_eq!(
            add.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::Add {
                    profile_json: PathBuf::from("profile.json"),
                    default_policy: ProfileStoreDefaultPolicy::SetToAdded,
                },
                store_path: None,
            })
        );
    }

    #[test]
    fn profile_store_cli_parse_update_remove_and_default_modes() {
        let update = AppOptions::parse([
            "--profile-store-update".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            update.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::Update {
                    profile_json: PathBuf::from("profile.json"),
                },
                store_path: None,
            })
        );

        let remove = AppOptions::parse([
            "--profile-store-remove".to_owned(),
            "prod".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            remove.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::Remove {
                    id: "prod".to_owned(),
                },
                store_path: Some(PathBuf::from("profiles.v1.json")),
            })
        );

        let check_launch = AppOptions::parse([
            "--profile-store-check-launch".to_owned(),
            "prod".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            check_launch.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::CheckLaunch {
                    id: "prod".to_owned(),
                },
                store_path: Some(PathBuf::from("profiles.v1.json")),
            })
        );

        let set_default =
            AppOptions::parse(["--profile-store-set-default".to_owned(), "prod".to_owned()])
                .unwrap();
        assert_eq!(
            set_default.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::SetDefault {
                    id: "prod".to_owned(),
                },
                store_path: None,
            })
        );

        let clear_default =
            AppOptions::parse(["--profile-store-clear-default".to_owned()]).unwrap();
        assert_eq!(
            clear_default.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::ClearDefault,
                store_path: None,
            })
        );
    }

    #[test]
    fn profile_store_import_preview_cli_parse_mode() {
        let preview = AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(preview.mode, AppMode::ProfileStore);
        assert!(preview.launcher_args.is_empty());
        assert_eq!(
            preview.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSshPreview {
                    config_path: PathBuf::from("ssh_config"),
                },
                store_path: Some(PathBuf::from("profiles.v1.json")),
            })
        );

        let confirmed = AppOptions::parse([
            "--profile-store-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--confirm".to_owned(),
            "--conflict".to_owned(),
            "replace".to_owned(),
            "--import-profile-id".to_owned(),
            "prod".to_owned(),
            "--import-profile-id".to_owned(),
            "staging".to_owned(),
            "--profile-store".to_owned(),
            "profiles.v1.json".to_owned(),
        ])
        .unwrap();

        assert_eq!(confirmed.mode, AppMode::ProfileStore);
        assert!(confirmed.launcher_args.is_empty());
        assert_eq!(
            confirmed.profile_store,
            Some(ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: PathBuf::from("ssh_config"),
                    selection: OpenSshImportSelection::profile_ids(["prod", "staging"]),
                    conflict_policy: OpenSshImportConflictPolicy::Replace,
                },
                store_path: Some(PathBuf::from("profiles.v1.json")),
            })
        );
    }

    #[test]
    fn profile_store_cli_parse_rejects_invalid_combinations() {
        assert!(AppOptions::parse(["--profile-store-add".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--web".to_owned(), "--profile-store-list".to_owned(),]).is_err()
        );
        assert!(
            AppOptions::parse(["--profile-store-list".to_owned(), "--web".to_owned(),]).is_err()
        );
        assert!(AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--open-browser".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--set-default".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-add".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
            "--ssh-profile-id".to_owned(),
            "prod".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--profile-store-add".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--profile-store-update".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-update".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
            "--set-default".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--profile-store-remove".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-remove".to_owned(),
            "prod".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--profile-store-check-launch".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-check-launch".to_owned(),
            "prod".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--profile-store-import-openssh-preview".to_owned()]).is_err());
        assert!(AppOptions::parse(["--profile-store-import-openssh".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh".to_owned(),
            "ssh_config".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--confirm".to_owned(),
            "--conflict".to_owned(),
            "merge".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--confirm".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh".to_owned(),
            "ssh_config".to_owned(),
            "--confirm".to_owned(),
            "--set-default".to_owned(),
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--profile-store-list".to_owned(), "--confirm".to_owned(),])
                .is_err()
        );
        assert!(AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--conflict".to_owned(),
            "reject".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-list".to_owned(),
            "--import-profile-id".to_owned(),
            "prod".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--confirm".to_owned()]).is_err());
        assert!(AppOptions::parse(["--conflict".to_owned(), "reject".to_owned(),]).is_err());
        assert!(AppOptions::parse(["--import-profile-id".to_owned(), "prod".to_owned(),]).is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--ssh-profile-json".to_owned(),
            "profile.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--set-default".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--web".to_owned(),
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--pty-smoke".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--open-browser".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--profile-store-import-openssh-preview".to_owned(),
            "ssh_config".to_owned(),
            "--window-command-palette".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn profile_store_cli_list_is_redacted() {
        let root = unique_temp_dir("witty-profile-store-cli-list");
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user("alice");
        prod.credential = witty_transport::SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        prod.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        prod.openssh.extra_args.push("-vv".to_owned());
        prod.openssh.remote_command.push("uptime".to_owned());
        prod.tag("work");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-secret-prod".to_owned(),
        };
        vaulted.tag("secure");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, vaulted])
        };
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::List,
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();

        assert!(output.contains("default\tid\tname\tlaunchability\ttags\n"));
        assert!(output.contains("*\tprod\tProduction\tlaunchable\twork\n"));
        assert!(output.contains("\tvaulted\tVaulted\trequires-credential-resolver\tsecure\n"));
        assert!(!output.contains("prod.example.com"));
        assert!(!output.contains("alice"));
        assert!(!output.contains("prod_ed25519"));
        assert!(!output.contains("/home/alice/.ssh/config"));
        assert!(!output.contains("-vv"));
        assert!(!output.contains("uptime"));
        assert!(!output.contains("vault-secret-prod"));
        assert!(!output.contains("vault.example.com"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_cli_list_handles_missing_default_but_not_missing_explicit() {
        let root = unique_temp_dir("witty-profile-store-cli-missing");
        let default_path = root.join("default").join("profiles.v1.json");
        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::List,
                store_path: None,
            },
            || Ok(default_path.clone()),
        )
        .unwrap();

        assert_eq!(output, "default\tid\tname\tlaunchability\ttags\n");

        let explicit_missing = root.join("explicit.json");
        assert!(run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::List,
                store_path: Some(explicit_missing),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .is_err());
    }

    #[test]
    fn profile_store_cli_check_launch_is_redacted_and_fails_closed() {
        let root = unique_temp_dir("witty-profile-store-check-launch");
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user("alice");
        prod.target.jump_host = Some("bastion.example.com".to_owned());
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
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::CheckLaunch {
                    id: "prod".to_owned(),
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();

        assert!(output.contains("profile launch check: id=prod"));
        assert!(output.contains("launchability=launchable"));
        assert!(output.contains("default=true"));
        assert!(output.contains("identity_file=true"));
        assert!(output.contains("config_file=true"));
        assert!(output.contains("jump_host=true"));
        assert!(output.contains("extra_args=1"));
        assert!(output.contains("remote_command_args=1"));
        assert!(!output.contains("prod.example.com"));
        assert!(!output.contains("alice"));
        assert!(!output.contains("bastion.example.com"));
        assert!(!output.contains("prod_ed25519"));
        assert!(!output.contains("/home/alice/.ssh/config"));
        assert!(!output.contains("-vv"));
        assert!(!output.contains("uptime"));
        assert!(!output.contains("vault-secret-prod"));

        let vaulted_error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::CheckLaunch {
                    id: "vaulted".to_owned(),
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err()
        .to_string();
        assert!(vaulted_error.contains("not launchable"));
        assert!(!vaulted_error.contains("vault-secret-prod"));

        let missing_error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::CheckLaunch {
                    id: "missing".to_owned(),
                },
                store_path: Some(store_path),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err()
        .to_string();
        assert!(missing_error.contains("profile id missing not found"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_cli_add_creates_store_with_locked_edit_helper() {
        let root = unique_temp_dir("witty-profile-store-cli-add");
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let profile_path = root.join("profile.json");
        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.tag("work");
        std::fs::write(&profile_path, serde_json::to_string(&profile).unwrap()).unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::Add {
                    profile_json: profile_path,
                    default_policy: ProfileStoreDefaultPolicy::SetToAdded,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();

        assert!(output.contains("changed=true"));
        assert!(output.contains("profiles=1"));
        assert!(output.contains("default_changed=true"));
        assert!(output.contains("bytes="));
        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].id, "prod");
        assert_eq!(loaded.default_profile_id.as_deref(), Some("prod"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_cli_update_remove_and_default_mutations() {
        let root = unique_temp_dir("witty-profile-store-cli-edit");
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let profile_path = root.join("profile.json");
        let prod = SshProfile::new("prod", "Production", "prod.example.com");
        let staging = SshProfile::new("staging", "Staging", "staging.example.com");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, staging])
        };
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let mut updated = SshProfile::new("prod", "Production Updated", "new.example.com");
        updated.tag("updated");
        std::fs::write(&profile_path, serde_json::to_string(&updated).unwrap()).unwrap();
        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::Update {
                    profile_json: profile_path,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();
        assert!(output.contains("changed=true"));
        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.profile("prod").unwrap().name, "Production Updated");
        assert_eq!(loaded.default_profile_id.as_deref(), Some("prod"));

        run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::SetDefault {
                    id: "staging".to_owned(),
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();
        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.default_profile_id.as_deref(), Some("staging"));

        run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ClearDefault,
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();
        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.default_profile_id, None);

        run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::Remove {
                    id: "prod".to_owned(),
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();
        let loaded = read_profile_store(&store_path).unwrap();
        assert!(loaded.profile("prod").is_none());
        assert!(loaded.profile("staging").is_some());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_preview_output_is_redacted_and_skips_default_store() {
        let root = unique_temp_dir("witty-profile-store-import-preview-redacted");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let config_display = config_path.display().to_string();
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
    User alice
    IdentityFile /home/alice/.ssh/prod_ed25519
    RemoteCommand uptime
    ProxyCommand ssh bastion nc %h %p
"#,
        )
        .unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSshPreview {
                    config_path: config_path.clone(),
                },
                store_path: None,
            },
            || unreachable!("preview without an explicit store should not use default"),
        )
        .unwrap();

        assert!(output.contains("id\tname\tlaunchability\tconflict\twarnings\n"));
        assert!(output.contains("prod\tprod\tlaunchable\tnone\t3\n"));
        assert!(output.contains("# summary candidates=1 conflicts=0 warnings=3\n"));
        for sensitive in [
            "prod.example.com",
            "alice",
            "prod_ed25519",
            config_display.as_str(),
            "uptime",
            "bastion",
            "%h",
            "%p",
        ] {
            assert!(
                !output.contains(sensitive),
                "preview output leaked {sensitive}: {output}"
            );
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_preview_marks_explicit_store_conflicts_without_writes() {
        let root = unique_temp_dir("witty-profile-store-import-preview-conflict");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Existing Production",
            "existing.example.com",
        )]);
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        let before = std::fs::read_to_string(&store_path).unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSshPreview {
                    config_path: config_path.clone(),
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();
        let after = std::fs::read_to_string(&store_path).unwrap();

        assert_eq!(before, after);
        assert!(output.contains("prod\tprod\tlaunchable\texisting-profile-id\t0\n"));
        assert!(output.contains("# summary candidates=1 conflicts=1 warnings=0\n"));
        assert!(!output.contains("prod.example.com"));
        assert!(!output.contains("existing.example.com"));
        assert!(!output.contains(store_path.display().to_string().as_str()));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_preview_requires_existing_explicit_store_for_conflicts() {
        let root = unique_temp_dir("witty-profile-store-import-preview-missing-store");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();

        let error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSshPreview {
                    config_path: config_path.clone(),
                },
                store_path: Some(root.join("missing-profiles.v1.json")),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("read profile store"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_writes_all_candidates_with_redacted_output() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-all");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        let config_display = config_path.display().to_string();
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
    User alice
    IdentityFile /home/alice/.ssh/prod_ed25519
    RemoteCommand uptime
    ProxyCommand ssh bastion nc %h %p
"#,
        )
        .unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::all(),
                    conflict_policy: OpenSshImportConflictPolicy::Reject,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();

        assert!(output.contains("OpenSSH import applied: changed=true"));
        assert!(output.contains("profiles=1"));
        assert!(output.contains("default_changed=false"));
        assert!(output.contains("selected=1"));
        assert!(output.contains("added=1"));
        assert!(output.contains("replaced=0"));
        assert!(output.contains("warnings=3"));
        assert!(output.contains("bytes="));
        assert!(output.contains("created_parent_dir=false"));
        for sensitive in [
            "prod.example.com",
            "alice",
            "prod_ed25519",
            config_display.as_str(),
            "uptime",
            "bastion",
            "%h",
            "%p",
            "\"profiles\"",
        ] {
            assert!(
                !output.contains(sensitive),
                "confirmed import output leaked {sensitive}: {output}"
            );
        }

        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].id, "prod");
        assert_eq!(loaded.default_profile_id, None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_uses_default_store_and_selects_requested_ids() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-select");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let default_path = root.join("default").join("profiles.v1.json");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com

Host staging
    HostName staging.example.com
"#,
        )
        .unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::profile_ids(["staging"]),
                    conflict_policy: OpenSshImportConflictPolicy::Reject,
                },
                store_path: None,
            },
            || Ok(default_path.clone()),
        )
        .unwrap();

        assert!(output.contains("selected=1"));
        assert!(output.contains("added=1"));
        assert!(output.contains("profiles=1"));
        let loaded = read_profile_store(&default_path).unwrap();
        assert!(loaded.profile("prod").is_none());
        assert!(loaded.profile("staging").is_some());
        assert_eq!(loaded.default_profile_id, None);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_reject_conflict_preserves_store_bytes() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-reject");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();

        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing Production",
                "existing.example.com",
            )])
        };
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        let before = std::fs::read_to_string(&store_path).unwrap();

        let error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::all(),
                    conflict_policy: OpenSshImportConflictPolicy::Reject,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err();
        let after = std::fs::read_to_string(&store_path).unwrap();

        let error = format!("{error:#}");
        assert!(error.contains("OpenSSH import has 1 selected profile id conflicts"));
        assert_eq!(before, after);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_replace_preserves_default_and_adds_new_ids() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-replace");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName new-prod.example.com

Host dev
    HostName dev.example.com
"#,
        )
        .unwrap();

        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Existing Production",
                "old-prod.example.com",
            )])
        };
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let output = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::all(),
                    conflict_policy: OpenSshImportConflictPolicy::Replace,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap();

        assert!(output.contains("selected=2"));
        assert!(output.contains("added=1"));
        assert!(output.contains("replaced=1"));
        assert!(output.contains("default_changed=false"));
        assert!(!output.contains("new-prod.example.com"));
        assert!(!output.contains("old-prod.example.com"));
        assert!(!output.contains("dev.example.com"));
        let loaded = read_profile_store(&store_path).unwrap();
        assert_eq!(loaded.profiles.len(), 2);
        assert_eq!(loaded.default_profile_id.as_deref(), Some("prod"));
        assert_eq!(
            loaded.profile("prod").unwrap().target.host,
            "new-prod.example.com"
        );
        assert!(loaded.profile("dev").is_some());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_unknown_or_duplicate_selection_writes_nothing() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-selection-error");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let missing_store_path = root.join("missing").join("profiles.v1.json");
        let existing_store_path = root.join("profiles.v1.json");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "existing.example.com",
        )]);
        std::fs::write(&existing_store_path, store.to_pretty_json().unwrap()).unwrap();
        let before = std::fs::read_to_string(&existing_store_path).unwrap();

        let missing_error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::profile_ids(["missing"]),
                    conflict_policy: OpenSshImportConflictPolicy::Reject,
                },
                store_path: Some(missing_store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err();
        let missing_error = format!("{missing_error:#}");
        assert!(missing_error.contains("OpenSSH import selection contains unknown profile ids"));
        assert!(!missing_store_path.exists());

        let duplicate_error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::profile_ids(["prod", "prod"]),
                    conflict_policy: OpenSshImportConflictPolicy::Replace,
                },
                store_path: Some(existing_store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err();
        let after = std::fs::read_to_string(&existing_store_path).unwrap();

        let duplicate_error = format!("{duplicate_error:#}");
        assert!(duplicate_error
            .contains("OpenSSH import selection contains duplicate requested profile ids"));
        assert_eq!(before, after);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_import_confirmed_existing_lock_preserves_store() {
        let root = unique_temp_dir("witty-profile-store-import-confirmed-lock");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("ssh_config");
        let store_path = root.join("profiles.v1.json");
        let lock_path = root.join("profiles.v1.json.lock");
        std::fs::write(
            &config_path,
            r#"
Host prod
    HostName prod.example.com
"#,
        )
        .unwrap();
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "existing",
            "Existing",
            "existing.example.com",
        )]);
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();
        std::fs::write(&lock_path, "external lock\n").unwrap();
        let before = std::fs::read_to_string(&store_path).unwrap();

        let error = run_profile_store_cli_with_default_path(
            &ProfileStoreCliOptions {
                command: ProfileStoreCommand::ImportOpenSsh {
                    config_path: config_path.clone(),
                    selection: OpenSshImportSelection::all(),
                    conflict_policy: OpenSshImportConflictPolicy::Reject,
                },
                store_path: Some(store_path.clone()),
            },
            || unreachable!("explicit store path should not use default"),
        )
        .unwrap_err();
        let after = std::fs::read_to_string(&store_path).unwrap();

        assert!(error
            .to_string()
            .contains("profile store write lock already exists"));
        assert_eq!(before, after);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn app_options_reject_missing_plugin_path() {
        assert!(AppOptions::parse(["--wasm-plugin".to_owned()]).is_err());
        assert!(AppOptions::parse(["--plugin-dir".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-exit-after-ms".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-title".to_owned()]).is_err());
        assert!(AppOptions::parse(["--web".to_owned(), "--program".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window".to_owned(), "--program".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window".to_owned(), "--cwd".to_owned()]).is_err());
        assert!(AppOptions::parse(["--web".to_owned(), "--ssh-profile-json".to_owned()]).is_err());
        assert!(AppOptions::parse(["--web".to_owned(), "--profile-store".to_owned()]).is_err());
        assert!(AppOptions::parse(["--web".to_owned(), "--ssh-profile-id".to_owned()]).is_err());
        assert!(AppOptions::parse(["--profile-picker".to_owned()]).is_err());
        assert!(AppOptions::parse(["--profile-picker-import-openssh".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--profile-picker-import-openssh".to_owned(),
            "ssh_config".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--profile-import-openssh".to_owned()]).is_err());
        assert!(AppOptions::parse(["--program".to_owned(), "/bin/sh".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--program".to_owned(),
            "   ".to_owned()
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--window".to_owned(), "--arg".to_owned(), "-l".to_owned()])
                .is_err()
        );
        assert!(
            AppOptions::parse(["--web".to_owned(), "--arg".to_owned(), "-l".to_owned()]).is_err()
        );
        assert!(AppOptions::parse(["--cwd".to_owned(), "/work".to_owned()]).is_err());
        assert!(AppOptions::parse(["--env".to_owned()]).is_err());
        assert!(AppOptions::parse(["--env".to_owned(), "TERM=xterm".to_owned()]).is_err());
        assert!(
            AppOptions::parse(["--web".to_owned(), "--cwd".to_owned(), "/work".to_owned()])
                .is_err()
        );
        assert!(
            AppOptions::parse(["--window".to_owned(), "--cwd".to_owned(), "   ".to_owned(),])
                .is_err()
        );
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cwd".to_owned(),
            "/work".to_owned(),
            "--cwd".to_owned(),
            "/other".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--window".to_owned(), "--env".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--env".to_owned(),
            "NOVALUE".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--env".to_owned(),
            "=value".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--env".to_owned(),
            " TERM=value".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse(["--open-browser".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-startup-report".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-diagnostics".to_owned()]).is_err());
        assert!(AppOptions::parse(["--font-list-filter".to_owned()]).is_err());
        assert!(
            AppOptions::parse(["--window-exit-after-ms".to_owned(), "100".to_owned(),]).is_err()
        );
        assert!(AppOptions::parse(["--window-last-active-close".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-config".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-last-active-close".to_owned(),
            "fallback".to_owned(),
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--window-last-active-close".to_owned(), "block".to_owned(),])
                .is_err()
        );
        assert!(AppOptions::parse(["--window-title".to_owned(), "Project".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window-config-template".to_owned(),
            "--window-title".to_owned(),
            "Project".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window-config-default-path".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window-config-init".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--wittyrc-template".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--wittyrc-default-path".to_owned(),
            "--wittyrc".to_owned(),
            "/tmp/.wittyrc".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--wittyrc-init".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-title".to_owned(),
            "   ".to_owned()
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-title".to_owned(),
            "Project".to_owned(),
            "--window-title".to_owned(),
            "Other".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window-config".to_owned(),
            "/tmp/window.v1.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "   ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "/tmp/window.v1.json".to_owned(),
            "--window-config".to_owned(),
            "/tmp/other.v1.json".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-config".to_owned(),
            "/tmp/window.v1.json".to_owned(),
            "--no-window-config".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--wittyrc".to_owned()]).is_err());
        assert!(AppOptions::parse(["--wittyrc".to_owned(), "/tmp/.wittyrc".to_owned(),]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--wittyrc".to_owned(),
            "   ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--wittyrc".to_owned(),
            "/tmp/.wittyrc".to_owned(),
            "--wittyrc".to_owned(),
            "/tmp/other.wittyrc".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--wittyrc".to_owned(),
            "/tmp/.wittyrc".to_owned(),
            "--no-wittyrc".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--no-wittyrc".to_owned()]).is_err());
        assert!(
            AppOptions::parse(["--wittyrc-check".to_owned(), "--no-wittyrc".to_owned(),]).is_err()
        );
        assert!(AppOptions::parse(["--mouse-selection-override".to_owned()]).is_err());
        assert!(AppOptions::parse(["--scrollback-lines".to_owned()]).is_err());
        assert!(AppOptions::parse(["--font-family".to_owned()]).is_err());
        assert!(AppOptions::parse(["--font-size".to_owned()]).is_err());
        assert!(AppOptions::parse(["--terminal-padding".to_owned()]).is_err());
        assert!(AppOptions::parse(["--background-opacity".to_owned()]).is_err());
        assert!(AppOptions::parse(["--background-image".to_owned()]).is_err());
        assert!(AppOptions::parse(["--background-image-fit".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-shape".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-blink".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-blink-rate".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-style-source".to_owned()]).is_err());
        assert!(AppOptions::parse(["--font-path".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-cols".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-rows".to_owned()]).is_err());
        assert!(
            AppOptions::parse(["--mouse-selection-override".to_owned(), "raw".to_owned()]).is_err()
        );
        assert!(AppOptions::parse([
            "--mouse-selection-override".to_owned(),
            "disabled".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--scrollback-lines".to_owned(), "2500".to_owned()]).is_err());
        assert!(
            AppOptions::parse(["--font-family".to_owned(), "Hack Nerd Font".to_owned()]).is_err()
        );
        assert!(AppOptions::parse(["--font-size".to_owned(), "16".to_owned()]).is_err());
        assert!(AppOptions::parse(["--terminal-padding".to_owned(), "8".to_owned()]).is_err());
        assert!(AppOptions::parse(["--background-opacity".to_owned(), "0.8".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--background-image".to_owned(),
            "/images/witty.png".to_owned()
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--background-image-fit".to_owned(), "cover".to_owned()]).is_err()
        );
        assert!(AppOptions::parse(["--cursor-shape".to_owned(), "bar".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-blink".to_owned(), "false".to_owned()]).is_err());
        assert!(AppOptions::parse(["--cursor-blink-rate".to_owned(), "slow".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--cursor-style-source".to_owned(),
            "config".to_owned()
        ])
        .is_err());
        assert!(
            AppOptions::parse(["--font-path".to_owned(), "/fonts/Hack.ttf".to_owned()]).is_err()
        );
        assert!(AppOptions::parse(["--window-cols".to_owned(), "120".to_owned()]).is_err());
        assert!(AppOptions::parse(["--window-rows".to_owned(), "36".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-family".to_owned(),
            "   ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-path".to_owned(),
            "   ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-size".to_owned(),
            "large".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-size".to_owned(),
            "5".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-size".to_owned(),
            "97".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--terminal-padding".to_owned(),
            "wide".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--terminal-padding".to_owned(),
            "65".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--terminal-padding".to_owned(),
            "4".to_owned(),
            "--terminal-padding".to_owned(),
            "8".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-opacity".to_owned(),
            "clear".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-opacity".to_owned(),
            "1.2".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-opacity".to_owned(),
            "0.6".to_owned(),
            "--background-opacity".to_owned(),
            "0.8".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-image".to_owned(),
            "   ".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-image".to_owned(),
            "/images/one.png".to_owned(),
            "--background-image".to_owned(),
            "/images/two.png".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-image-fit".to_owned(),
            "contain".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--background-image-fit".to_owned(),
            "cover".to_owned(),
            "--background-image-fit".to_owned(),
            "scale-crop".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-shape".to_owned(),
            "caret".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-shape".to_owned(),
            "bar".to_owned(),
            "--cursor-shape".to_owned(),
            "block".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-blink".to_owned(),
            "maybe".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-blink".to_owned(),
            "true".to_owned(),
            "--cursor-blink".to_owned(),
            "false".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-blink-rate".to_owned(),
            "fast".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-blink-rate".to_owned(),
            "slow".to_owned(),
            "--cursor-blink-rate".to_owned(),
            "variable".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-style-source".to_owned(),
            "driver".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--cursor-style-source".to_owned(),
            "program".to_owned(),
            "--cursor-style-source".to_owned(),
            "config".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-cols".to_owned(),
            "wide".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-cols".to_owned(),
            "19".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-rows".to_owned(),
            "201".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-cols".to_owned(),
            "120".to_owned(),
            "--window-cols".to_owned(),
            "132".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--window-rows".to_owned(),
            "36".to_owned(),
            "--window-rows".to_owned(),
            "40".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-size".to_owned(),
            "14".to_owned(),
            "--font-size".to_owned(),
            "16".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--font-family".to_owned(),
            "Hack Nerd Font".to_owned(),
            "--font-family".to_owned(),
            "FiraCode Nerd Font".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--scrollback-lines".to_owned(),
            "many".to_owned(),
        ])
        .is_err());
        assert!(AppOptions::parse(["--osc52-clipboard".to_owned()]).is_err());
        assert!(AppOptions::parse(["--real-tui-smoke".to_owned()]).is_err());
        assert!(AppOptions::parse(["--osc52-clipboard".to_owned(), "allow".to_owned()]).is_err());
        assert!(AppOptions::parse([
            "--window".to_owned(),
            "--osc52-clipboard".to_owned(),
            "always".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn plugin_dir_discovery_returns_sorted_wasm_files() {
        let dir = unique_temp_dir("witty-plugin-discovery");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.wasm"), []).unwrap();
        std::fs::write(dir.join("a.wasm"), []).unwrap();
        std::fs::write(dir.join("ignored.txt"), []).unwrap();

        let plugins = discover_wasm_plugins(&dir).unwrap();

        assert_eq!(plugins, vec![dir.join("a.wasm"), dir.join("b.wasm")]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn incremental_smoke_tracks_row_reuse() {
        let stats = incremental_smoke_stats().unwrap();

        assert_eq!(stats[0].rebuilt_rows, 3);
        assert_eq!(stats[1].reused_rows, 2);
        assert_eq!(stats[1].rebuilt_rows, 1);
        assert_eq!(stats[2].reused_rows, 1);
        assert_eq!(stats[2].rebuilt_rows, 2);
    }

    #[test]
    fn renderer_backend_info_is_non_graphical_policy_report() {
        let info = renderer_backend_info_json();

        assert_eq!(info["renderer"], "wgpu");
        assert_eq!(info["opens_window"], false);
        assert_eq!(info["enumerates_adapter"], false);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(info["native_backend_policy"], "gl");
            assert_eq!(info["opengl_only"], true);
            assert_eq!(info["honors_wgpu_backend_env"], false);
        }
    }

    #[test]
    fn font_list_text_filters_sorts_and_deduplicates() {
        let families = vec![
            "Symbols Nerd Font Mono".to_owned(),
            "DejaVu Sans Mono".to_owned(),
            "JetBrainsMono Nerd Font".to_owned(),
            "JetBrainsMono Nerd Font".to_owned(),
            "FiraCode Nerd Font".to_owned(),
        ];

        assert_eq!(
            font_list_text(families.clone(), Some("nerd")),
            "FiraCode Nerd Font\nJetBrainsMono Nerd Font\nSymbols Nerd Font Mono\n"
        );
        assert_eq!(
            font_list_text(families.clone(), Some("SANS")),
            "DejaVu Sans Mono\n"
        );
        assert_eq!(font_list_text(families, Some("missing")), "");
    }

    #[test]
    fn keyboard_protocol_diagnostics_reports_representative_sequences() {
        let report = keyboard_protocol_diagnostics_json();
        let cases = report["cases"].as_array().expect("cases array");
        let case_by_id = |id: &str| {
            cases
                .iter()
                .find(|case| case["id"] == id)
                .unwrap_or_else(|| panic!("missing diagnostic case {id}"))
        };

        assert_eq!(report["diagnostic"], "keyboard-protocol");
        assert_eq!(report["opensWindow"], false);
        assert_eq!(report["startsPty"], false);
        assert_eq!(case_by_id("legacy-ctrl-i")["bytesHex"], "09");
        assert_eq!(
            case_by_id("kitty-disambiguate-ctrl-i")["bytesEscaped"],
            "\\x1b[105;5u"
        );
        assert_eq!(
            case_by_id("kitty-event-ctrl-i")["bytesEscaped"],
            "\\x1b[105;5:1u"
        );
        assert_eq!(
            case_by_id("kitty-associated-shift-a-repeat")["bytesEscaped"],
            "\\x1b[97:65;2:2;65u"
        );
        assert_eq!(
            case_by_id("kitty-keypad-left-numlock-off")["keypadKey"],
            "KP_LEFT"
        );
        assert_eq!(
            case_by_id("kitty-right-ctrl-release")["modifierKey"],
            "RIGHT_CONTROL"
        );
    }

    #[test]
    fn keyboard_protocol_capture_formats_grouped_bytes() {
        let mut reader = std::io::Cursor::new(b"\x1b[A".to_vec());
        let bytes = read_keyboard_protocol_capture_event(&mut reader).unwrap();

        assert_eq!(bytes, b"\x1b[A");
        assert_eq!(
            keyboard_protocol_capture_event_line(7, &bytes),
            "event 7: bytesHex=1b 5b 41 bytesEscaped=\\x1b[A"
        );
    }

    #[test]
    fn renderer_no_surface_diagnostics_reports_frame_stats_without_driver_contact() {
        let info = renderer_no_surface_diagnostics_json();

        assert_eq!(info["diagnostic"], "renderer-no-surface");
        assert_eq!(info["opensWindow"], false);
        assert_eq!(info["createsSurface"], false);
        assert_eq!(info["requestsAdapter"], false);
        assert_eq!(info["createsDevice"], false);
        assert_eq!(info["frameCount"], 2);
        assert_eq!(info["frames"][0]["label"], "empty");
        assert_eq!(info["frames"][1]["label"], "populated");
        assert!(info["frames"][1]["glyphRuns"].as_u64().unwrap() >= 1);
        assert!(info["frames"][1]["selectionRects"].as_u64().unwrap() >= 1);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(info["nativeBackend"]["native_backend_policy"], "gl");
            assert_eq!(info["nativeBackend"]["opengl_only"], true);
        }
    }

    #[test]
    fn incremental_smoke_stats_json_includes_frame_stats() {
        let stats = vec![FrameStats {
            visible_rows: 3,
            visible_cols: 8,
            background_runs: 4,
            glyph_runs: 2,
            glyph_chars: 130,
            glyph_prepare_batches: 2,
            max_glyph_run_chars: 120,
            selection_rects: 1,
            search_highlight_rects: 1,
            hyperlink_hover_rects: 1,
            hyperlink_underline_rects: 1,
            text_decoration_rects: 1,
            ime_preedit_rects: 1,
            search_active_visible: true,
            cursor_visible: true,
            rect_vertices: 18,
            rect_vertex_capacity: 32,
            full_damage: false,
            damage_regions: 1,
            reused_rows: 2,
            rebuilt_rows: 1,
        }];

        let json = incremental_smoke_stats_json(&stats).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let frame = &value["frames"][0];

        assert_eq!(value["smoke"], "incremental");
        assert_eq!(value["frameCount"], 1);
        assert_eq!(frame["label"], "first");
        assert_eq!(frame["visibleRows"], 3);
        assert_eq!(frame["visibleCols"], 8);
        assert_eq!(frame["backgroundRuns"], 4);
        assert_eq!(frame["glyphRuns"], 2);
        assert_eq!(frame["glyphChars"], 130);
        assert_eq!(frame["glyphPrepareBatches"], 2);
        assert_eq!(frame["maxGlyphRunChars"], 120);
        assert_eq!(frame["selectionRects"], 1);
        assert_eq!(frame["searchHighlightRects"], 1);
        assert_eq!(frame["hyperlinkHoverRects"], 1);
        assert_eq!(frame["hyperlinkUnderlineRects"], 1);
        assert_eq!(frame["textDecorationRects"], 1);
        assert_eq!(frame["imePreeditRects"], 1);
        assert_eq!(frame["searchActiveVisible"], true);
        assert_eq!(frame["cursorVisible"], true);
        assert_eq!(frame["rectVertices"], 18);
        assert_eq!(frame["rectVertexCapacity"], 32);
        assert_eq!(frame["fullDamage"], false);
        assert_eq!(frame["damageRegions"], 1);
        assert_eq!(frame["reusedRows"], 2);
        assert_eq!(frame["rebuiltRows"], 1);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}
