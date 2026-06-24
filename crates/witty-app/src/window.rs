use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey};
use winit::monitor::MonitorHandle;
use winit::window::{
    CursorIcon, Fullscreen, ImePurpose, ResizeDirection, Window, WindowAttributes, WindowId,
};
use witty_core::{
    encode_terminal_focus_event, encode_terminal_mouse_event, paste_payload, terminal_char_width,
    BasicTerminal, CellFlags, CellPoint, CellRange, CursorShape, CursorState, FocusEventKind,
    GridSize, HyperlinkId, MouseButtonCode, MouseEventKind, MouseModifiers, Osc52ClipboardPolicy,
    PixelMousePosition, RenderSnapshot, Rgba, SearchHighlight, TerminalClipboardSelection,
    TerminalClipboardWrite, TerminalHostAction, TerminalHyperlink, TerminalInputModes,
    TerminalMouseEvent,
};
use witty_launcher::open_external_url;
use witty_plugin_api::{CommandRegistration, PluginAction, PluginEvent};
use witty_render_wgpu::{
    native_wgpu_backend_policy, CellMetrics, FramePlan, FramePlanner, FrameStats, GlyphBatchItem,
    PixelPoint, PixelSize, RectBatchItem, RendererBackgroundImage, RendererBackgroundImageFit,
    RendererFontConfig, RendererVisualConfig, WgpuRectRenderer, DEFAULT_TERMINAL_FONT_SIZE,
};
use witty_ui::{
    apply_command_block_action_menu_overlay, apply_command_block_command,
    apply_command_block_folded_frame_remap_with_anchors,
    apply_command_block_gutter_hover_overlay_with_anchors,
    apply_command_block_gutter_overlay_with_anchors,
    apply_command_block_selection_overlay_with_anchors,
    apply_command_block_status_label_overlay_with_anchors, apply_ime_preedit_overlay,
    command_block_command_registrations, command_block_copy_target,
    command_block_folded_visual_pixel_to_terminal_pixel_with_anchors,
    command_block_gutter_hit_test_with_anchors, search_command_registrations,
    selected_command_block_copy_text, CommandBlockActionMenu, CommandBlockCopyTarget,
    CommandPalette, DismissedPendingProfileAction, ImeComposition,
    PendingProfileActionConfirmation, PendingProfileActionKey, PendingProfileActionReview,
    PendingProfileLaunchRequestStatus, ResolvedPendingProfileActionPtyConfig,
    ShellIntegrationState, TerminalApp, TerminalSearch, COMMAND_BLOCK_ACTION_MENU_COMMAND_ID,
    COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID, COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID,
    COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID, SEARCH_CLOSE_COMMAND_ID, SEARCH_NEXT_COMMAND_ID,
    SEARCH_OPEN_COMMAND_ID, SEARCH_PREVIOUS_COMMAND_ID,
};

use crate::{
    install_wasm_plugins,
    update_state::{
        default_install_state_path, default_restart_state_path, installed_update_status,
        now_unix_ms, plan_restart_execution, read_installed_build_marker, running_build_identity,
        spawn_restart_plan, write_restart_snapshot_atomic, InstalledBuildMarkerV1,
        RestartExecutionPlan, RestartLaunchConfigV1, RestartProfileMetadataV1, RestartSnapshotV1,
        RestartTabKindV1, RestartTabModeV1, RestartTabV1, RestartWindowStateV1,
        RunningBuildIdentity,
    },
    BuiltInCommandsPlugin, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE,
};
use witty_transport::{
    default_profile_store_path, read_profile_store, LocalPtyConfig, LocalPtyTransport,
    ProfileStoreV1, SshProfileLaunchability, TerminalTransport, TransportEvent,
};

const DOUBLE_CLICK_MAX_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_MAX_CELL_DISTANCE: u16 = 1;
pub(crate) const DEFAULT_WINDOW_TITLE: &str = "Witty Rust/wgpu Prototype";
#[cfg(target_os = "linux")]
const WITTY_LINUX_APP_ID: &str = "dev.witty.Witty";
const SEARCH_SCROLL_BUFFER_ROWS: u16 = 1;
const SYNCHRONIZED_OUTPUT_TIMEOUT: Duration = Duration::from_millis(150);
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const TEXT_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const FULLSCREEN_MONITOR_SIZE_TOLERANCE_PX: u32 = 8;
const FULLSCREEN_REAFFIRM_INTERVAL: Duration = Duration::from_millis(120);
const FULLSCREEN_REAFFIRM_DURATION: Duration = Duration::from_millis(1_200);
const WINDOWED_RESTORE_MAX_ATTEMPTS: u8 = 8;
const NATIVE_LOCAL_SESSION_SOURCE_PLUGIN: &str = "witty-local";
const INSTALLED_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const RESTART_BUTTON_LABEL: &str = "Restart to update";
const WITTY_NEW_LOCAL_SHELL_COMMAND_ID: &str = "witty.new-local-shell";
const CUSTOM_TITLEBAR_HEIGHT: f32 = 34.0;
const CUSTOM_TITLEBAR_BUTTON_SIZE: f32 = 28.0;
const CUSTOM_TITLEBAR_BUTTON_GAP: f32 = 4.0;
const CUSTOM_TITLEBAR_EDGE_PADDING: f32 = 6.0;
const CUSTOM_TITLEBAR_MENU_WIDTH: f32 = 220.0;
const CUSTOM_TITLEBAR_MENU_ITEM_HEIGHT: f32 = 30.0;
const CUSTOM_TITLEBAR_FULLSCREEN_REVEAL_ZONE: f64 = 4.0;
const CUSTOM_TITLEBAR_FULLSCREEN_HIDE_DELAY: Duration = Duration::from_millis(900);
const CUSTOM_RESIZE_BORDER_WIDTH: f64 = 6.0;
const ABOUT_STATUS_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowSmokeOptions {
    pub open_command_palette: bool,
    pub show_diagnostics: bool,
    pub report_startup: bool,
    pub exit_after: Option<Duration>,
    pub last_active_close_policy: WindowLastActiveClosePolicy,
    pub initial_size: Option<GridSize>,
}

#[derive(Debug)]
enum NativeWindowEvent {
    BackgroundImageReady,
}

#[derive(Debug)]
struct BackgroundImageLoadResult {
    generation: u64,
    path: PathBuf,
    target_size: PhysicalSize<u32>,
    image: Option<RendererBackgroundImage>,
    elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowLastActiveClosePolicy {
    Block,
    #[default]
    CloseWindow,
    FallbackLocalSession,
}

impl WindowLastActiveClosePolicy {
    #[cfg(test)]
    pub const ALL: &'static [Self] = &[Self::Block, Self::CloseWindow, Self::FallbackLocalSession];

    #[cfg(test)]
    pub fn all() -> &'static [Self] {
        Self::ALL
    }

    pub fn config_values() -> &'static [&'static str] {
        &["block", "close-window", "fallback-local-session"]
    }

    pub fn parse_config_value(value: &str) -> Option<Self> {
        match value {
            "block" => Some(Self::Block),
            "close-window" => Some(Self::CloseWindow),
            "fallback-local-session" => Some(Self::FallbackLocalSession),
            _ => None,
        }
    }

    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::CloseWindow => "close-window",
            Self::FallbackLocalSession => "fallback-local-session",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum NativeSessionTabPosition {
    #[default]
    Top,
    Bottom,
}

impl NativeSessionTabPosition {
    pub(crate) fn config_values() -> &'static [&'static str] {
        &["top", "bottom"]
    }

    pub(crate) fn parse_config_value(value: &str) -> Option<Self> {
        match value {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum NativeSessionTabLabelStyle {
    #[default]
    Index,
    Profile,
    IndexProfile,
}

impl NativeSessionTabLabelStyle {
    pub(crate) fn config_values() -> &'static [&'static str] {
        &["index", "profile", "index-profile"]
    }

    pub(crate) fn parse_config_value(value: &str) -> Option<Self> {
        match value {
            "index" => Some(Self::Index),
            "profile" => Some(Self::Profile),
            "index-profile" | "index_profile" => Some(Self::IndexProfile),
            _ => None,
        }
    }

    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Profile => "profile",
            Self::IndexProfile => "index-profile",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct NativeSessionTabDisplayPolicy {
    pub show_single: bool,
    pub show_multiple: bool,
}

impl NativeSessionTabDisplayPolicy {
    fn should_show(self, session_count: usize) -> bool {
        match session_count {
            0 => false,
            1 => self.show_single,
            _ => self.show_multiple,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextInputTarget {
    Terminal,
    Search,
    CommandPalette,
    SessionSwitcher,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeFontSizeAction {
    Increase,
    Decrease,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeTitleBarButton {
    Menu,
    NewSession,
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeTitleBarMenuItem {
    NewSession,
    CommandPalette,
    Search,
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeTitleBarHit {
    Button(NativeTitleBarButton),
    MenuItem(NativeTitleBarMenuItem),
    DragRegion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NativeOverlayRect {
    origin: PixelPoint,
    size: PixelSize,
}

impl NativeOverlayRect {
    fn contains_point(self, point: PixelPoint) -> bool {
        point.x >= self.origin.x
            && point.x < self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y < self.origin.y + self.size.height
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeTitleBarOverlay<'a> {
    title: &'a str,
    window_width: f32,
    scale_factor: f32,
    visible: bool,
    menu_open: bool,
    hovered_hit: Option<NativeTitleBarHit>,
    maximized: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CursorBlinkKey {
    position: CellPoint,
    shape: CursorShape,
}

#[derive(Clone, Debug, Default)]
struct CursorBlinkState {
    key: Option<CursorBlinkKey>,
    visible_phase: bool,
    next_deadline: Option<Instant>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct NativeSessionSwitcher {
    selected_session_id: Option<NativeSessionId>,
}

impl NativeSessionSwitcher {
    fn is_open(&self) -> bool {
        self.selected_session_id.is_some()
    }

    fn open(&mut self, rows: &[NativeSessionTabRow], direction: i32) -> bool {
        if rows.len() < 2 {
            self.selected_session_id = None;
            return false;
        }

        let active_index = rows.iter().position(|row| row.active).unwrap_or(0);
        let selected_index = wrapping_row_index(active_index, direction, rows.len());
        self.selected_session_id = Some(rows[selected_index].session_id);
        true
    }

    fn move_selection(&mut self, rows: &[NativeSessionTabRow], direction: i32) -> bool {
        if rows.len() < 2 {
            self.selected_session_id = None;
            return false;
        }

        let current_index = self
            .selected_session_id
            .and_then(|id| rows.iter().position(|row| row.session_id == id))
            .or_else(|| rows.iter().position(|row| row.active))
            .unwrap_or(0);
        let selected_index = wrapping_row_index(current_index, direction, rows.len());
        self.selected_session_id = Some(rows[selected_index].session_id);
        true
    }

    fn close(&mut self) {
        self.selected_session_id = None;
    }
}

fn wrapping_row_index(current: usize, direction: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let len = len as i32;
    let next = current as i32 + direction;
    next.rem_euclid(len) as usize
}

impl CursorBlinkState {
    fn apply_to_frame(
        &mut self,
        frame: &mut FramePlan,
        cursor: CursorState,
        text_input_target: TextInputTarget,
        now: Instant,
    ) {
        let key = cursor_blink_key(cursor, text_input_target, frame.cursor.is_some());
        if self.key != key {
            self.key = key;
            self.visible_phase = true;
            self.next_deadline = key.and_then(|_| now.checked_add(CURSOR_BLINK_INTERVAL));
        } else if key.is_some() && self.next_deadline.is_none() {
            self.next_deadline = now.checked_add(CURSOR_BLINK_INTERVAL);
        }

        if key.is_none() {
            self.visible_phase = true;
            self.next_deadline = None;
        } else if !self.visible_phase {
            frame.cursor = None;
        }
    }

    fn toggle_if_due(
        &mut self,
        cursor: CursorState,
        text_input_target: TextInputTarget,
        now: Instant,
    ) -> bool {
        let key = cursor_blink_key(cursor, text_input_target, true);
        if self.key != key {
            self.key = key;
            self.visible_phase = true;
            self.next_deadline = key.and_then(|_| now.checked_add(CURSOR_BLINK_INTERVAL));
            return false;
        }

        let Some(deadline) = self.next_deadline else {
            return false;
        };
        if now < deadline {
            return false;
        }

        self.visible_phase = !self.visible_phase;
        self.next_deadline = now.checked_add(CURSOR_BLINK_INTERVAL);
        true
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.next_deadline
    }
}

#[derive(Clone, Debug)]
struct TextBlinkState {
    active: bool,
    visible_phase: bool,
    next_deadline: Option<Instant>,
}

impl Default for TextBlinkState {
    fn default() -> Self {
        Self {
            active: false,
            visible_phase: true,
            next_deadline: None,
        }
    }
}

impl TextBlinkState {
    fn apply_to_snapshot(&mut self, snapshot: &RenderSnapshot, now: Instant) -> bool {
        let active = snapshot_contains_blinking_text(snapshot);
        if !active {
            self.active = false;
            self.visible_phase = true;
            self.next_deadline = None;
            return true;
        }

        if !self.active {
            self.active = true;
            self.visible_phase = true;
            self.next_deadline = now.checked_add(TEXT_BLINK_INTERVAL);
            return true;
        }

        if self.next_deadline.is_none() {
            self.next_deadline = now.checked_add(TEXT_BLINK_INTERVAL);
        }
        self.visible_phase
    }

    fn toggle_if_due(&mut self, now: Instant) -> bool {
        if !self.active {
            return false;
        }

        let Some(deadline) = self.next_deadline else {
            self.next_deadline = now.checked_add(TEXT_BLINK_INTERVAL);
            return false;
        };
        if now < deadline {
            return false;
        }

        self.visible_phase = !self.visible_phase;
        self.next_deadline = now.checked_add(TEXT_BLINK_INTERVAL);
        true
    }

    fn visible_phase(&self) -> bool {
        self.visible_phase
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.next_deadline
    }
}

fn cursor_blink_key(
    cursor: CursorState,
    text_input_target: TextInputTarget,
    frame_cursor_visible: bool,
) -> Option<CursorBlinkKey> {
    if text_input_target != TextInputTarget::Terminal
        || !frame_cursor_visible
        || !cursor.visible
        || !cursor.blink
    {
        return None;
    }
    Some(CursorBlinkKey {
        position: cursor.position,
        shape: cursor.shape,
    })
}

fn snapshot_contains_blinking_text(snapshot: &RenderSnapshot) -> bool {
    snapshot.rows.iter().any(|row| {
        row.cells
            .iter()
            .any(|cell| cell.style.flags.blink && !cell.style.flags.conceal)
    })
}

fn earliest_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeProfileActionDisplayStatus {
    PickProfile,
    Launchable,
    RequiresCredentialResolver,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionDisplayRow {
    pub key: PendingProfileActionKey,
    pub source_plugin: String,
    pub title: String,
    pub detail: String,
    pub reason: Option<String>,
    pub status: NativeProfileActionDisplayStatus,
    pub confirm_label: Option<String>,
    pub new_tab_label: Option<String>,
    pub dismiss_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeProfilePickerOptionStatus {
    Launchable,
    RequiresCredentialResolver,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfilePickerOptionRow {
    pub request_key: PendingProfileActionKey,
    pub profile_id: String,
    pub title: String,
    pub detail: String,
    pub status: NativeProfilePickerOptionStatus,
    pub select_label: Option<String>,
    pub new_tab_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionStartSuccessRow {
    pub key: PendingProfileActionKey,
    pub source_plugin: String,
    pub profile_id: String,
    pub title: String,
    pub detail: String,
    pub reason: Option<String>,
    pub dismiss_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionStartFailureRow {
    pub key: PendingProfileActionKey,
    pub source_plugin: String,
    pub profile_id: String,
    pub title: String,
    pub detail: String,
    pub reason: Option<String>,
    pub retry_label: String,
    pub dismiss_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeProfileActionOverlayTarget {
    Row,
    Confirm,
    ConfirmNewTab,
    Dismiss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionOverlayHit {
    pub key: PendingProfileActionKey,
    pub row_index: usize,
    pub target: NativeProfileActionOverlayTarget,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NativeProfileActionSnapshot {
    pub reviews: Vec<PendingProfileActionReview>,
    pub display_rows: Vec<NativeProfileActionDisplayRow>,
    pub start_success: Option<NativeProfileActionStartSuccessRow>,
    pub start_failure: Option<NativeProfileActionStartFailureRow>,
    pub picker_options: Vec<NativeProfilePickerOptionRow>,
    pub picker_requests: usize,
    pub launch_requests: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeProfileActionBridgeEvent {
    SnapshotRefreshed(NativeProfileActionSnapshot),
    #[allow(dead_code)]
    Dismissed {
        dismissed: DismissedPendingProfileAction,
        snapshot: NativeProfileActionSnapshot,
    },
    #[allow(dead_code)]
    Confirmed {
        resolved: ResolvedPendingProfileActionPtyConfig,
        snapshot: NativeProfileActionSnapshot,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeResolvedProfileActionKind {
    ProfilePicker,
    ProfileLaunch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeResolvedProfileActionHandoff {
    pub key: PendingProfileActionKey,
    pub kind: NativeResolvedProfileActionKind,
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub config: LocalPtyConfig,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NativeResolvedProfileActionHandoffQueue {
    pending: Vec<NativeResolvedProfileActionHandoff>,
}

impl NativeResolvedProfileActionHandoffQueue {
    pub(crate) fn push(&mut self, handoff: NativeResolvedProfileActionHandoff) {
        self.pending.push(handoff);
    }

    pub(crate) fn take_next(&mut self) -> Option<NativeResolvedProfileActionHandoff> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[NativeResolvedProfileActionHandoff] {
        &self.pending
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeResolvedProfileActionSessionPolicy {
    DeferStart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeProfileActionStartMode {
    ReplaceCurrentSession,
    #[allow(dead_code)]
    NewTab,
}

impl Default for NativeProfileActionStartMode {
    fn default() -> Self {
        Self::ReplaceCurrentSession
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionStartPlan {
    pub mode: NativeProfileActionStartMode,
    pub key: PendingProfileActionKey,
    pub kind: NativeResolvedProfileActionKind,
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub config: LocalPtyConfig,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NativeProfileActionStartPlanQueue {
    pending: Vec<NativeProfileActionStartPlan>,
}

impl NativeProfileActionStartPlanQueue {
    pub(crate) fn push(&mut self, plan: NativeProfileActionStartPlan) {
        self.pending.push(plan);
    }

    pub(crate) fn take_next(&mut self) -> Option<NativeProfileActionStartPlan> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    pub(crate) fn peek_next(&self) -> Option<&NativeProfileActionStartPlan> {
        self.pending.first()
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[NativeProfileActionStartPlan] {
        &self.pending
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionStartExecution {
    pub plan: NativeProfileActionStartPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeProfileActionCurrentSession {
    pub key: PendingProfileActionKey,
    pub kind: NativeResolvedProfileActionKind,
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub mode: NativeProfileActionStartMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeSessionLaunchKind {
    Local,
    ProfilePicker,
    ProfileLaunch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionLaunchMetadata {
    pub kind: NativeSessionLaunchKind,
    pub config: LocalPtyConfig,
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionRecord {
    pub id: NativeSessionId,
    pub profile_action: NativeProfileActionCurrentSession,
    pub launch: Option<NativeSessionLaunchMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionRegistry {
    next_session_id: u64,
    active_session_id: Option<NativeSessionId>,
    sessions: Vec<NativeSessionRecord>,
}

impl Default for NativeSessionRegistry {
    fn default() -> Self {
        Self {
            next_session_id: 1,
            active_session_id: None,
            sessions: Vec::new(),
        }
    }
}

impl NativeSessionRegistry {
    pub(crate) fn replace_current(
        &mut self,
        session: NativeProfileActionCurrentSession,
    ) -> NativeSessionId {
        if let Some(active_session_id) = self.active_session_id {
            if let Some(record) = self
                .sessions
                .iter_mut()
                .find(|record| record.id == active_session_id)
            {
                record.profile_action = session;
                return active_session_id;
            }
        }

        let id = self.allocate_session_id();
        self.sessions.push(NativeSessionRecord {
            id,
            profile_action: session,
            launch: None,
        });
        self.active_session_id = Some(id);
        id
    }

    pub(crate) fn insert_inactive(
        &mut self,
        session: NativeProfileActionCurrentSession,
    ) -> NativeSessionId {
        let id = self.allocate_session_id();
        self.sessions.push(NativeSessionRecord {
            id,
            profile_action: session,
            launch: None,
        });
        id
    }

    fn insert_with_active_state(
        &mut self,
        session: NativeProfileActionCurrentSession,
        active: bool,
    ) -> NativeSessionId {
        let id = self.allocate_session_id();
        self.sessions.push(NativeSessionRecord {
            id,
            profile_action: session,
            launch: None,
        });
        if active {
            self.active_session_id = Some(id);
        }
        id
    }

    pub(crate) fn active(&self) -> Option<&NativeSessionRecord> {
        let active_session_id = self.active_session_id?;
        self.sessions
            .iter()
            .find(|record| record.id == active_session_id)
    }

    pub(crate) fn set_active(&mut self, session_id: NativeSessionId) -> bool {
        if self.active_session_id == Some(session_id) {
            return false;
        }
        if !self.sessions.iter().any(|record| record.id == session_id) {
            return false;
        }

        self.active_session_id = Some(session_id);
        true
    }

    fn set_launch_metadata(
        &mut self,
        session_id: NativeSessionId,
        launch: NativeSessionLaunchMetadata,
    ) -> bool {
        let Some(record) = self
            .sessions
            .iter_mut()
            .find(|record| record.id == session_id)
        else {
            return false;
        };
        record.launch = Some(launch);
        true
    }

    pub(crate) fn remove_inactive(
        &mut self,
        session_id: NativeSessionId,
    ) -> Option<NativeSessionRecord> {
        if self.active_session_id == Some(session_id) {
            return None;
        }
        let index = self
            .sessions
            .iter()
            .position(|record| record.id == session_id)?;
        Some(self.sessions.remove(index))
    }

    pub(crate) fn is_inactive_session(&self, session_id: NativeSessionId) -> bool {
        self.active_session_id != Some(session_id)
            && self.sessions.iter().any(|record| record.id == session_id)
    }

    pub(crate) fn tab_rows(&self) -> Vec<NativeSessionTabRow> {
        self.sessions
            .iter()
            .map(|record| native_session_tab_row(record, self.active_session_id))
            .collect()
    }

    pub(crate) fn inactive_session_ids(&self) -> Vec<NativeSessionId> {
        self.sessions
            .iter()
            .filter(|record| Some(record.id) != self.active_session_id)
            .map(|record| record.id)
            .collect()
    }

    fn clear(&mut self) {
        self.active_session_id = None;
        self.sessions.clear();
    }

    fn allocate_session_id(&mut self) -> NativeSessionId {
        let id = NativeSessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.saturating_add(1);
        id
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[NativeSessionRecord] {
        &self.sessions
    }
}

struct NativeSessionRuntime<T> {
    transport: T,
    terminal: BasicTerminal,
    terminal_search: TerminalSearch,
    shell_integration: ShellIntegrationState,
}

struct NativeSessionRuntimeRecord<T> {
    id: NativeSessionId,
    runtime: NativeSessionRuntime<T>,
}

struct NativeSessionRuntimeRegistry<T> {
    parked: Vec<NativeSessionRuntimeRecord<T>>,
}

impl<T> Default for NativeSessionRuntimeRegistry<T> {
    fn default() -> Self {
        Self { parked: Vec::new() }
    }
}

impl<T> NativeSessionRuntimeRegistry<T> {
    fn park_or_replace(
        &mut self,
        id: NativeSessionId,
        runtime: NativeSessionRuntime<T>,
    ) -> Option<NativeSessionRuntime<T>> {
        if let Some(record) = self.parked.iter_mut().find(|record| record.id == id) {
            return Some(std::mem::replace(&mut record.runtime, runtime));
        }

        self.parked.push(NativeSessionRuntimeRecord { id, runtime });
        None
    }

    fn take(&mut self, id: NativeSessionId) -> Option<NativeSessionRuntime<T>> {
        let index = self.parked.iter().position(|record| record.id == id)?;
        Some(self.parked.remove(index).runtime)
    }

    fn contains(&self, id: NativeSessionId) -> bool {
        self.parked.iter().any(|record| record.id == id)
    }

    fn clear(&mut self) {
        self.parked.clear();
    }

    #[cfg(test)]
    fn as_slice(&self) -> &[NativeSessionRuntimeRecord<T>] {
        &self.parked
    }
}

impl<T> NativeSessionRuntimeRegistry<T>
where
    T: TerminalTransport,
{
    fn resize_all(&mut self, size: GridSize) {
        for record in &mut self.parked {
            record.runtime.terminal.resize(size);
            if let Err(err) = record.runtime.transport.resize(size) {
                eprintln!("failed to resize parked session {}: {err:#}", record.id.0);
            }
            record
                .runtime
                .terminal_search
                .rebuild(&record.runtime.terminal.search_text_rows());
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionTabRow {
    pub session_id: NativeSessionId,
    pub key: PendingProfileActionKey,
    pub kind: NativeResolvedProfileActionKind,
    pub source_plugin: String,
    pub profile_id: String,
    pub mode: NativeProfileActionStartMode,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeSessionTabStripTarget {
    Select,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeSessionTabStripHit {
    pub session_id: NativeSessionId,
    pub row_index: usize,
    pub target: NativeSessionTabStripTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeSessionTabStripNotice {
    LastActiveCloseBlocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeSessionTabStripSpan {
    hit: NativeSessionTabStripHit,
    start_col: u16,
    end_col: u16,
}

fn native_profile_action_start_plan(
    handoff: NativeResolvedProfileActionHandoff,
    mode: NativeProfileActionStartMode,
) -> NativeProfileActionStartPlan {
    NativeProfileActionStartPlan {
        mode,
        key: handoff.key,
        kind: handoff.kind,
        source_plugin: handoff.source_plugin,
        profile_id: handoff.profile_id,
        reason: handoff.reason,
        config: handoff.config,
    }
}

fn apply_native_resolved_profile_action_session_policy(
    handoffs: &mut NativeResolvedProfileActionHandoffQueue,
    deferred_starts: &mut NativeResolvedProfileActionHandoffQueue,
    policy: NativeResolvedProfileActionSessionPolicy,
) -> bool {
    let Some(handoff) = handoffs.take_next() else {
        return false;
    };

    match policy {
        NativeResolvedProfileActionSessionPolicy::DeferStart => {
            deferred_starts.push(handoff);
        }
    }
    true
}

fn plan_next_native_profile_action_start(
    deferred_starts: &mut NativeResolvedProfileActionHandoffQueue,
    start_plans: &mut NativeProfileActionStartPlanQueue,
    mode: NativeProfileActionStartMode,
) -> bool {
    let Some(handoff) = deferred_starts.take_next() else {
        return false;
    };
    start_plans.push(native_profile_action_start_plan(handoff, mode));
    true
}

fn apply_native_profile_action_start_plan_with_transport<T>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    mut plan: NativeProfileActionStartPlan,
    transport: T,
    size: GridSize,
) -> NativeProfileActionStartExecution
where
    T: TerminalTransport,
{
    if plan.mode == NativeProfileActionStartMode::NewTab && sessions.active().is_none() {
        plan.mode = NativeProfileActionStartMode::ReplaceCurrentSession;
    }
    let launch = native_session_launch_metadata_for_profile_plan(&plan);
    match plan.mode {
        NativeProfileActionStartMode::ReplaceCurrentSession => {
            let max_scrollback_lines = terminal.max_scrollback_lines();
            app.replace_transport(transport);
            *terminal = basic_terminal_like(size, max_scrollback_lines, terminal);
            *terminal_search = TerminalSearch::default();
            *shell_integration = ShellIntegrationState::default();
        }
        NativeProfileActionStartMode::NewTab => {
            let execution = NativeProfileActionStartExecution { plan };
            let session_id =
                sessions.insert_inactive(native_profile_action_current_session(&execution));
            sessions.set_launch_metadata(session_id, launch);
            parked_sessions.park_or_replace(
                session_id,
                NativeSessionRuntime {
                    transport,
                    terminal: basic_terminal_like(size, terminal.max_scrollback_lines(), terminal),
                    terminal_search: TerminalSearch::default(),
                    shell_integration: ShellIntegrationState::default(),
                },
            );
            return execution;
        }
    }
    let execution = NativeProfileActionStartExecution { plan };
    let session_id = sessions.replace_current(native_profile_action_current_session(&execution));
    sessions.set_launch_metadata(session_id, launch);
    execution
}

fn switch_native_session_runtime<T>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    current_session_id: NativeSessionId,
    target_session_id: NativeSessionId,
) -> bool
where
    T: TerminalTransport,
{
    if current_session_id == target_session_id {
        return false;
    }

    let Some(target_runtime) = parked_sessions.take(target_session_id) else {
        return false;
    };

    let old_transport = app.replace_transport(target_runtime.transport);
    let old_terminal = std::mem::replace(terminal, target_runtime.terminal);
    let old_search = std::mem::replace(terminal_search, target_runtime.terminal_search);
    let old_shell_integration =
        std::mem::replace(shell_integration, target_runtime.shell_integration);

    parked_sessions.park_or_replace(
        current_session_id,
        NativeSessionRuntime {
            transport: old_transport,
            terminal: old_terminal,
            terminal_search: old_search,
            shell_integration: old_shell_integration,
        },
    );
    true
}

#[allow(dead_code)]
fn close_parked_native_session_runtime<T>(
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    session_id: NativeSessionId,
) -> bool {
    if !sessions.is_inactive_session(session_id) || !parked_sessions.contains(session_id) {
        return false;
    }

    sessions.remove_inactive(session_id).is_some() && parked_sessions.take(session_id).is_some()
}

#[allow(dead_code)]
fn close_active_native_session_by_switching_to_parked<T>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
) -> bool
where
    T: TerminalTransport,
{
    let Some(active_session_id) = sessions.active().map(|record| record.id) else {
        return false;
    };
    let Some(target_session_id) = sessions
        .inactive_session_ids()
        .into_iter()
        .find(|session_id| parked_sessions.contains(*session_id))
    else {
        return false;
    };

    if !switch_native_session_runtime(
        app,
        terminal,
        terminal_search,
        shell_integration,
        parked_sessions,
        active_session_id,
        target_session_id,
    ) {
        return false;
    }

    let activated_target = sessions.set_active(target_session_id);
    debug_assert!(activated_target);
    let closed_old_active =
        close_parked_native_session_runtime(sessions, parked_sessions, active_session_id);
    debug_assert!(closed_old_active);
    activated_target && closed_old_active
}

fn close_exited_active_native_session<T>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    policy: NativeActiveSessionCloseFallbackPolicy,
) -> NativeSessionCloseResult
where
    T: TerminalTransport,
{
    if sessions.active().is_none() {
        return native_session_close_result_for_last_active_policy(policy);
    }

    if close_active_native_session_by_switching_to_parked(
        app,
        terminal,
        terminal_search,
        shell_integration,
        sessions,
        parked_sessions,
    ) {
        return NativeSessionCloseResult::Closed;
    }

    if !has_inactive_parked_native_session_runtime(sessions, parked_sessions) {
        return native_session_close_result_for_last_active_policy(policy);
    }

    NativeSessionCloseResult::Ignored
}

fn replace_with_untracked_local_session<T>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    transport: T,
    size: GridSize,
) where
    T: TerminalTransport,
{
    let max_scrollback_lines = terminal.max_scrollback_lines();
    app.replace_transport(transport);
    *terminal = basic_terminal_like(size, max_scrollback_lines, terminal);
    *terminal_search = TerminalSearch::default();
    *shell_integration = ShellIntegrationState::default();
    sessions.clear();
    parked_sessions.clear();
}

fn apply_fallback_local_session_with_spawner<T, F>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    spawn_transport: F,
    size: GridSize,
) -> Result<()>
where
    T: TerminalTransport,
    F: FnOnce(GridSize) -> Result<T>,
{
    let transport = spawn_transport(size)?;
    replace_with_untracked_local_session(
        app,
        terminal,
        terminal_search,
        shell_integration,
        sessions,
        parked_sessions,
        transport,
        size,
    );
    Ok(())
}

fn replace_with_tracked_local_session_with_spawner<T, F>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    config: LocalPtyConfig,
    spawn_transport: F,
    size: GridSize,
) -> Result<NativeSessionId>
where
    T: TerminalTransport,
    F: FnOnce(LocalPtyConfig) -> Result<T>,
{
    let config_for_metadata = config.clone();
    let transport = spawn_transport(config)?;
    replace_with_untracked_local_session(
        app,
        terminal,
        terminal_search,
        shell_integration,
        sessions,
        parked_sessions,
        transport,
        size,
    );

    let session = native_local_session_metadata(
        sessions.next_session_id,
        NativeProfileActionStartMode::ReplaceCurrentSession,
    );
    let launch = native_session_launch_metadata_for_local(&config_for_metadata, &session);
    let session_id = sessions.replace_current(session);
    sessions.set_launch_metadata(session_id, launch);
    Ok(session_id)
}

fn native_local_session_metadata(
    session_id_hint: u64,
    mode: NativeProfileActionStartMode,
) -> NativeProfileActionCurrentSession {
    let request_index = usize::try_from(session_id_hint.saturating_sub(1)).unwrap_or(usize::MAX);
    NativeProfileActionCurrentSession {
        key: PendingProfileActionKey::profile_launch(request_index),
        kind: NativeResolvedProfileActionKind::ProfileLaunch,
        source_plugin: NATIVE_LOCAL_SESSION_SOURCE_PLUGIN.to_owned(),
        profile_id: format!("local-{session_id_hint}"),
        reason: None,
        mode,
    }
}

fn ensure_active_native_local_session(sessions: &mut NativeSessionRegistry) -> NativeSessionId {
    if let Some(active) = sessions.active() {
        return active.id;
    }

    let session = native_local_session_metadata(
        sessions.next_session_id,
        NativeProfileActionStartMode::ReplaceCurrentSession,
    );
    sessions.replace_current(session)
}

fn local_new_tab_config(template: &LocalPtyConfig, size: GridSize) -> LocalPtyConfig {
    let mut config = template.clone();
    config.size = size;
    config
}

fn open_local_new_tab_with_spawner<T, F>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    config: LocalPtyConfig,
    spawn_transport: F,
    size: GridSize,
) -> Result<NativeSessionId>
where
    T: TerminalTransport,
    F: FnOnce(LocalPtyConfig) -> Result<T>,
{
    let config_for_metadata = config.clone();
    let had_active_session = sessions.active().is_some();
    let transport = spawn_transport(config)?;
    let active_session_id = ensure_active_native_local_session(sessions);
    if !had_active_session {
        if let Some(active) = sessions.active().cloned() {
            let launch = native_session_launch_metadata_for_local(
                &config_for_metadata,
                &active.profile_action,
            );
            sessions.set_launch_metadata(active.id, launch);
        }
    }
    let new_session = native_local_session_metadata(
        sessions.next_session_id,
        NativeProfileActionStartMode::NewTab,
    );
    let new_session_launch =
        native_session_launch_metadata_for_local(&config_for_metadata, &new_session);
    let new_session_id = sessions.insert_inactive(new_session);
    sessions.set_launch_metadata(new_session_id, new_session_launch);
    parked_sessions.park_or_replace(
        new_session_id,
        NativeSessionRuntime {
            transport,
            terminal: basic_terminal_like(size, terminal.max_scrollback_lines(), terminal),
            terminal_search: TerminalSearch::default(),
            shell_integration: ShellIntegrationState::default(),
        },
    );

    if !switch_native_session_runtime(
        app,
        terminal,
        terminal_search,
        shell_integration,
        parked_sessions,
        active_session_id,
        new_session_id,
    ) {
        let _ = close_parked_native_session_runtime(sessions, parked_sessions, new_session_id);
        bail!("failed to activate local new tab");
    }
    if !sessions.set_active(new_session_id) {
        bail!("failed to mark local new tab active");
    }

    Ok(new_session_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeSessionCloseResult {
    Closed,
    BlockedLastActive,
    RequestWindowClose,
    RequestFallbackLocalSession,
    Ignored,
}

impl NativeSessionCloseResult {
    #[cfg(test)]
    const ALL: &'static [Self] = &[
        Self::Closed,
        Self::BlockedLastActive,
        Self::RequestWindowClose,
        Self::RequestFallbackLocalSession,
        Self::Ignored,
    ];

    #[cfg(test)]
    fn all() -> &'static [Self] {
        Self::ALL
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NativeSessionCloseEventRequests {
    window_close: bool,
    fallback_local_session: bool,
}

impl NativeSessionCloseEventRequests {
    fn any(self) -> bool {
        self.window_close || self.fallback_local_session
    }

    fn apply_to(
        self,
        window_close_requested: &mut bool,
        fallback_local_session_requested: &mut bool,
    ) {
        if self.window_close {
            *window_close_requested = true;
        }
        if self.fallback_local_session {
            *fallback_local_session_requested = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum NativeActiveSessionCloseFallbackPolicy {
    Block,
    #[default]
    CloseWindow,
    FallbackLocalSession,
}

impl From<WindowLastActiveClosePolicy> for NativeActiveSessionCloseFallbackPolicy {
    fn from(policy: WindowLastActiveClosePolicy) -> Self {
        match policy {
            WindowLastActiveClosePolicy::Block => Self::Block,
            WindowLastActiveClosePolicy::CloseWindow => Self::CloseWindow,
            WindowLastActiveClosePolicy::FallbackLocalSession => Self::FallbackLocalSession,
        }
    }
}

impl NativeActiveSessionCloseFallbackPolicy {
    #[cfg(test)]
    const ALL: &'static [Self] = &[Self::Block, Self::CloseWindow, Self::FallbackLocalSession];

    #[cfg(test)]
    fn all() -> &'static [Self] {
        Self::ALL
    }

    fn as_config_value(self) -> &'static str {
        match self {
            Self::Block => WindowLastActiveClosePolicy::Block.as_config_value(),
            Self::CloseWindow => WindowLastActiveClosePolicy::CloseWindow.as_config_value(),
            Self::FallbackLocalSession => {
                WindowLastActiveClosePolicy::FallbackLocalSession.as_config_value()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeActiveSessionCloseFallbackAction {
    BlockLastActive,
    RequestWindowClose,
    RequestFallbackLocalSession,
}

fn native_active_session_close_fallback_action_without_switch_target(
    policy: NativeActiveSessionCloseFallbackPolicy,
) -> NativeActiveSessionCloseFallbackAction {
    match policy {
        NativeActiveSessionCloseFallbackPolicy::Block => {
            NativeActiveSessionCloseFallbackAction::BlockLastActive
        }
        NativeActiveSessionCloseFallbackPolicy::CloseWindow => {
            NativeActiveSessionCloseFallbackAction::RequestWindowClose
        }
        NativeActiveSessionCloseFallbackPolicy::FallbackLocalSession => {
            NativeActiveSessionCloseFallbackAction::RequestFallbackLocalSession
        }
    }
}

fn native_session_close_result_for_fallback_action(
    action: NativeActiveSessionCloseFallbackAction,
) -> NativeSessionCloseResult {
    match action {
        NativeActiveSessionCloseFallbackAction::BlockLastActive => {
            NativeSessionCloseResult::BlockedLastActive
        }
        NativeActiveSessionCloseFallbackAction::RequestWindowClose => {
            NativeSessionCloseResult::RequestWindowClose
        }
        NativeActiveSessionCloseFallbackAction::RequestFallbackLocalSession => {
            NativeSessionCloseResult::RequestFallbackLocalSession
        }
    }
}

fn native_session_close_result_for_last_active_policy(
    policy: NativeActiveSessionCloseFallbackPolicy,
) -> NativeSessionCloseResult {
    native_session_close_result_for_fallback_action(
        native_active_session_close_fallback_action_without_switch_target(policy),
    )
}

fn native_session_close_event_requests(
    result: NativeSessionCloseResult,
) -> NativeSessionCloseEventRequests {
    match result {
        NativeSessionCloseResult::RequestWindowClose => NativeSessionCloseEventRequests {
            window_close: true,
            fallback_local_session: false,
        },
        NativeSessionCloseResult::RequestFallbackLocalSession => NativeSessionCloseEventRequests {
            window_close: false,
            fallback_local_session: true,
        },
        NativeSessionCloseResult::Closed
        | NativeSessionCloseResult::BlockedLastActive
        | NativeSessionCloseResult::Ignored => NativeSessionCloseEventRequests::default(),
    }
}

fn has_inactive_parked_native_session_runtime<T>(
    sessions: &NativeSessionRegistry,
    parked_sessions: &NativeSessionRuntimeRegistry<T>,
) -> bool {
    sessions
        .inactive_session_ids()
        .into_iter()
        .any(|id| parked_sessions.contains(id))
}

fn take_native_event_request_flag(requested: &mut bool) -> bool {
    let value = *requested;
    *requested = false;
    value
}

fn native_session_tab_notice_after_close_result(
    current: Option<NativeSessionTabStripNotice>,
    result: NativeSessionCloseResult,
) -> Option<NativeSessionTabStripNotice> {
    match result {
        NativeSessionCloseResult::Closed => None,
        NativeSessionCloseResult::BlockedLastActive => {
            Some(NativeSessionTabStripNotice::LastActiveCloseBlocked)
        }
        NativeSessionCloseResult::RequestWindowClose => None,
        NativeSessionCloseResult::RequestFallbackLocalSession => None,
        NativeSessionCloseResult::Ignored => current,
    }
}

fn native_profile_action_current_session(
    execution: &NativeProfileActionStartExecution,
) -> NativeProfileActionCurrentSession {
    NativeProfileActionCurrentSession {
        key: execution.plan.key,
        kind: execution.plan.kind,
        source_plugin: execution.plan.source_plugin.clone(),
        profile_id: execution.plan.profile_id.clone(),
        reason: execution.plan.reason.clone(),
        mode: execution.plan.mode,
    }
}

fn native_session_launch_metadata_for_profile_plan(
    plan: &NativeProfileActionStartPlan,
) -> NativeSessionLaunchMetadata {
    NativeSessionLaunchMetadata {
        kind: match plan.kind {
            NativeResolvedProfileActionKind::ProfilePicker => {
                NativeSessionLaunchKind::ProfilePicker
            }
            NativeResolvedProfileActionKind::ProfileLaunch => {
                NativeSessionLaunchKind::ProfileLaunch
            }
        },
        config: plan.config.clone(),
        source_plugin: plan.source_plugin.clone(),
        profile_id: plan.profile_id.clone(),
        reason: plan.reason.clone(),
    }
}

fn native_session_launch_metadata_for_local(
    config: &LocalPtyConfig,
    session: &NativeProfileActionCurrentSession,
) -> NativeSessionLaunchMetadata {
    NativeSessionLaunchMetadata {
        kind: NativeSessionLaunchKind::Local,
        config: config.clone(),
        source_plugin: session.source_plugin.clone(),
        profile_id: session.profile_id.clone(),
        reason: session.reason.clone(),
    }
}

fn native_session_tab_row(
    record: &NativeSessionRecord,
    active_session_id: Option<NativeSessionId>,
) -> NativeSessionTabRow {
    let session = &record.profile_action;
    NativeSessionTabRow {
        session_id: record.id,
        key: session.key,
        kind: session.kind,
        source_plugin: session.source_plugin.clone(),
        profile_id: session.profile_id.clone(),
        mode: session.mode,
        active: Some(record.id) == active_session_id,
    }
}

fn restart_snapshot_v1_for_native_state(
    sessions: &NativeSessionRegistry,
    local_tab_config: &LocalPtyConfig,
    grid_size: GridSize,
    inner_size: Option<PhysicalSize<u32>>,
    running_build_id: &str,
    installed_build_id: Option<&str>,
) -> RestartSnapshotV1 {
    restart_snapshot_v1_for_native_state_at(
        sessions,
        local_tab_config,
        grid_size,
        inner_size,
        running_build_id,
        installed_build_id,
        now_unix_ms(),
    )
}

fn restart_snapshot_v1_for_native_state_at(
    sessions: &NativeSessionRegistry,
    local_tab_config: &LocalPtyConfig,
    grid_size: GridSize,
    inner_size: Option<PhysicalSize<u32>>,
    running_build_id: &str,
    installed_build_id: Option<&str>,
    created_at_unix_ms: u128,
) -> RestartSnapshotV1 {
    let tabs = restart_snapshot_tabs_for_native_state(sessions, local_tab_config);
    let active_tab_index = tabs.iter().position(|tab| tab.active).unwrap_or(0);
    RestartSnapshotV1 {
        schema_version: 1,
        created_at_unix_ms,
        running_build_id: running_build_id.to_owned(),
        installed_build_id: installed_build_id.map(str::to_owned),
        window: RestartWindowStateV1 {
            grid_rows: grid_size.rows,
            grid_cols: grid_size.cols,
            inner_width_px: inner_size.map(|size| size.width),
            inner_height_px: inner_size.map(|size| size.height),
        },
        active_tab_index,
        tabs,
    }
}

fn restart_snapshot_tabs_for_native_state(
    sessions: &NativeSessionRegistry,
    local_tab_config: &LocalPtyConfig,
) -> Vec<RestartTabV1> {
    if sessions.sessions.is_empty() {
        let session =
            native_local_session_metadata(1, NativeProfileActionStartMode::ReplaceCurrentSession);
        let launch = native_session_launch_metadata_for_local(local_tab_config, &session);
        return vec![restart_tab_v1_from_session_parts(
            NativeSessionId(1),
            true,
            &session,
            &launch,
        )];
    }

    let active_session_id = sessions
        .active_session_id
        .or_else(|| sessions.sessions.first().map(|record| record.id));
    sessions
        .sessions
        .iter()
        .map(|record| {
            let launch = record.launch.clone().unwrap_or_else(|| {
                native_session_launch_metadata_for_local(local_tab_config, &record.profile_action)
            });
            restart_tab_v1_from_session_parts(
                record.id,
                Some(record.id) == active_session_id,
                &record.profile_action,
                &launch,
            )
        })
        .collect()
}

fn restart_tab_v1_from_session_parts(
    session_id: NativeSessionId,
    active: bool,
    session: &NativeProfileActionCurrentSession,
    launch: &NativeSessionLaunchMetadata,
) -> RestartTabV1 {
    RestartTabV1 {
        tab_id: session_id.0,
        active,
        source_plugin: session.source_plugin.clone(),
        profile_id: session.profile_id.clone(),
        kind: restart_tab_kind_v1(launch.kind),
        mode: restart_tab_mode_v1(session.mode),
        launch: RestartLaunchConfigV1::from_local_pty_config(&launch.config),
        profile: (launch.kind != NativeSessionLaunchKind::Local).then(|| {
            RestartProfileMetadataV1 {
                source_plugin: launch.source_plugin.clone(),
                profile_id: launch.profile_id.clone(),
                reason: launch.reason.clone(),
            }
        }),
    }
}

fn restart_tab_kind_v1(kind: NativeSessionLaunchKind) -> RestartTabKindV1 {
    match kind {
        NativeSessionLaunchKind::Local => RestartTabKindV1::Local,
        NativeSessionLaunchKind::ProfilePicker => RestartTabKindV1::ProfilePicker,
        NativeSessionLaunchKind::ProfileLaunch => RestartTabKindV1::ProfileLaunch,
    }
}

fn restart_tab_mode_v1(mode: NativeProfileActionStartMode) -> RestartTabModeV1 {
    match mode {
        NativeProfileActionStartMode::ReplaceCurrentSession => {
            RestartTabModeV1::ReplaceCurrentSession
        }
        NativeProfileActionStartMode::NewTab => RestartTabModeV1::NewTab,
    }
}

fn native_session_from_restart_tab(tab: &RestartTabV1) -> NativeProfileActionCurrentSession {
    NativeProfileActionCurrentSession {
        key: PendingProfileActionKey::profile_launch(
            usize::try_from(tab.tab_id.saturating_sub(1)).unwrap_or(usize::MAX),
        ),
        kind: match tab.kind {
            RestartTabKindV1::Local | RestartTabKindV1::ProfileLaunch => {
                NativeResolvedProfileActionKind::ProfileLaunch
            }
            RestartTabKindV1::ProfilePicker => NativeResolvedProfileActionKind::ProfilePicker,
        },
        source_plugin: tab.source_plugin.clone(),
        profile_id: tab.profile_id.clone(),
        reason: tab
            .profile
            .as_ref()
            .and_then(|profile| profile.reason.clone()),
        mode: match tab.mode {
            RestartTabModeV1::ReplaceCurrentSession => {
                NativeProfileActionStartMode::ReplaceCurrentSession
            }
            RestartTabModeV1::NewTab => NativeProfileActionStartMode::NewTab,
        },
    }
}

fn native_session_launch_metadata_from_restart_tab(
    tab: &RestartTabV1,
    size: GridSize,
) -> NativeSessionLaunchMetadata {
    NativeSessionLaunchMetadata {
        kind: match tab.kind {
            RestartTabKindV1::Local => NativeSessionLaunchKind::Local,
            RestartTabKindV1::ProfilePicker => NativeSessionLaunchKind::ProfilePicker,
            RestartTabKindV1::ProfileLaunch => NativeSessionLaunchKind::ProfileLaunch,
        },
        config: tab.launch.to_local_pty_config(size),
        source_plugin: tab.source_plugin.clone(),
        profile_id: tab.profile_id.clone(),
        reason: tab
            .profile
            .as_ref()
            .and_then(|profile| profile.reason.clone()),
    }
}

fn apply_next_native_profile_action_start_plan_with_spawner<T, F>(
    app: &mut TerminalApp<T>,
    terminal: &mut BasicTerminal,
    terminal_search: &mut TerminalSearch,
    shell_integration: &mut ShellIntegrationState,
    sessions: &mut NativeSessionRegistry,
    parked_sessions: &mut NativeSessionRuntimeRegistry<T>,
    start_plans: &mut NativeProfileActionStartPlanQueue,
    spawn_transport: F,
    size: GridSize,
) -> Result<Option<NativeProfileActionStartExecution>>
where
    T: TerminalTransport,
    F: FnOnce(LocalPtyConfig) -> Result<T>,
{
    let Some(plan) = start_plans.peek_next().cloned() else {
        return Ok(None);
    };
    let transport = spawn_transport(plan.config.clone())?;
    let plan = start_plans
        .take_next()
        .expect("peeked profile action start plan should still be queued");
    Ok(Some(apply_native_profile_action_start_plan_with_transport(
        app,
        terminal,
        terminal_search,
        shell_integration,
        sessions,
        parked_sessions,
        plan,
        transport,
        size,
    )))
}

fn native_resolved_profile_action_handoff(
    resolved: ResolvedPendingProfileActionPtyConfig,
) -> NativeResolvedProfileActionHandoff {
    match resolved {
        ResolvedPendingProfileActionPtyConfig::ProfilePicker { key, resolved } => {
            NativeResolvedProfileActionHandoff {
                key,
                kind: NativeResolvedProfileActionKind::ProfilePicker,
                source_plugin: resolved.source_plugin,
                profile_id: resolved.profile_id,
                reason: resolved.reason,
                config: resolved.config,
            }
        }
        ResolvedPendingProfileActionPtyConfig::ProfileLaunch { key, resolved } => {
            NativeResolvedProfileActionHandoff {
                key,
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: resolved.source_plugin,
                profile_id: resolved.profile_id,
                reason: resolved.reason,
                config: resolved.config,
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NativeProfileActionBridge {
    snapshot: NativeProfileActionSnapshot,
    start_success: Option<NativeProfileActionStartSuccessRow>,
    start_failure: Option<NativeProfileActionStartFailureRow>,
}

impl NativeProfileActionBridge {
    pub(crate) fn snapshot(&self) -> &NativeProfileActionSnapshot {
        &self.snapshot
    }

    fn snapshot_with_start_status(
        &self,
        mut snapshot: NativeProfileActionSnapshot,
    ) -> NativeProfileActionSnapshot {
        snapshot.start_success = self.start_success.clone();
        snapshot.start_failure = self.start_failure.clone();
        snapshot
    }

    pub(crate) fn set_start_success(
        &mut self,
        success: Option<NativeProfileActionStartSuccessRow>,
    ) {
        self.start_success = success;
        if self.start_success.is_some() {
            self.start_failure = None;
        }
        self.snapshot.start_success = self.start_success.clone();
        self.snapshot.start_failure = self.start_failure.clone();
    }

    pub(crate) fn set_start_failure(
        &mut self,
        failure: Option<NativeProfileActionStartFailureRow>,
    ) {
        self.start_failure = failure;
        if self.start_failure.is_some() {
            self.start_success = None;
        }
        self.snapshot.start_success = self.start_success.clone();
        self.snapshot.start_failure = self.start_failure.clone();
    }

    pub(crate) fn refresh<T>(
        &mut self,
        app: &TerminalApp<T>,
        store: &ProfileStoreV1,
    ) -> Result<NativeProfileActionBridgeEvent>
    where
        T: TerminalTransport,
    {
        let snapshot = self.snapshot_with_start_status(native_profile_action_snapshot(app, store)?);
        self.snapshot = snapshot.clone();
        Ok(NativeProfileActionBridgeEvent::SnapshotRefreshed(snapshot))
    }

    #[allow(dead_code)]
    pub(crate) fn dismiss<T>(
        &mut self,
        app: &mut TerminalApp<T>,
        store: &ProfileStoreV1,
        key: PendingProfileActionKey,
    ) -> Result<NativeProfileActionBridgeEvent>
    where
        T: TerminalTransport,
    {
        let dismissed = app.dismiss_pending_profile_action(key)?;
        let snapshot = self.snapshot_with_start_status(native_profile_action_snapshot(app, store)?);
        self.snapshot = snapshot.clone();
        Ok(NativeProfileActionBridgeEvent::Dismissed {
            dismissed,
            snapshot,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn confirm<T>(
        &mut self,
        app: &mut TerminalApp<T>,
        store: &ProfileStoreV1,
        confirmation: PendingProfileActionConfirmation,
        size: GridSize,
    ) -> Result<NativeProfileActionBridgeEvent>
    where
        T: TerminalTransport,
    {
        let resolved =
            app.take_resolved_pending_profile_action_pty_config(store, confirmation, size)?;
        let snapshot = self.snapshot_with_start_status(native_profile_action_snapshot(app, store)?);
        self.snapshot = snapshot.clone();
        Ok(NativeProfileActionBridgeEvent::Confirmed { resolved, snapshot })
    }
}

fn native_profile_action_display_rows(
    reviews: &[PendingProfileActionReview],
) -> Vec<NativeProfileActionDisplayRow> {
    reviews
        .iter()
        .map(|review| match review {
            PendingProfileActionReview::ProfilePicker { key, request } => {
                let profile_count = request.summary.profiles.len();
                NativeProfileActionDisplayRow {
                    key: *key,
                    source_plugin: request.source_plugin.clone(),
                    title: "Choose SSH profile".to_owned(),
                    detail: format!(
                        "{} profile{} available, {} launchable, {} require credentials",
                        profile_count,
                        plural_suffix(profile_count),
                        request.summary.launchable_profiles,
                        request.summary.credential_resolver_required_profiles
                    ),
                    reason: request.reason.clone(),
                    status: NativeProfileActionDisplayStatus::PickProfile,
                    confirm_label: (request.summary.launchable_profiles > 0)
                        .then(|| "Choose".to_owned()),
                    new_tab_label: None,
                    dismiss_label: "Dismiss".to_owned(),
                }
            }
            PendingProfileActionReview::ProfileLaunch { key, request } => {
                let status = match request.status {
                    PendingProfileLaunchRequestStatus::Launchable => {
                        NativeProfileActionDisplayStatus::Launchable
                    }
                    PendingProfileLaunchRequestStatus::RequiresCredentialResolver => {
                        NativeProfileActionDisplayStatus::RequiresCredentialResolver
                    }
                    PendingProfileLaunchRequestStatus::NotFound => {
                        NativeProfileActionDisplayStatus::NotFound
                    }
                };
                let profile_label = request
                    .profile_name
                    .as_deref()
                    .unwrap_or(request.profile_id.as_str());
                let tags = if request.tags.is_empty() {
                    "-".to_owned()
                } else {
                    request.tags.join(",")
                };
                NativeProfileActionDisplayRow {
                    key: *key,
                    source_plugin: request.source_plugin.clone(),
                    title: format!("Launch {profile_label}"),
                    detail: format!(
                        "id={} default={} tags={tags}",
                        request.profile_id, request.is_default
                    ),
                    reason: request.reason.clone(),
                    status,
                    confirm_label: (request.status
                        == PendingProfileLaunchRequestStatus::Launchable)
                        .then(|| "Launch".to_owned()),
                    new_tab_label: (request.status
                        == PendingProfileLaunchRequestStatus::Launchable)
                        .then(|| "New Tab".to_owned()),
                    dismiss_label: "Dismiss".to_owned(),
                }
            }
        })
        .collect()
}

fn native_profile_picker_option_rows(
    reviews: &[PendingProfileActionReview],
) -> Vec<NativeProfilePickerOptionRow> {
    reviews
        .iter()
        .flat_map(|review| match review {
            PendingProfileActionReview::ProfilePicker { key, request } => request
                .summary
                .profiles
                .iter()
                .map(|profile| {
                    let status = match profile.launchability {
                        SshProfileLaunchability::Launchable => {
                            NativeProfilePickerOptionStatus::Launchable
                        }
                        SshProfileLaunchability::RequiresCredentialResolver => {
                            NativeProfilePickerOptionStatus::RequiresCredentialResolver
                        }
                    };
                    NativeProfilePickerOptionRow {
                        request_key: *key,
                        profile_id: profile.id.clone(),
                        title: profile.name.clone(),
                        detail: format!(
                            "id={} default={} tags={}",
                            profile.id,
                            profile.is_default,
                            profile_tags_label(&profile.tags)
                        ),
                        status,
                        select_label: (profile.launchability
                            == SshProfileLaunchability::Launchable)
                            .then(|| "Select".to_owned()),
                        new_tab_label: (profile.launchability
                            == SshProfileLaunchability::Launchable)
                            .then(|| "New Tab".to_owned()),
                    }
                })
                .collect::<Vec<_>>(),
            PendingProfileActionReview::ProfileLaunch { .. } => Vec::new(),
        })
        .collect()
}

fn native_profile_action_start_success_row(
    plan: &NativeProfileActionStartPlan,
) -> NativeProfileActionStartSuccessRow {
    NativeProfileActionStartSuccessRow {
        key: plan.key,
        source_plugin: plan.source_plugin.clone(),
        profile_id: plan.profile_id.clone(),
        title: match plan.mode {
            NativeProfileActionStartMode::ReplaceCurrentSession => {
                format!("Active {}", plan.profile_id)
            }
            NativeProfileActionStartMode::NewTab => format!("New tab {}", plan.profile_id),
        },
        detail: format!(
            "mode={} status=started",
            native_profile_action_start_mode_label(plan.mode)
        ),
        reason: plan.reason.clone(),
        dismiss_label: "Dismiss".to_owned(),
    }
}

fn native_profile_action_start_failure_row(
    plan: &NativeProfileActionStartPlan,
) -> NativeProfileActionStartFailureRow {
    NativeProfileActionStartFailureRow {
        key: plan.key,
        source_plugin: plan.source_plugin.clone(),
        profile_id: plan.profile_id.clone(),
        title: format!("Retry {}", plan.profile_id),
        detail: format!(
            "mode={} status=failed",
            native_profile_action_start_mode_label(plan.mode)
        ),
        reason: plan.reason.clone(),
        retry_label: "Retry".to_owned(),
        dismiss_label: "Dismiss".to_owned(),
    }
}

fn profile_tags_label(tags: &[String]) -> String {
    if tags.is_empty() {
        "-".to_owned()
    } else {
        tags.join(",")
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

pub(crate) fn native_profile_action_snapshot<T>(
    app: &TerminalApp<T>,
    store: &ProfileStoreV1,
) -> Result<NativeProfileActionSnapshot>
where
    T: TerminalTransport,
{
    let reviews = app.review_pending_profile_actions(store)?;
    let display_rows = native_profile_action_display_rows(&reviews);
    let picker_options = native_profile_picker_option_rows(&reviews);
    Ok(NativeProfileActionSnapshot {
        reviews,
        display_rows,
        start_success: None,
        start_failure: None,
        picker_options,
        picker_requests: app.profile_picker_requests().len(),
        launch_requests: app.profile_launch_requests().len(),
    })
}

fn pending_profile_action_feedback(snapshot: &NativeProfileActionSnapshot) -> Option<String> {
    let mut counts = Vec::new();
    if snapshot.picker_requests > 0 {
        counts.push(format!("picker={}", snapshot.picker_requests));
    }
    if snapshot.launch_requests > 0 {
        counts.push(format!("launch={}", snapshot.launch_requests));
    }
    if counts.is_empty() {
        return None;
    }
    Some(format!(
        "\r\n[profile action pending: {}]\r\n",
        counts.join(" ")
    ))
}

fn plugin_actions_request_profile_action(actions: &[PluginAction]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action,
            PluginAction::RequestProfilePicker(_) | PluginAction::RequestProfileLaunch(_)
        )
    })
}

fn default_profile_action_store_snapshot() -> Result<ProfileStoreV1> {
    let path = default_profile_store_path()?;
    read_profile_store_snapshot_or_empty(&path)
}

fn read_profile_store_snapshot_or_empty(path: &Path) -> Result<ProfileStoreV1> {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                bail!("profile store path is not a file: {}", path.display());
            }
            read_profile_store(path)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(ProfileStoreV1::new()),
        Err(err) => Err(err).with_context(|| format!("check profile store {}", path.display())),
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct NativeImeEventResult {
    changed: bool,
    committed_text: Option<String>,
}

pub fn run(
    wasm_plugins: Vec<PathBuf>,
    smoke: WindowSmokeOptions,
    default_window_title: Option<String>,
    launch_program: Option<String>,
    launch_args: Vec<String>,
    launch_cwd: Option<PathBuf>,
    launch_env: Vec<(String, String)>,
    mouse_override_policy: MouseSelectionOverridePolicy,
    osc52_clipboard_policy: Osc52ClipboardPolicy,
    max_scrollback_lines: usize,
    font_family: Option<String>,
    font_size: Option<u16>,
    terminal_padding: Option<u16>,
    background_opacity: Option<f32>,
    background_image_path: Option<PathBuf>,
    background_image_fit: Option<RendererBackgroundImageFit>,
    cursor_shape: CursorShape,
    cursor_blink: bool,
    session_tab_position: NativeSessionTabPosition,
    session_tab_label_style: NativeSessionTabLabelStyle,
    session_tab_display_policy: NativeSessionTabDisplayPolicy,
    font_paths: Vec<PathBuf>,
    restore_state: Option<RestartSnapshotV1>,
) -> anyhow::Result<()> {
    start_smoke_exit_timer(smoke.exit_after)?;
    let event_loop = EventLoop::<NativeWindowEvent>::with_user_event().build()?;
    let event_loop_proxy = event_loop.create_proxy();
    let mut app = TerminalWindowApp::new(
        Some(event_loop_proxy),
        wasm_plugins,
        smoke,
        default_window_title,
        launch_program,
        launch_args,
        launch_cwd,
        launch_env,
        mouse_override_policy,
        osc52_clipboard_policy,
        max_scrollback_lines,
        font_family,
        font_size,
        terminal_padding,
        background_opacity,
        background_image_path,
        background_image_fit,
        cursor_shape,
        cursor_blink,
        session_tab_position,
        session_tab_label_style,
        session_tab_display_policy,
        font_paths,
        restore_state,
    )?;
    event_loop
        .run_app(&mut app)
        .context("run Witty native window event loop")?;
    Ok(())
}

fn start_smoke_exit_timer(exit_after: Option<Duration>) -> Result<()> {
    let Some(exit_after) = exit_after else {
        return Ok(());
    };
    // Smoke runs must terminate even if the GUI thread stalls inside platform GL code.
    thread::Builder::new()
        .name("witty-smoke-exit".to_owned())
        .spawn(move || {
            thread::sleep(exit_after);
            process::exit(0);
        })
        .context("start native window smoke exit timer")?;
    Ok(())
}

fn load_window_font_data(font_paths: &[PathBuf]) -> Result<Vec<Vec<u8>>> {
    font_paths
        .iter()
        .map(|path| {
            let data = std::fs::read(path)
                .with_context(|| format!("read font file {}", path.display()))?;
            if data.is_empty() {
                bail!("font file is empty: {}", path.display());
            }
            Ok(data)
        })
        .collect()
}

fn load_window_background_image(
    path: &Path,
    target_size: PhysicalSize<u32>,
) -> Option<RendererBackgroundImage> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!(
                "background image unavailable; falling back to transparent desktop background: {}: {err}",
                path.display()
            );
            return None;
        }
    };
    match RendererBackgroundImage::decode_cover_limited(
        &bytes,
        target_size.width,
        target_size.height,
    ) {
        Ok(image) => Some(image),
        Err(err) => {
            eprintln!(
                "background image unsupported; falling back to transparent desktop background: {}: {err:#}",
                path.display()
            );
            None
        }
    }
}

fn background_image_target_size(
    event_loop: &ActiveEventLoop,
    window_size: PhysicalSize<u32>,
) -> PhysicalSize<u32> {
    let mut width = window_size.width.max(1);
    let mut height = window_size.height.max(1);
    for monitor in event_loop.available_monitors() {
        let size = monitor.size();
        width = width.max(size.width);
        height = height.max(size.height);
    }
    PhysicalSize::new(width, height)
}

fn local_pty_config_for_launch(
    size: GridSize,
    program: Option<String>,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
) -> Result<LocalPtyConfig> {
    let mut config = match program {
        Some(program) => {
            let mut config = LocalPtyConfig::new(size);
            config.program = Some(program);
            config.args(args);
            config
        }
        None => {
            if !args.is_empty() {
                bail!("native window launch args require an explicit program");
            }
            LocalPtyConfig::new(size)
        }
    };

    if let Some(cwd) = cwd {
        config.cwd(cwd);
    }
    for pair in env {
        set_local_pty_env_pair(&mut config.env, pair);
    }
    Ok(config)
}

fn basic_terminal_with_cursor_style(
    size: GridSize,
    max_scrollback_lines: usize,
    cursor_shape: CursorShape,
    cursor_blink: bool,
) -> BasicTerminal {
    let mut terminal = BasicTerminal::with_scrollback_limit(size, max_scrollback_lines);
    terminal.set_default_cursor_style(cursor_shape, cursor_blink);
    terminal
}

fn basic_terminal_like(
    size: GridSize,
    max_scrollback_lines: usize,
    template: &BasicTerminal,
) -> BasicTerminal {
    let (cursor_shape, cursor_blink) = template.default_cursor_style();
    basic_terminal_with_cursor_style(size, max_scrollback_lines, cursor_shape, cursor_blink)
}

fn set_local_pty_env_pair(env: &mut Vec<(String, String)>, pair: (String, String)) {
    if let Some(existing) = env.iter_mut().find(|(key, _)| *key == pair.0) {
        existing.1 = pair.1;
    } else {
        env.push(pair);
    }
}

pub(crate) fn selection_copy_regression_smoke() -> Result<String> {
    let mut terminal = BasicTerminal::new(GridSize::new(3, 5));
    let mut clipboard = MemoryClipboardSink::default();

    terminal.feed(b"abc\r\ndef");
    terminal.set_selection(Some(CellRange {
        start: CellPoint::new(0, 1),
        end: CellPoint::new(1, 1),
    }));

    let copied = copy_selection_to_clipboard(&terminal, &mut clipboard)?;
    if !copied {
        bail!("selection copy smoke did not copy selected text");
    }
    if clipboard.clipboard_writes != 1 {
        bail!(
            "selection copy smoke wrote clipboard {} times, expected 1",
            clipboard.clipboard_writes
        );
    }
    if clipboard.clipboard_text != "bc\nde" {
        bail!(
            "selection copy smoke copied {:?}, expected {:?}",
            clipboard.clipboard_text,
            "bc\nde"
        );
    }

    Ok(clipboard.clipboard_text)
}

pub(crate) fn primary_selection_boundary_smoke() -> Result<String> {
    let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
    let mut clipboard = MemoryClipboardSink::default();

    terminal.feed(b"middle");
    terminal.set_selection(Some(CellRange {
        start: CellPoint::new(0, 0),
        end: CellPoint::new(0, 5),
    }));

    let copied = copy_selection_to_target(&terminal, &mut clipboard, ClipboardSelection::Primary)?;
    if !copied {
        bail!("primary selection smoke did not copy selected text");
    }
    if clipboard.primary_writes != 1 || clipboard.clipboard_writes != 0 {
        bail!(
            "primary selection smoke wrote primary={} clipboard={}, expected primary=1 clipboard=0",
            clipboard.primary_writes,
            clipboard.clipboard_writes
        );
    }

    let Some(text) = selection_paste_text(&mut clipboard, ClipboardSelection::Primary)? else {
        bail!("primary selection smoke could not read copied text");
    };
    if text != "middle" {
        bail!(
            "primary selection smoke read {:?}, expected {:?}",
            text,
            "middle"
        );
    }

    Ok(text)
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct PrimarySelectionGuiSmoke {
    pub copied: String,
    pub pasted: Vec<u8>,
}

pub(crate) fn primary_selection_gui_smoke() -> Result<PrimarySelectionGuiSmoke> {
    let mut terminal = BasicTerminal::new(GridSize::new(3, 24));
    let mut clipboard = MemoryClipboardSink::default();
    let point = CellPoint::new(0, 6);
    let now = Instant::now();

    terminal.feed(b"alpha middle omega");
    let first = selection_for_left_press(&terminal, None, point, now);
    terminal.set_selection(Some(first.range));

    let second = selection_for_left_press(
        &terminal,
        Some(first.click),
        point,
        now + Duration::from_millis(100),
    );
    if !second.completed || second.anchor.is_some() {
        bail!("primary selection GUI smoke did not complete double-click word selection");
    }
    terminal.set_selection(Some(second.range));

    let copied = copy_selection_to_primary(&terminal, &mut clipboard)?;
    if !copied {
        bail!("primary selection GUI smoke did not publish selected text");
    }
    if clipboard.primary_text != "middle" {
        bail!(
            "primary selection GUI smoke copied {:?}, expected {:?}",
            clipboard.primary_text,
            "middle"
        );
    }
    if clipboard.primary_writes != 1 || clipboard.clipboard_writes != 0 {
        bail!(
            "primary selection GUI smoke wrote primary={} clipboard={}, expected primary=1 clipboard=0",
            clipboard.primary_writes,
            clipboard.clipboard_writes
        );
    }

    terminal.feed(b"\x1b[?2004h");
    let mut pasted = Vec::new();
    let did_paste = paste_selection_to_input(
        &mut clipboard,
        ClipboardSelection::Primary,
        terminal.bracketed_paste_enabled(),
        |bytes| {
            pasted.extend_from_slice(bytes);
            Ok(())
        },
    )?;
    if !did_paste {
        bail!("primary selection GUI smoke did not paste primary selection");
    }
    let expected = b"\x1b[200~middle\x1b[201~";
    if pasted != expected {
        bail!(
            "primary selection GUI smoke pasted {:?}, expected {:?}",
            String::from_utf8_lossy(&pasted),
            String::from_utf8_lossy(expected)
        );
    }

    Ok(PrimarySelectionGuiSmoke {
        copied: clipboard.primary_text,
        pasted,
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct NativeSearchSmoke {
    pub query: String,
    pub match_count: usize,
    pub active_index: Option<usize>,
    pub visible_highlights: usize,
    pub active_visible: bool,
    pub status: String,
}

pub(crate) fn native_search_smoke() -> Result<NativeSearchSmoke> {
    let size = GridSize::new(3, 32);
    let mut terminal = BasicTerminal::new(size);
    let mut search = TerminalSearch::default();

    terminal.feed(b"alpha\r\nbeta\r\nalpha\r\ngamma");
    search.open(&terminal.search_text_rows(), Some("alpha"));
    let Some(active) = search.active_match() else {
        bail!("native search smoke did not find an active match");
    };
    if !terminal.scroll_to_search_match(active.row, SEARCH_SCROLL_BUFFER_ROWS) {
        bail!("native search smoke could not scroll to active match");
    }

    let highlights = terminal.visible_search_highlights(search.matches(), search.active_match());
    if highlights.is_empty() {
        bail!("native search smoke did not produce visible highlights");
    }

    let mut snapshot = terminal.take_snapshot();
    snapshot.search_highlights = highlights;
    let mut frame = FramePlanner::new(CellMetrics::default()).plan(&snapshot);
    apply_search_bar_overlay(&mut frame, &search, None, CellMetrics::default(), size);
    frame.refresh_stats(snapshot.size.rows, snapshot.size.cols);

    if frame.stats.search_highlight_rects == 0 || !frame.stats.search_active_visible {
        bail!(
            "native search smoke produced invalid highlight stats: rects={} active={}",
            frame.stats.search_highlight_rects,
            frame.stats.search_active_visible
        );
    }
    if !frame
        .glyphs
        .iter()
        .any(|glyph| glyph.text.contains("Find:"))
    {
        bail!("native search smoke did not render find bar text");
    }

    Ok(NativeSearchSmoke {
        query: search.query().to_owned(),
        match_count: search.match_count(),
        active_index: search.active_index(),
        visible_highlights: frame.stats.search_highlight_rects,
        active_visible: frame.stats.search_active_visible,
        status: search_count_label(&search),
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct NativeCommandBlockSmoke {
    pub completed_blocks: usize,
    pub selected_id: Option<u64>,
    pub command_copy: String,
    pub output_copy: String,
    pub overlay_rects: usize,
    pub frame_backgrounds: usize,
    pub folded_hidden_rows: usize,
    pub folded_second_compact_row: Option<u16>,
    pub folded_second_glyph_row: Option<u16>,
    pub folded_gutter_selected_id: Option<u64>,
}

pub(crate) fn native_command_block_smoke() -> Result<NativeCommandBlockSmoke> {
    let size = GridSize::new(4, 32);
    let mut terminal = BasicTerminal::new(size);
    let mut shell_integration = ShellIntegrationState::default();
    let mut clipboard = MemoryClipboardSink::default();
    let mut replies = Vec::new();

    terminal.feed(
        b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\echo native\x1b]133;C\x1b\\\r\nok\r\n\x1b]133;D;0\x1b\\",
    );
    apply_terminal_host_actions(
        terminal.drain_host_actions(),
        Osc52ClipboardPolicy::Disabled,
        &mut clipboard,
        &mut shell_integration,
        |bytes| {
            replies.extend_from_slice(bytes);
            Ok(())
        },
    )?;
    if !replies.is_empty() {
        bail!("native command block smoke produced unexpected terminal replies");
    }
    if shell_integration.completed_len() != 1 {
        bail!(
            "native command block smoke completed {} blocks, expected 1",
            shell_integration.completed_len()
        );
    }
    if !apply_command_block_command(
        &mut shell_integration,
        terminal.active_screen(),
        COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID,
    ) {
        bail!("native command block smoke did not handle latest command");
    }
    let selected_id = shell_integration.selected_block_id();
    if selected_id != Some(0) {
        bail!(
            "native command block smoke selected {:?}, expected Some(0)",
            selected_id
        );
    }
    if !copy_command_block_to_clipboard(
        &terminal,
        &shell_integration,
        CommandBlockCopyTarget::Command,
        &mut clipboard,
    )? {
        bail!("native command block smoke did not copy command text");
    }
    let command_copy = clipboard.clipboard_text.clone();
    if command_copy != "echo native" {
        bail!(
            "native command block smoke copied command {:?}, expected {:?}",
            command_copy,
            "echo native"
        );
    }
    if !copy_command_block_to_clipboard(
        &terminal,
        &shell_integration,
        CommandBlockCopyTarget::Output,
        &mut clipboard,
    )? {
        bail!("native command block smoke did not copy output text");
    }
    let output_copy = clipboard.clipboard_text.clone();
    if output_copy != "ok" {
        bail!(
            "native command block smoke copied output {:?}, expected {:?}",
            output_copy,
            "ok"
        );
    }

    let visible_row_anchors = terminal.visible_row_anchors();
    let mut frame = FramePlanner::new(CellMetrics::default()).plan(&terminal.take_snapshot());
    let gutter_rects = apply_command_block_gutter_overlay_with_anchors(
        &mut frame,
        &shell_integration,
        terminal.active_screen(),
        &visible_row_anchors,
        CellMetrics::default(),
        size,
    );
    let overlay_rects = apply_command_block_selection_overlay_with_anchors(
        &mut frame,
        &shell_integration,
        terminal.active_screen(),
        &visible_row_anchors,
        CellMetrics::default(),
        size,
    );
    frame.refresh_stats(size.rows, size.cols);
    if overlay_rects == 0 {
        bail!("native command block smoke did not render selected-block overlay");
    }
    if gutter_rects != 0 {
        bail!("native command block smoke rendered gutter for selected block only");
    }

    if !apply_command_block_command(
        &mut shell_integration,
        terminal.active_screen(),
        COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
    ) || shell_integration.selected_block_id().is_some()
    {
        bail!("native command block smoke did not clear selection");
    }

    let folded_size = GridSize::new(5, 32);
    let metrics = CellMetrics::default();
    let mut folded_terminal = BasicTerminal::new(folded_size);
    let mut folded_shell_integration = ShellIntegrationState::default();
    let mut folded_replies = Vec::new();

    folded_terminal.feed(
        b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ first\x1b]133;C\x1b\\\r\none\r\ntwo\x1b]133;D;0\x1b\\\r\n\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ second\x1b]133;C\x1b\\\r\nok\x1b]133;D;0\x1b\\",
    );
    apply_terminal_host_actions(
        folded_terminal.drain_host_actions(),
        Osc52ClipboardPolicy::Disabled,
        &mut clipboard,
        &mut folded_shell_integration,
        |bytes| {
            folded_replies.extend_from_slice(bytes);
            Ok(())
        },
    )?;
    if !folded_replies.is_empty() {
        bail!("native folded command block smoke produced unexpected terminal replies");
    }
    if folded_shell_integration.completed_len() != 2 {
        bail!(
            "native folded command block smoke completed {} blocks, expected 2",
            folded_shell_integration.completed_len()
        );
    }
    if folded_shell_integration.select_completed_block(0).is_none()
        || !apply_command_block_command(
            &mut folded_shell_integration,
            folded_terminal.active_screen(),
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        )
    {
        bail!("native folded command block smoke did not fold the first block");
    }

    let folded_visible_row_anchors = folded_terminal.visible_row_anchors();
    let compact_rows = folded_shell_integration.folded_compact_visual_rows(
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        folded_size.rows,
    );
    let folded_hidden_rows = compact_rows.iter().filter(|row| row.hidden).count();
    let folded_second_compact_row = compact_rows
        .iter()
        .find(|row| row.visible_row == 3)
        .and_then(|row| row.compact_row);
    let mut folded_frame = FramePlanner::new(metrics).plan(&folded_terminal.take_snapshot());
    apply_command_block_gutter_overlay_with_anchors(
        &mut folded_frame,
        &folded_shell_integration,
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        metrics,
        folded_size,
    );
    apply_command_block_status_label_overlay_with_anchors(
        &mut folded_frame,
        &folded_shell_integration,
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        None,
        metrics,
        folded_size,
    );
    apply_command_block_selection_overlay_with_anchors(
        &mut folded_frame,
        &folded_shell_integration,
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        metrics,
        folded_size,
    );
    apply_command_block_folded_frame_remap_with_anchors(
        &mut folded_frame,
        &folded_shell_integration,
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        metrics,
        folded_size,
    );
    let folded_second_glyph_row =
        glyph_row_containing_text(&folded_frame.glyphs, metrics, "second");
    let folded_visual_gutter_point = PixelPoint {
        x: metrics.padding.x + 2.0,
        y: metrics.padding.y + metrics.cell.height * 1.5,
    };
    let Some(folded_terminal_gutter_point) =
        command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
            &folded_shell_integration,
            folded_terminal.active_screen(),
            &folded_visible_row_anchors,
            folded_visual_gutter_point,
            metrics,
            folded_size,
        )
    else {
        bail!("native folded command block smoke did not remap compact gutter point");
    };
    let folded_gutter_selected_id = select_command_block_gutter_hit_for_pixel_point(
        &mut folded_shell_integration,
        folded_terminal.active_screen(),
        &folded_visible_row_anchors,
        folded_terminal_gutter_point,
        metrics,
        folded_size,
    );
    if folded_hidden_rows != 2
        || folded_second_compact_row != Some(1)
        || folded_second_glyph_row != Some(1)
        || folded_gutter_selected_id != Some(1)
        || folded_shell_integration.selected_block_id() != Some(1)
    {
        bail!(
            "native folded command block compact smoke mismatch: hidden_rows={} second_compact={:?} second_glyph_row={:?} gutter_selected={:?} selected={:?}",
            folded_hidden_rows,
            folded_second_compact_row,
            folded_second_glyph_row,
            folded_gutter_selected_id,
            folded_shell_integration.selected_block_id()
        );
    }

    Ok(NativeCommandBlockSmoke {
        completed_blocks: shell_integration.completed_len(),
        selected_id,
        command_copy,
        output_copy,
        overlay_rects,
        frame_backgrounds: frame.backgrounds.len(),
        folded_hidden_rows,
        folded_second_compact_row,
        folded_second_glyph_row,
        folded_gutter_selected_id,
    })
}

struct NativeWindowSessionStartup {
    size: GridSize,
    terminal: BasicTerminal,
    transport: LocalPtyTransport,
    local_tab_config: LocalPtyConfig,
    profile_action_sessions: NativeSessionRegistry,
    profile_action_session_runtimes: NativeSessionRuntimeRegistry<LocalPtyTransport>,
}

fn native_window_session_startup_for_launch(
    size: GridSize,
    max_scrollback_lines: usize,
    local_tab_config: LocalPtyConfig,
    cursor_shape: CursorShape,
    cursor_blink: bool,
) -> Result<NativeWindowSessionStartup> {
    let terminal =
        basic_terminal_with_cursor_style(size, max_scrollback_lines, cursor_shape, cursor_blink);
    let transport = LocalPtyTransport::spawn(local_tab_config.clone())?;
    Ok(NativeWindowSessionStartup {
        size,
        terminal,
        transport,
        local_tab_config,
        profile_action_sessions: NativeSessionRegistry::default(),
        profile_action_session_runtimes: NativeSessionRuntimeRegistry::default(),
    })
}

fn native_window_session_startup_for_restore(
    snapshot: RestartSnapshotV1,
    fallback_local_tab_config: LocalPtyConfig,
    max_scrollback_lines: usize,
    cursor_shape: CursorShape,
    cursor_blink: bool,
) -> Result<NativeWindowSessionStartup> {
    let size = snapshot.window.grid_size();
    let active_tab = snapshot
        .tabs
        .get(snapshot.active_tab_index)
        .context("restart snapshot active tab missing")?;
    let active_config = active_tab.launch.to_local_pty_config(size);
    let transport =
        LocalPtyTransport::spawn(active_config).context("spawn active restored local pty")?;
    let terminal =
        basic_terminal_with_cursor_style(size, max_scrollback_lines, cursor_shape, cursor_blink);
    let mut sessions = NativeSessionRegistry::default();
    let mut parked_sessions = NativeSessionRuntimeRegistry::default();

    for (index, tab) in snapshot.tabs.iter().enumerate() {
        let active = index == snapshot.active_tab_index;
        let session = native_session_from_restart_tab(tab);
        let launch = native_session_launch_metadata_from_restart_tab(tab, size);
        let session_id = sessions.insert_with_active_state(session, active);
        sessions.set_launch_metadata(session_id, launch.clone());
        if active {
            continue;
        }

        let transport = LocalPtyTransport::spawn(launch.config)
            .with_context(|| format!("spawn restored tab {}", tab.profile_id))?;
        parked_sessions.park_or_replace(
            session_id,
            NativeSessionRuntime {
                transport,
                terminal: basic_terminal_with_cursor_style(
                    size,
                    max_scrollback_lines,
                    cursor_shape,
                    cursor_blink,
                ),
                terminal_search: TerminalSearch::default(),
                shell_integration: ShellIntegrationState::default(),
            },
        );
    }

    let local_tab_config = snapshot
        .tabs
        .iter()
        .find(|tab| tab.kind == RestartTabKindV1::Local)
        .map(|tab| tab.launch.to_local_pty_config(size))
        .unwrap_or(fallback_local_tab_config);

    Ok(NativeWindowSessionStartup {
        size,
        terminal,
        transport,
        local_tab_config,
        profile_action_sessions: sessions,
        profile_action_session_runtimes: parked_sessions,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeUpdateNotice {
    running_build_id: String,
    installed_marker: InstalledBuildMarkerV1,
}

impl NativeUpdateNotice {
    fn installed_build_id(&self) -> &str {
        &self.installed_marker.build_id
    }
}

#[derive(Clone, Debug)]
struct NativeUpdateMonitor {
    marker_path: Option<PathBuf>,
    running: RunningBuildIdentity,
    next_check: Instant,
    interval: Duration,
}

impl NativeUpdateMonitor {
    fn new(now: Instant) -> Self {
        let marker_path = default_install_state_path().ok();
        let startup_marker = marker_path
            .as_ref()
            .and_then(|path| read_installed_build_marker(path).ok().flatten());
        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("witty"));
        let running = running_build_identity(
            current_exe,
            startup_marker.as_ref(),
            env!("CARGO_PKG_VERSION"),
        );
        Self {
            marker_path,
            running,
            next_check: now,
            interval: INSTALLED_UPDATE_CHECK_INTERVAL,
        }
    }

    fn check(&self) -> Result<Option<NativeUpdateNotice>> {
        let Some(marker_path) = &self.marker_path else {
            return Ok(None);
        };
        let marker = read_installed_build_marker(marker_path)?;
        let status = installed_update_status(&self.running, marker);
        if !status.update_needed {
            return Ok(None);
        }
        let installed_marker = status
            .installed_marker
            .context("update-needed status did not include installed marker")?;
        Ok(Some(NativeUpdateNotice {
            running_build_id: status.running_build_id,
            installed_marker,
        }))
    }

    fn schedule_next(&mut self, now: Instant) {
        self.next_check = now.checked_add(self.interval).unwrap_or(now);
    }

    fn due(&self, now: Instant) -> bool {
        now >= self.next_check
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeUpdateNoticeTarget {
    Row,
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeUpdateNoticeHit {
    target: NativeUpdateNoticeTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeEmptySessionWelcomeTarget {
    NewLocalShell,
    CommandPalette,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEmptySessionWelcomeHit {
    target: NativeEmptySessionWelcomeTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEmptySessionWelcomeButtonSpan {
    target: NativeEmptySessionWelcomeTarget,
    start_col: u16,
    end_col: u16,
}

struct TerminalWindowApp {
    event_loop_proxy: Option<EventLoopProxy<NativeWindowEvent>>,
    window: Option<Arc<Window>>,
    renderer: Option<WgpuRectRenderer>,
    app: TerminalApp<LocalPtyTransport>,
    terminal: BasicTerminal,
    frame: FramePlan,
    command_palette: CommandPalette,
    command_block_action_menu: CommandBlockActionMenu,
    terminal_search: TerminalSearch,
    local_tab_config: LocalPtyConfig,
    profile_actions: NativeProfileActionBridge,
    resolved_profile_action_handoffs: NativeResolvedProfileActionHandoffQueue,
    deferred_profile_action_starts: NativeResolvedProfileActionHandoffQueue,
    profile_action_start_plans: NativeProfileActionStartPlanQueue,
    profile_action_sessions: NativeSessionRegistry,
    profile_action_session_runtimes: NativeSessionRuntimeRegistry<LocalPtyTransport>,
    active_session_close_fallback_policy: NativeActiveSessionCloseFallbackPolicy,
    session_tab_notice: Option<NativeSessionTabStripNotice>,
    session_tab_position: NativeSessionTabPosition,
    session_tab_label_style: NativeSessionTabLabelStyle,
    session_tab_display_policy: NativeSessionTabDisplayPolicy,
    session_switcher: NativeSessionSwitcher,
    shell_integration: ShellIntegrationState,
    ime_composition: ImeComposition,
    metrics: CellMetrics,
    size: GridSize,
    modifiers: Modifiers,
    pointer_position: Option<PhysicalPosition<f64>>,
    native_cursor_icon: Option<CursorIcon>,
    window_fullscreen: bool,
    fullscreen_target_monitor: Option<MonitorHandle>,
    fullscreen_target_size: Option<PhysicalSize<u32>>,
    fullscreen_target_scale_factor: Option<f64>,
    fullscreen_windowed_restore_size: Option<PhysicalSize<u32>>,
    fullscreen_reaffirm_until: Option<Instant>,
    fullscreen_next_reaffirm: Option<Instant>,
    pending_windowed_restore_size: Option<PhysicalSize<u32>>,
    pending_windowed_restore_attempts: u8,
    fullscreen_titlebar_visible: bool,
    fullscreen_titlebar_hide_deadline: Option<Instant>,
    titlebar_menu_open: bool,
    hovered_titlebar_hit: Option<NativeTitleBarHit>,
    hovered_session_tab: Option<NativeSessionTabStripHit>,
    hovered_profile_action: Option<NativeProfileActionOverlayHit>,
    hovered_update_notice: Option<NativeUpdateNoticeHit>,
    hovered_empty_session_welcome: Option<NativeEmptySessionWelcomeHit>,
    hovered_hyperlink: Option<HyperlinkId>,
    hovered_command_block_id: Option<u64>,
    about_status_deadline: Option<Instant>,
    selection_anchor: Option<CellPoint>,
    last_left_click: Option<ClickStamp>,
    mouse_report: NativeMouseReportState,
    mouse_override_policy: MouseSelectionOverridePolicy,
    osc52_clipboard_policy: Osc52ClipboardPolicy,
    show_diagnostics: bool,
    report_startup: bool,
    window_close_requested: bool,
    fallback_local_session_requested: bool,
    restart_exit_requested: bool,
    empty_session_welcome: bool,
    empty_session_notice: Option<String>,
    started: Instant,
    exited: bool,
    clipboard: Box<dyn ClipboardSink>,
    default_window_title: String,
    window_title: String,
    synchronized_output_deadline: Option<Instant>,
    cursor_blink: CursorBlinkState,
    text_blink: TextBlinkState,
    font_config: RendererFontConfig,
    visual_config: RendererVisualConfig,
    background_image_path: Option<PathBuf>,
    background_image_target_size: Option<PhysicalSize<u32>>,
    background_image_generation: u64,
    background_image_load_pending: bool,
    background_image_poll_reported: bool,
    background_image_result_ready: Arc<AtomicBool>,
    background_image_results: Arc<Mutex<Vec<BackgroundImageLoadResult>>>,
    font_data: Vec<Vec<u8>>,
    initial_window_size: Option<GridSize>,
    first_frame_reported: bool,
    update_monitor: NativeUpdateMonitor,
    update_notice: Option<NativeUpdateNotice>,
}

impl TerminalWindowApp {
    fn new(
        event_loop_proxy: Option<EventLoopProxy<NativeWindowEvent>>,
        wasm_plugins: Vec<PathBuf>,
        smoke: WindowSmokeOptions,
        default_window_title: Option<String>,
        launch_program: Option<String>,
        launch_args: Vec<String>,
        launch_cwd: Option<PathBuf>,
        launch_env: Vec<(String, String)>,
        mouse_override_policy: MouseSelectionOverridePolicy,
        osc52_clipboard_policy: Osc52ClipboardPolicy,
        max_scrollback_lines: usize,
        font_family: Option<String>,
        font_size: Option<u16>,
        terminal_padding: Option<u16>,
        background_opacity: Option<f32>,
        background_image_path: Option<PathBuf>,
        background_image_fit: Option<RendererBackgroundImageFit>,
        cursor_shape: CursorShape,
        cursor_blink: bool,
        session_tab_position: NativeSessionTabPosition,
        session_tab_label_style: NativeSessionTabLabelStyle,
        session_tab_display_policy: NativeSessionTabDisplayPolicy,
        font_paths: Vec<PathBuf>,
        restore_state: Option<RestartSnapshotV1>,
    ) -> Result<Self> {
        let restoring = restore_state.is_some();
        let size = GridSize::new(24, 80);
        let size = restore_state
            .as_ref()
            .map(|snapshot| snapshot.window.grid_size())
            .or(smoke.initial_size)
            .unwrap_or(size);
        let initial_window_size = if restoring {
            Some(size)
        } else {
            smoke.initial_size
        };
        let local_tab_config =
            local_pty_config_for_launch(size, launch_program, launch_args, launch_cwd, launch_env)?;
        let startup = match restore_state {
            Some(snapshot) => native_window_session_startup_for_restore(
                snapshot,
                local_tab_config.clone(),
                max_scrollback_lines,
                cursor_shape,
                cursor_blink,
            )?,
            None => native_window_session_startup_for_launch(
                size,
                max_scrollback_lines,
                local_tab_config,
                cursor_shape,
                cursor_blink,
            )?,
        };
        let mut app = TerminalApp::new(startup.transport, startup.size);
        app.install_builtin_plugin(BuiltInCommandsPlugin)?;
        for command in native_window_command_registrations() {
            app.register_command(command)?;
        }
        for command in search_command_registrations() {
            app.register_command(command)?;
        }
        for command in command_block_command_registrations() {
            app.register_command(command)?;
        }
        install_wasm_plugins(&mut app, &wasm_plugins)?;
        app.dispatch_plugin_event(PluginEvent::AppStarted)?;
        let font_config = match font_size {
            Some(font_size) => RendererFontConfig::with_font_size(font_family, font_size),
            None => RendererFontConfig::new(font_family),
        };
        let font_config = match terminal_padding {
            Some(padding) => font_config.with_terminal_padding(f32::from(padding)),
            None => font_config,
        };
        let visual_config = match background_opacity {
            Some(opacity) => RendererVisualConfig::default().with_background_opacity(opacity),
            None => RendererVisualConfig::default(),
        };
        let visual_config = match background_image_fit {
            Some(fit) => visual_config.with_background_image_fit(fit),
            None => visual_config,
        };
        let metrics = font_config.cell_metrics();
        app.set_cell_metrics(metrics);
        let font_data = load_window_font_data(&font_paths)?;
        let mut command_palette = CommandPalette::default();
        if smoke.open_command_palette {
            command_palette.open(app.commands());
        }
        let default_window_title =
            default_window_title.unwrap_or_else(|| DEFAULT_WINDOW_TITLE.to_owned());
        let started = Instant::now();
        let mut update_monitor = NativeUpdateMonitor::new(started);
        let update_notice = match update_monitor.check() {
            Ok(notice) => notice,
            Err(err) => {
                eprintln!("failed to check installed Witty update marker: {err:#}");
                None
            }
        };
        update_monitor.schedule_next(started);

        let mut window_app = Self {
            event_loop_proxy,
            window: None,
            renderer: None,
            app,
            terminal: startup.terminal,
            frame: FramePlan::default(),
            command_palette,
            command_block_action_menu: CommandBlockActionMenu::default(),
            terminal_search: TerminalSearch::default(),
            local_tab_config: startup.local_tab_config,
            profile_actions: NativeProfileActionBridge::default(),
            resolved_profile_action_handoffs: NativeResolvedProfileActionHandoffQueue::default(),
            deferred_profile_action_starts: NativeResolvedProfileActionHandoffQueue::default(),
            profile_action_start_plans: NativeProfileActionStartPlanQueue::default(),
            profile_action_sessions: startup.profile_action_sessions,
            profile_action_session_runtimes: startup.profile_action_session_runtimes,
            active_session_close_fallback_policy: smoke.last_active_close_policy.into(),
            session_tab_notice: None,
            session_tab_position,
            session_tab_label_style,
            session_tab_display_policy,
            session_switcher: NativeSessionSwitcher::default(),
            shell_integration: ShellIntegrationState::default(),
            ime_composition: ImeComposition::default(),
            metrics,
            size: startup.size,
            modifiers: Modifiers::default(),
            pointer_position: None,
            native_cursor_icon: None,
            window_fullscreen: false,
            fullscreen_target_monitor: None,
            fullscreen_target_size: None,
            fullscreen_target_scale_factor: None,
            fullscreen_windowed_restore_size: None,
            fullscreen_reaffirm_until: None,
            fullscreen_next_reaffirm: None,
            pending_windowed_restore_size: None,
            pending_windowed_restore_attempts: 0,
            fullscreen_titlebar_visible: false,
            fullscreen_titlebar_hide_deadline: None,
            titlebar_menu_open: false,
            hovered_titlebar_hit: None,
            hovered_session_tab: None,
            hovered_profile_action: None,
            hovered_update_notice: None,
            hovered_empty_session_welcome: None,
            hovered_hyperlink: None,
            hovered_command_block_id: None,
            about_status_deadline: None,
            selection_anchor: None,
            last_left_click: None,
            mouse_report: NativeMouseReportState::default(),
            mouse_override_policy,
            osc52_clipboard_policy,
            show_diagnostics: smoke.show_diagnostics,
            report_startup: smoke.report_startup,
            window_close_requested: false,
            fallback_local_session_requested: false,
            restart_exit_requested: false,
            empty_session_welcome: false,
            empty_session_notice: None,
            started,
            exited: false,
            clipboard: Box::new(SystemClipboardSink::new()),
            window_title: default_window_title.clone(),
            default_window_title,
            synchronized_output_deadline: None,
            cursor_blink: CursorBlinkState::default(),
            text_blink: TextBlinkState::default(),
            font_config,
            visual_config,
            background_image_path,
            background_image_target_size: None,
            background_image_generation: 0,
            background_image_load_pending: false,
            background_image_poll_reported: false,
            background_image_result_ready: Arc::new(AtomicBool::new(false)),
            background_image_results: Arc::new(Mutex::new(Vec::new())),
            font_data,
            initial_window_size,
            first_frame_reported: false,
            update_monitor,
            update_notice,
        };
        let _ = window_app.refresh_pending_profile_actions();
        window_app.rebuild_frame();
        Ok(window_app)
    }

    fn resize_grid(&mut self, physical_size: PhysicalSize<u32>) {
        let size = grid_size_for_window_with_reserved_height(
            physical_size,
            self.metrics,
            self.titlebar_reserved_height(),
        );
        if size != self.size {
            self.size = size;
            self.sync_all_session_runtime_sizes();
            let _ = self.refresh_session_tab_hover_for_current_pointer();
            let _ = self.refresh_profile_action_hover_for_current_pointer();
            let _ = self.refresh_empty_session_welcome_hover_for_current_pointer();
        }
        self.rebuild_frame();
    }

    fn sync_all_session_runtime_sizes(&mut self) {
        let size = self.size;
        self.terminal.resize(size);
        if let Err(err) = self.app.resize_transport(size) {
            eprintln!("failed to resize active pty: {err:#}");
        }
        self.profile_action_session_runtimes.resize_all(size);
        self.refresh_search_after_terminal_change();
    }

    fn apply_window_scale_factor(&mut self, scale_factor: f64) -> bool {
        let scale_factor = sane_window_scale_factor(scale_factor);
        if (self.font_config.font_scale_factor() - scale_factor).abs() <= f32::EPSILON {
            return false;
        }

        self.font_config = self.font_config.clone().with_scale_factor(scale_factor);
        self.metrics = self.font_config.cell_metrics();
        self.app.set_cell_metrics(self.metrics);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_font_config(self.font_config.clone());
        }
        true
    }

    fn effective_window_scale_factor(&self, window: &Window, fallback: f64) -> f64 {
        if self.window_fullscreen {
            self.fullscreen_target_scale_factor.unwrap_or(fallback)
        } else {
            window.scale_factor()
        }
    }

    fn apply_runtime_font_size_action(&mut self, action: RuntimeFontSizeAction) {
        let next_config = runtime_font_config_after_action(&self.font_config, action);
        if next_config == self.font_config {
            return;
        }

        self.font_config = next_config;
        self.metrics = self.font_config.cell_metrics();
        self.app.set_cell_metrics(self.metrics);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_font_config(self.font_config.clone());
        }

        if let Some(physical_size) = self.window.as_ref().map(|window| window.inner_size()) {
            self.resize_grid(physical_size);
        }
        self.refresh_search_after_terminal_change();
        let _ = self.refresh_session_tab_hover_for_current_pointer();
        let _ = self.refresh_profile_action_hover_for_current_pointer();
        let _ = self.refresh_empty_session_welcome_hover_for_current_pointer();
        self.rebuild_frame();
    }

    fn rebuild_frame(&mut self) {
        let visible_row_anchors = self.terminal.visible_row_anchors();
        let mut snapshot = self.terminal.take_snapshot();
        snapshot.search_highlights = self.visible_search_highlights();
        snapshot.hovered_hyperlink = self.hovered_hyperlink;
        let cursor = snapshot.cursor;
        let blink_visible = self.text_blink.apply_to_snapshot(&snapshot, Instant::now());
        self.app.set_blink_visible(blink_visible);
        self.app.set_snapshot(snapshot);
        self.sync_window_title();
        let mut frame = self.app.frame_plan();
        let base_stats = frame.stats;
        let reused_rows = frame.stats.reused_rows;
        let rebuilt_rows = frame.stats.rebuilt_rows;
        apply_command_block_gutter_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        apply_command_block_gutter_hover_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.hovered_command_block_id,
            self.metrics,
            self.size,
        );
        apply_command_block_selection_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.hovered_command_block_id,
            self.metrics,
            self.size,
        );
        apply_command_block_action_menu_overlay(
            &mut frame,
            &self.command_block_action_menu,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        if self.text_input_target() == TextInputTarget::Terminal {
            apply_ime_preedit_overlay(
                &mut frame,
                &self.ime_composition,
                cursor,
                self.metrics,
                self.size,
            );
        }
        apply_command_block_folded_frame_remap_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        let session_tabs = self.profile_action_sessions.tab_rows();
        if self
            .session_tab_display_policy
            .should_show(session_tabs.len())
        {
            apply_native_session_tab_strip_overlay(
                &mut frame,
                &session_tabs,
                self.hovered_session_tab,
                self.session_tab_notice,
                self.session_tab_position,
                self.session_tab_label_style,
                self.metrics,
                self.size,
            );
        }
        apply_empty_session_welcome_overlay(
            &mut frame,
            self.empty_session_welcome_visible(),
            self.empty_session_notice.as_deref(),
            self.hovered_empty_session_welcome,
            self.metrics,
            self.size,
        );
        apply_native_update_notice_overlay(
            &mut frame,
            self.update_notice.as_ref(),
            self.hovered_update_notice,
            self.metrics,
            self.size,
        );
        apply_profile_action_overlay(
            &mut frame,
            self.profile_actions.snapshot(),
            self.hovered_profile_action,
            self.metrics,
            self.size,
        );
        apply_about_status_overlay(
            &mut frame,
            self.about_status_deadline.is_some(),
            self.metrics,
            self.size,
        );
        apply_search_bar_overlay(
            &mut frame,
            &self.terminal_search,
            self.active_search_ime_composition(),
            self.metrics,
            self.size,
        );
        apply_command_palette_overlay(
            &mut frame,
            &self.command_palette,
            self.active_command_palette_ime_composition(),
            self.app.commands(),
            self.metrics,
            self.size,
        );
        apply_session_switcher_overlay(
            &mut frame,
            &self.session_switcher,
            &session_tabs,
            self.session_tab_label_style,
            self.metrics,
            self.size,
        );
        if self.show_diagnostics {
            apply_frame_diagnostics_overlay(&mut frame, base_stats, self.metrics, self.size);
        }
        self.cursor_blink.apply_to_frame(
            &mut frame,
            cursor,
            self.text_input_target(),
            Instant::now(),
        );
        offset_frame_y(&mut frame, self.terminal_content_y_offset());
        apply_native_titlebar_overlay(&mut frame, self.titlebar_overlay(), self.metrics);
        frame.refresh_stats_with_rows(self.size.rows, self.size.cols, reused_rows, rebuilt_rows);
        self.frame = frame;
        self.sync_ime_cursor_area(self.active_ime_cursor(cursor.position));
    }

    fn visible_search_highlights(&self) -> Vec<SearchHighlight> {
        if !self.terminal_search.is_open() {
            return Vec::new();
        }

        self.terminal.visible_search_highlights(
            self.terminal_search.matches(),
            self.terminal_search.active_match(),
        )
    }

    fn sync_window_title(&mut self) {
        let title = terminal_window_title(self.app.title(), &self.default_window_title);
        if self.window_title == title {
            return;
        }

        self.window_title = title;
        if let Some(window) = &self.window {
            window.set_title(&self.window_title);
        }
    }

    fn titlebar_visible(&self) -> bool {
        !self.window_fullscreen || self.fullscreen_titlebar_visible || self.titlebar_menu_open
    }

    fn titlebar_reserves_space(&self) -> bool {
        !self.window_fullscreen
    }

    fn titlebar_reserved_height(&self) -> f32 {
        if self.titlebar_reserves_space() {
            native_titlebar_height(self.window_scale_factor())
        } else {
            0.0
        }
    }

    fn terminal_content_y_offset(&self) -> f32 {
        let titlebar_reserved_height = self.titlebar_reserved_height();
        titlebar_reserved_height
            + self
                .window
                .as_ref()
                .map(|window| {
                    terminal_bottom_align_y_offset(
                        window.inner_size(),
                        self.metrics,
                        self.size,
                        titlebar_reserved_height,
                    )
                })
                .unwrap_or(0.0)
    }

    fn titlebar_window_width(&self) -> f32 {
        self.window
            .as_ref()
            .map(|window| window.inner_size().width as f32)
            .unwrap_or_else(|| {
                self.metrics.padding.x * 2.0 + f32::from(self.size.cols) * self.metrics.cell.width
            })
    }

    fn window_scale_factor(&self) -> f32 {
        self.font_config.font_scale_factor()
    }

    fn titlebar_overlay(&self) -> NativeTitleBarOverlay<'_> {
        NativeTitleBarOverlay {
            title: &self.window_title,
            window_width: self.titlebar_window_width(),
            scale_factor: self.window_scale_factor(),
            visible: self.titlebar_visible(),
            menu_open: self.titlebar_menu_open,
            hovered_hit: self.hovered_titlebar_hit,
            maximized: self
                .window
                .as_ref()
                .is_some_and(|window| window.is_maximized()),
        }
    }

    fn sync_ime_cursor_area(&self, cursor: CellPoint) {
        let Some(window) = &self.window else {
            return;
        };

        let mut origin = cell_origin(cursor, self.metrics);
        origin.y += self.terminal_content_y_offset();
        let width = self.metrics.cell.width.ceil().max(1.0) as u32;
        let height = self.metrics.cell.height.ceil().max(1.0) as u32;
        window.set_ime_cursor_area(
            PhysicalPosition::new(origin.x.round() as i32, origin.y.round() as i32),
            PhysicalSize::new(width, height),
        );
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn request_redraw_if_changed(&self, changed: bool) {
        if changed {
            self.request_redraw();
        }
    }

    fn ensure_background_image_load(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_size: PhysicalSize<u32>,
    ) {
        if self.background_image_path.is_none() {
            return;
        }
        let target_size = background_image_target_size(event_loop, window_size);
        if self.background_image_target_size.is_some_and(|size| {
            size.width >= target_size.width && size.height >= target_size.height
        }) {
            return;
        }
        self.background_image_target_size = Some(target_size);
        self.background_image_generation = self.background_image_generation.saturating_add(1);
        let generation = self.background_image_generation;
        let Some(path) = self.background_image_path.clone() else {
            return;
        };
        let results = Arc::clone(&self.background_image_results);
        let result_ready = Arc::clone(&self.background_image_result_ready);
        let event_loop_proxy = self.event_loop_proxy.clone();
        let window = self.window.as_ref().cloned();
        self.background_image_load_pending = true;
        self.background_image_poll_reported = false;
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.report_startup {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "witty.background_image_load_started",
                    "path": path.display().to_string(),
                    "target_width": target_size.width,
                    "target_height": target_size.height,
                })
            );
        }
        let report_startup = self.report_startup;

        thread::spawn(move || {
            let started = Instant::now();
            let image = load_window_background_image(&path, target_size);
            if report_startup {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "event": "witty.background_image_decoded",
                        "path": path.display().to_string(),
                        "loaded": image.is_some(),
                        "target_width": target_size.width,
                        "target_height": target_size.height,
                        "image_width": image.as_ref().map(RendererBackgroundImage::width),
                        "image_height": image.as_ref().map(RendererBackgroundImage::height),
                        "elapsed_ms": started.elapsed().as_millis(),
                    })
                );
            }
            let result = BackgroundImageLoadResult {
                generation,
                path,
                target_size,
                image,
                elapsed: started.elapsed(),
            };
            match results.lock() {
                Ok(mut results) => {
                    results.push(result);
                    result_ready.store(true, Ordering::Release);
                    if report_startup {
                        eprintln!(
                            "{}",
                            serde_json::json!({
                                "event": "witty.background_image_result_stored",
                            })
                        );
                    }
                    if let Some(proxy) = event_loop_proxy {
                        if let Err(err) = proxy.send_event(NativeWindowEvent::BackgroundImageReady)
                        {
                            if report_startup {
                                eprintln!(
                                    "{}",
                                    serde_json::json!({
                                        "event": "witty.background_image_ready_dispatch_failed",
                                        "error": format!("{err:?}"),
                                    })
                                );
                            }
                        } else if report_startup {
                            eprintln!(
                                "{}",
                                serde_json::json!({
                                    "event": "witty.background_image_ready_dispatched",
                                })
                            );
                        }
                    }
                }
                Err(err) if report_startup => {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "event": "witty.background_image_result_store_failed",
                            "error": err.to_string(),
                        })
                    );
                }
                Err(_) => {}
            }
            if let Some(window) = window {
                window.request_redraw();
            }
        });
    }

    fn apply_background_image_results(&mut self) -> bool {
        if !self
            .background_image_result_ready
            .swap(false, Ordering::Acquire)
        {
            return false;
        }
        let results = match self.background_image_results.lock() {
            Ok(mut results) => results.drain(..).collect::<Vec<_>>(),
            Err(err) => {
                eprintln!("failed to lock background image results: {err}");
                Vec::new()
            }
        };
        let mut changed = false;
        for result in results {
            if result.generation == self.background_image_generation {
                self.background_image_load_pending = false;
            }
            changed |= self.apply_background_image_loaded(result);
        }
        changed
    }

    fn schedule_background_image_result_poll(&mut self, event_loop: &ActiveEventLoop) {
        if !self.background_image_load_pending {
            return;
        }
        if self.report_startup && !self.background_image_poll_reported {
            self.background_image_poll_reported = true;
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "witty.background_image_poll_active",
                    "generation": self.background_image_generation,
                })
            );
        }
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn apply_background_image_loaded(&mut self, result: BackgroundImageLoadResult) -> bool {
        if result.generation != self.background_image_generation {
            return false;
        }

        let image_width = result.image.as_ref().map(RendererBackgroundImage::width);
        let image_height = result.image.as_ref().map(RendererBackgroundImage::height);
        let loaded = result.image.is_some();
        let visual_config = self
            .visual_config
            .clone()
            .with_background_image(result.image);
        self.visual_config = visual_config.clone();
        if self.report_startup {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "witty.background_image_apply_started",
                    "path": result.path.display().to_string(),
                    "loaded": loaded,
                    "target_width": result.target_size.width,
                    "target_height": result.target_size.height,
                    "image_width": image_width,
                    "image_height": image_height,
                })
            );
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_visual_config(visual_config);
        }
        if self.report_startup {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "witty.background_image_loaded",
                    "path": result.path.display().to_string(),
                    "loaded": loaded,
                    "target_width": result.target_size.width,
                    "target_height": result.target_size.height,
                    "image_width": image_width,
                    "image_height": image_height,
                    "background_opacity": self.visual_config.background_opacity(),
                    "background_image_fit": self.visual_config.background_image_fit().as_config_value(),
                    "elapsed_ms": result.elapsed.as_millis(),
                })
            );
        }
        self.request_redraw();
        true
    }

    fn refresh_pending_profile_actions(&mut self) -> Option<NativeProfileActionBridgeEvent> {
        let store = match default_profile_action_store_snapshot() {
            Ok(store) => store,
            Err(err) => {
                eprintln!("failed to load profile store snapshot: {err:#}");
                return None;
            }
        };
        match self.profile_actions.refresh(&self.app, &store) {
            Ok(event) => {
                let _ = self.profile_actions.snapshot();
                Some(event)
            }
            Err(err) => {
                eprintln!("failed to refresh pending profile actions: {err:#}");
                None
            }
        }
    }

    fn refresh_profile_action_hover_for_current_pointer(&mut self) -> bool {
        match self.pointer_position {
            Some(position) => self.set_hovered_profile_action_for_position(position),
            None => {
                let changed = self.hovered_profile_action.is_some();
                self.hovered_profile_action = None;
                changed
            }
        }
    }

    fn refresh_update_notice_hover_for_current_pointer(&mut self) -> bool {
        match self.pointer_position {
            Some(position) => self.set_hovered_update_notice_for_position(position),
            None => {
                let changed = self.hovered_update_notice.is_some();
                self.hovered_update_notice = None;
                changed
            }
        }
    }

    fn refresh_empty_session_welcome_hover_for_current_pointer(&mut self) -> bool {
        match self.pointer_position {
            Some(position) => self.set_hovered_empty_session_welcome_for_position(position),
            None => {
                let changed = self.hovered_empty_session_welcome.is_some();
                self.hovered_empty_session_welcome = None;
                changed
            }
        }
    }

    fn refresh_session_tab_hover_for_current_pointer(&mut self) -> bool {
        match self.pointer_position {
            Some(position) => self.set_hovered_session_tab_for_position(position),
            None => {
                let changed = self.hovered_session_tab.is_some();
                self.hovered_session_tab = None;
                changed
            }
        }
    }

    fn refresh_titlebar_hover_for_current_pointer(&mut self) -> bool {
        match self.pointer_position {
            Some(position) => self.update_titlebar_for_cursor_position(position),
            None => {
                let changed = self.hovered_titlebar_hit.is_some();
                self.hovered_titlebar_hit = None;
                changed
            }
        }
    }

    fn update_titlebar_for_cursor_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_visible = self.fullscreen_titlebar_visible;
        let previous_deadline = self.fullscreen_titlebar_hide_deadline;
        let previous_hover = self.hovered_titlebar_hit;
        let point = pixel_point_for_position(position);

        if self.window_fullscreen {
            if let Some(point) = point {
                let scale_factor = self.window_scale_factor();
                let over_reveal_zone =
                    f64::from(point.y) <= native_titlebar_fullscreen_reveal_zone(scale_factor);
                let over_titlebar = self.fullscreen_titlebar_visible
                    && point.y >= 0.0
                    && point.y < native_titlebar_height(scale_factor);
                let over_menu = self.titlebar_menu_open
                    && native_titlebar_menu_rect(scale_factor).contains_point(point);
                if over_reveal_zone || over_titlebar || over_menu {
                    self.fullscreen_titlebar_visible = true;
                    self.fullscreen_titlebar_hide_deadline = None;
                } else if self.fullscreen_titlebar_visible
                    && !self.titlebar_menu_open
                    && self.fullscreen_titlebar_hide_deadline.is_none()
                {
                    self.fullscreen_titlebar_hide_deadline =
                        Instant::now().checked_add(CUSTOM_TITLEBAR_FULLSCREEN_HIDE_DELAY);
                }
            }
        } else {
            self.fullscreen_titlebar_visible = false;
            self.fullscreen_titlebar_hide_deadline = None;
        }

        self.hovered_titlebar_hit = self.titlebar_hit_test(position).and_then(|hit| match hit {
            NativeTitleBarHit::DragRegion => None,
            hit => Some(hit),
        });

        previous_visible != self.fullscreen_titlebar_visible
            || previous_deadline != self.fullscreen_titlebar_hide_deadline
            || previous_hover != self.hovered_titlebar_hit
    }

    fn clear_content_hover_state(&mut self) -> bool {
        let changed = self.hovered_update_notice.is_some()
            || self.hovered_profile_action.is_some()
            || self.hovered_empty_session_welcome.is_some()
            || self.hovered_session_tab.is_some()
            || self.hovered_hyperlink.is_some()
            || self.hovered_command_block_id.is_some();
        self.hovered_update_notice = None;
        self.hovered_profile_action = None;
        self.hovered_empty_session_welcome = None;
        self.hovered_session_tab = None;
        self.hovered_hyperlink = None;
        self.hovered_command_block_id = None;
        changed
    }

    fn titlebar_consumes_position(&self, position: PhysicalPosition<f64>) -> bool {
        self.titlebar_hit_test(position).is_some()
    }

    fn titlebar_hit_test(&self, position: PhysicalPosition<f64>) -> Option<NativeTitleBarHit> {
        let point = pixel_point_for_position(position)?;
        let scale_factor = self.window_scale_factor();

        if self.titlebar_menu_open {
            if let Some(item) = native_titlebar_menu_item_for_point(point, scale_factor) {
                return Some(NativeTitleBarHit::MenuItem(item));
            }
        }

        if !self.titlebar_visible()
            || point.y < 0.0
            || point.y >= native_titlebar_height(scale_factor)
        {
            return None;
        }

        for button in native_titlebar_buttons() {
            if let Some(rect) =
                native_titlebar_button_rect(button, self.titlebar_window_width(), scale_factor)
            {
                if rect.contains_point(point) {
                    return Some(NativeTitleBarHit::Button(button));
                }
            }
        }

        Some(NativeTitleBarHit::DragRegion)
    }

    fn empty_session_welcome_visible(&self) -> bool {
        self.empty_session_welcome
    }

    fn enter_empty_session_welcome(&mut self) {
        self.empty_session_welcome = true;
        self.empty_session_notice = None;
        self.command_block_action_menu.close();
        self.ime_composition.clear_preedit();
        self.terminal_search.close();
        self.selection_anchor = None;
        self.last_left_click = None;
        self.hovered_session_tab = None;
        self.session_tab_notice = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.hovered_hyperlink = None;
        self.hovered_command_block_id = None;
        self.profile_actions.set_start_success(None);
        self.profile_actions.set_start_failure(None);
        self.profile_action_sessions.clear();
        self.profile_action_session_runtimes.clear();
        self.shell_integration = ShellIntegrationState::default();
        self.synchronized_output_deadline = None;
        self.cursor_blink = CursorBlinkState::default();
        self.text_blink = TextBlinkState::default();
        let _ = self.refresh_empty_session_welcome_hover_for_current_pointer();
    }

    fn clear_empty_session_welcome(&mut self) {
        self.empty_session_welcome = false;
        self.empty_session_notice = None;
        self.hovered_empty_session_welcome = None;
    }

    fn take_window_close_request(&mut self) -> bool {
        take_native_event_request_flag(&mut self.window_close_requested)
    }

    fn take_fallback_local_session_request(&mut self) -> bool {
        take_native_event_request_flag(&mut self.fallback_local_session_requested)
    }

    fn take_restart_exit_request(&mut self) -> bool {
        take_native_event_request_flag(&mut self.restart_exit_requested)
    }

    fn start_fallback_local_session(&mut self) -> Result<()> {
        apply_fallback_local_session_with_spawner(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            LocalPtyTransport::spawn_default,
            self.size,
        )
        .context("spawn fallback local pty for last active close")?;
        self.command_block_action_menu.close();
        self.selection_anchor = None;
        self.last_left_click = None;
        self.hovered_session_tab = None;
        self.session_tab_notice = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.hovered_hyperlink = None;
        self.hovered_command_block_id = None;
        self.exited = false;
        self.synchronized_output_deadline = None;
        self.cursor_blink = CursorBlinkState::default();
        self.text_blink = TextBlinkState::default();
        self.clear_empty_session_welcome();
        self.rebuild_frame();
        Ok(())
    }

    fn start_empty_session_local_shell(&mut self) {
        let config = local_new_tab_config(&self.local_tab_config, self.size);
        let result = replace_with_tracked_local_session_with_spawner(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            config,
            |config| {
                LocalPtyTransport::spawn(config)
                    .context("spawn local pty for empty-session welcome")
            },
            self.size,
        );

        match result {
            Ok(_) => {
                self.command_block_action_menu.close();
                self.ime_composition.clear_preedit();
                self.selection_anchor = None;
                self.last_left_click = None;
                self.hovered_session_tab = None;
                self.session_tab_notice = None;
                self.hovered_profile_action = None;
                self.hovered_update_notice = None;
                self.hovered_hyperlink = None;
                self.hovered_command_block_id = None;
                self.exited = false;
                self.synchronized_output_deadline = None;
                self.cursor_blink = CursorBlinkState::default();
                self.text_blink = TextBlinkState::default();
                self.clear_empty_session_welcome();
                self.rebuild_frame();
            }
            Err(err) => {
                eprintln!("failed to start local shell from empty-session welcome: {err:#}");
                self.empty_session_notice = Some(format!("New session failed: {err:#}"));
                self.rebuild_frame();
            }
        }
    }

    fn start_new_local_shell_command(&mut self) {
        self.session_switcher.close();
        if self.empty_session_welcome_visible() {
            self.start_empty_session_local_shell();
        } else {
            self.open_local_new_tab();
        }
    }

    fn open_local_new_tab(&mut self) {
        let config = local_new_tab_config(&self.local_tab_config, self.size);
        let result = open_local_new_tab_with_spawner(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            config,
            |config| LocalPtyTransport::spawn(config).context("spawn local pty for new tab"),
            self.size,
        );

        match result {
            Ok(_) => {
                self.command_block_action_menu.close();
                self.ime_composition.clear_preedit();
                self.selection_anchor = None;
                self.last_left_click = None;
                self.hovered_session_tab = None;
                self.session_tab_notice = None;
                self.hovered_profile_action = None;
                self.hovered_update_notice = None;
                self.hovered_hyperlink = None;
                self.hovered_command_block_id = None;
                self.exited = false;
                self.synchronized_output_deadline = None;
                self.cursor_blink = CursorBlinkState::default();
                self.text_blink = TextBlinkState::default();
                self.clear_empty_session_welcome();
                self.rebuild_frame();
            }
            Err(err) => {
                let message = format!("\r\n[new tab failed: {err:#}]\r\n");
                self.terminal.feed(message.as_bytes());
                self.rebuild_frame();
            }
        }
    }

    fn handle_active_transport_exit(&mut self, code: Option<i32>) -> NativeSessionCloseResult {
        self.exited = true;
        let message = match code {
            Some(code) => format!("\r\n[process exited: {code}]\r\n"),
            None => "\r\n[process exited]\r\n".to_owned(),
        };
        self.terminal.feed(message.as_bytes());

        let close_result = close_exited_active_native_session(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            self.active_session_close_fallback_policy,
        );
        let requests = native_session_close_event_requests(close_result);
        requests.apply_to(
            &mut self.window_close_requested,
            &mut self.fallback_local_session_requested,
        );

        if close_result == NativeSessionCloseResult::Closed {
            self.sync_all_session_runtime_sizes();
            self.command_block_action_menu.close();
            self.selection_anchor = None;
            self.last_left_click = None;
            self.hovered_session_tab = None;
            self.session_tab_notice = None;
            self.hovered_profile_action = None;
            self.hovered_update_notice = None;
            self.hovered_hyperlink = None;
            self.hovered_command_block_id = None;
            self.exited = false;
            self.synchronized_output_deadline = None;
            self.cursor_blink = CursorBlinkState::default();
            self.text_blink = TextBlinkState::default();
        } else if close_result == NativeSessionCloseResult::BlockedLastActive {
            self.enter_empty_session_welcome();
        } else if requests.any() {
            self.synchronized_output_deadline = None;
        }

        close_result
    }

    fn poll_transport(&mut self) -> bool {
        let mut changed = false;
        let mut force_render = false;
        for _ in 0..256 {
            match self.app.poll_transport() {
                Ok(Some(TransportEvent::Output(bytes))) => {
                    self.terminal.feed(&bytes);
                    self.apply_terminal_host_actions();
                    changed = true;
                }
                Ok(Some(TransportEvent::Exit { code })) => {
                    self.handle_active_transport_exit(code);
                    changed = true;
                    force_render = true;
                }
                Ok(Some(TransportEvent::Error(err))) => {
                    let message = format!("\r\n[transport error: {err}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                    force_render = true;
                }
                Ok(None) => break,
                Err(err) => {
                    let message = format!("\r\n[transport poll failed: {err:#}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                    force_render = true;
                    break;
                }
            }
        }

        if changed {
            self.refresh_search_after_terminal_change();
            if force_render || !self.terminal.synchronized_output_enabled() {
                self.synchronized_output_deadline = None;
                self.rebuild_frame();
                return true;
            }
            self.synchronized_output_deadline = synchronized_output_deadline_after_poll(
                self.synchronized_output_deadline,
                Instant::now(),
            );
        }
        false
    }

    fn apply_synchronized_output_timeout(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if !self.terminal.synchronized_output_enabled() {
            self.synchronized_output_deadline = None;
            return false;
        }

        let Some(deadline) = self.synchronized_output_deadline else {
            return false;
        };

        if Instant::now() >= deadline {
            self.synchronized_output_deadline = None;
            self.rebuild_frame();
            self.request_redraw();
            return true;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        false
    }

    fn apply_fullscreen_titlebar_hide_timeout(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if !self.window_fullscreen || self.titlebar_menu_open {
            self.fullscreen_titlebar_hide_deadline = None;
            return false;
        }

        let Some(deadline) = self.fullscreen_titlebar_hide_deadline else {
            return false;
        };

        if Instant::now() >= deadline {
            self.fullscreen_titlebar_hide_deadline = None;
            if self.fullscreen_titlebar_visible {
                self.fullscreen_titlebar_visible = false;
                self.hovered_titlebar_hit = None;
                self.rebuild_frame();
                self.request_redraw();
                return true;
            }
            return false;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        false
    }

    fn apply_about_status_timeout(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(deadline) = self.about_status_deadline else {
            return false;
        };

        if Instant::now() >= deadline {
            self.about_status_deadline = None;
            self.rebuild_frame();
            self.request_redraw();
            return true;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        false
    }

    fn apply_blink_timeouts(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let now = Instant::now();
        let cursor = self.terminal.snapshot().cursor;
        let cursor_changed = self
            .cursor_blink
            .toggle_if_due(cursor, self.text_input_target(), now);
        let text_changed = self.text_blink.toggle_if_due(now);

        if text_changed {
            self.app.set_blink_visible(self.text_blink.visible_phase());
        }
        if cursor_changed || text_changed {
            self.rebuild_frame();
            self.request_redraw();
            return true;
        }

        if let Some(deadline) = earliest_deadline(
            self.cursor_blink.next_deadline(),
            self.text_blink.next_deadline(),
        ) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
        false
    }

    fn apply_installed_update_check_timeout(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if self.update_notice.is_some() || self.update_monitor.marker_path.is_none() {
            return false;
        }

        let now = Instant::now();
        if self.update_monitor.due(now) {
            let changed = self.refresh_installed_update_notice();
            self.update_monitor.schedule_next(now);
            if changed {
                self.rebuild_frame();
                self.request_redraw();
                return true;
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.update_monitor.next_check));
        false
    }

    fn refresh_installed_update_notice(&mut self) -> bool {
        let previous = self.update_notice.clone();
        match self.update_monitor.check() {
            Ok(notice) => {
                self.update_notice = notice;
            }
            Err(err) => {
                eprintln!("failed to check installed Witty update marker: {err:#}");
            }
        }
        if self.update_notice.is_none() {
            self.hovered_update_notice = None;
        } else {
            let _ = self.refresh_update_notice_hover_for_current_pointer();
        }
        previous != self.update_notice
    }

    fn apply_terminal_host_actions(&mut self) {
        let actions = self.terminal.drain_host_actions();
        if actions.is_empty() {
            return;
        }

        let result = {
            let app = &mut self.app;
            let clipboard = self.clipboard.as_mut();
            let shell_integration = &mut self.shell_integration;
            let observed_at_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX));
            apply_terminal_host_actions_at_ms(
                actions,
                self.osc52_clipboard_policy,
                clipboard,
                shell_integration,
                Some(observed_at_ms as u64),
                |bytes| app.write_input(bytes),
            )
        };
        if let Err(err) = result {
            let message = format!("\r\n[terminal host action failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
        }
    }

    fn send_key(&mut self, event: &KeyEvent) {
        if self.exited || event.state != ElementState::Pressed {
            return;
        }

        let bytes =
            encode_key_event_input(event, self.modifiers.state(), self.terminal.input_modes());
        let Some(bytes) = bytes else {
            return;
        };

        if let Err(err) = self.app.write_input(&bytes) {
            let message = format!("\r\n[transport write failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
            self.rebuild_frame();
        }
    }

    fn text_input_target(&self) -> TextInputTarget {
        if self.session_switcher.is_open() {
            TextInputTarget::SessionSwitcher
        } else if self.command_palette.is_open() {
            TextInputTarget::CommandPalette
        } else if self.terminal_search.is_open() {
            TextInputTarget::Search
        } else {
            TextInputTarget::Terminal
        }
    }

    fn active_search_ime_composition(&self) -> Option<&ImeComposition> {
        (self.text_input_target() == TextInputTarget::Search).then_some(&self.ime_composition)
    }

    fn active_command_palette_ime_composition(&self) -> Option<&ImeComposition> {
        (self.text_input_target() == TextInputTarget::CommandPalette)
            .then_some(&self.ime_composition)
    }

    fn active_ime_cursor(&self, terminal_cursor: CellPoint) -> CellPoint {
        match self.text_input_target() {
            TextInputTarget::Terminal => self
                .compact_visual_cell_for_terminal_cell(terminal_ime_cursor_cell(
                    terminal_cursor,
                    &self.ime_composition,
                    self.size,
                ))
                .unwrap_or_else(|| {
                    terminal_ime_cursor_cell(terminal_cursor, &self.ime_composition, self.size)
                }),
            TextInputTarget::Search => {
                search_ime_cursor_cell(&self.terminal_search, &self.ime_composition, self.size)
            }
            TextInputTarget::CommandPalette => command_palette_ime_cursor_cell(
                &self.command_palette,
                &self.ime_composition,
                self.size,
            )
            .unwrap_or(terminal_cursor),
            TextInputTarget::SessionSwitcher => terminal_cursor,
        }
    }

    fn handle_ime_event(&mut self, event: Ime) -> bool {
        if self.empty_session_welcome_visible() && !self.command_palette.is_open() {
            self.ime_composition.clear_preedit();
            return false;
        }

        let target = self.text_input_target();
        let result = apply_native_ime_event(&mut self.ime_composition, event);

        if let Some(text) = result.committed_text {
            let route_result = match target {
                TextInputTarget::Terminal => self
                    .app
                    .write_input(text.as_bytes())
                    .context("failed to write IME commit text to terminal"),
                TextInputTarget::Search => {
                    self.commit_ime_text_to_search(&text);
                    Ok(())
                }
                TextInputTarget::CommandPalette => {
                    self.command_palette.input_text(&text);
                    Ok(())
                }
                TextInputTarget::SessionSwitcher => Ok(()),
            };
            if let Err(err) = route_result {
                let message = format!("\r\n[IME commit failed: {err:#}]\r\n");
                self.terminal.feed(message.as_bytes());
                self.rebuild_frame();
                return true;
            }
        }

        let changed = result.changed;
        if changed {
            self.rebuild_frame();
        } else {
            self.sync_ime_cursor_area(
                self.active_ime_cursor(self.terminal.snapshot().cursor.position),
            );
        }
        changed
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }

        if is_session_switcher_shortcut(&event.logical_key, self.modifiers) {
            let direction = if self.modifiers.state().shift_key() {
                -1
            } else {
                1
            };
            self.advance_session_switcher(direction);
            return;
        }

        if self.session_switcher.is_open() {
            self.handle_session_switcher_key(event);
            return;
        }

        if is_toggle_fullscreen_shortcut(&event.logical_key, self.modifiers) {
            self.toggle_fullscreen();
            return;
        }

        if is_search_shortcut(&event.logical_key, self.modifiers) {
            if self.empty_session_welcome_visible() {
                return;
            }
            self.open_search();
            return;
        }

        if is_command_palette_shortcut(&event.logical_key, self.modifiers) {
            self.open_command_palette();
            return;
        }

        if is_frame_diagnostics_shortcut(&event.logical_key) {
            self.toggle_frame_diagnostics();
            return;
        }

        if let Some(action) = runtime_font_size_shortcut_action(event, self.modifiers) {
            self.apply_runtime_font_size_action(action);
            return;
        }

        if is_new_local_tab_shortcut(&event.logical_key, self.modifiers) {
            self.start_new_local_shell_command();
            return;
        }

        if self.command_palette.is_open() {
            self.handle_command_palette_key(event);
            return;
        }

        if self.empty_session_welcome_visible() {
            self.handle_empty_session_welcome_key(event);
            return;
        }

        if self.command_block_action_menu.is_open() {
            self.handle_command_block_action_menu_key(event);
            return;
        }

        if self.terminal_search.is_open() {
            self.handle_search_key(event);
            return;
        }

        if is_copy_selection_shortcut(&event.logical_key, self.modifiers) {
            self.copy_selection_to_clipboard();
            return;
        }

        if is_paste_clipboard_shortcut(&event.logical_key, self.modifiers) {
            self.paste_clipboard_to_terminal();
            return;
        }

        if let Some(command_id) = command_shortcut_for_key(&event.logical_key, self.app.commands())
        {
            self.invoke_window_command(&command_id);
            return;
        }

        self.send_key(event);
    }

    fn open_command_palette(&mut self) {
        self.ime_composition.clear_preedit();
        self.session_switcher.close();
        self.terminal_search.close();
        self.command_block_action_menu.close();
        self.about_status_deadline = None;
        self.command_palette.open(self.app.commands());
        self.rebuild_frame();
    }

    fn open_search(&mut self) {
        self.ime_composition.clear_preedit();
        self.session_switcher.close();
        self.command_palette.close();
        self.command_block_action_menu.close();
        self.about_status_deadline = None;
        apply_search_command(
            &mut self.terminal,
            &mut self.terminal_search,
            SEARCH_OPEN_COMMAND_ID,
        );
        self.rebuild_frame();
    }

    fn show_about_status(&mut self) {
        self.ime_composition.clear_preedit();
        self.session_switcher.close();
        self.command_palette.close();
        self.command_block_action_menu.close();
        self.terminal_search.close();
        self.about_status_deadline = Instant::now().checked_add(ABOUT_STATUS_DURATION);
        self.rebuild_frame();
        self.request_redraw();
    }

    fn toggle_frame_diagnostics(&mut self) {
        self.show_diagnostics = !self.show_diagnostics;
        self.rebuild_frame();
    }

    fn toggle_fullscreen(&mut self) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };

        let entering_fullscreen =
            !self.window_fullscreen || !self.window_fullscreen_is_effective(&window);
        if entering_fullscreen {
            self.enter_fullscreen(&window);
        } else {
            self.exit_fullscreen(&window);
        }

        let size = window.inner_size();
        self.resize_grid(size);
        self.rebuild_frame();
    }

    fn enter_fullscreen(&mut self, window: &Window) {
        let target_monitor = window.current_monitor();
        let target_size = target_monitor.as_ref().map(|monitor| monitor.size());
        let target_scale_factor = fullscreen_target_scale_factor(target_monitor.as_ref(), window);

        if !self.window_fullscreen {
            self.fullscreen_windowed_restore_size = Some(window.inner_size());
            self.pending_windowed_restore_size = None;
            self.pending_windowed_restore_attempts = 0;
        }
        self.log_fullscreen_debug("enter-before", Some(window));
        apply_borderless_fullscreen(window, target_monitor.clone());
        self.window_fullscreen = true;
        self.fullscreen_target_monitor = target_monitor;
        self.fullscreen_target_size = target_size;
        self.fullscreen_target_scale_factor = Some(target_scale_factor);
        let _ = self.apply_window_scale_factor(target_scale_factor);
        self.schedule_fullscreen_reaffirm();
        self.reset_fullscreen_overlay_state();
        self.log_fullscreen_debug("enter-after", Some(window));
    }

    fn exit_fullscreen(&mut self, window: &Window) {
        self.log_fullscreen_debug("exit-before", Some(window));
        clear_fullscreen_size_guard(window);
        let restore_size = self.fullscreen_windowed_restore_size.take();
        window.set_fullscreen(None);
        self.window_fullscreen = false;
        self.fullscreen_target_monitor = None;
        self.fullscreen_target_size = None;
        self.fullscreen_target_scale_factor = None;
        self.fullscreen_reaffirm_until = None;
        self.fullscreen_next_reaffirm = None;
        self.pending_windowed_restore_size = restore_size;
        self.pending_windowed_restore_attempts = 0;
        self.reset_fullscreen_overlay_state();
        self.log_fullscreen_debug("exit-after", Some(window));
    }

    fn repair_fullscreen(&mut self, window: &Window) {
        let target_monitor = self
            .fullscreen_target_monitor
            .clone()
            .or_else(|| window.current_monitor());
        let target_size = self
            .fullscreen_target_size
            .or_else(|| target_monitor.as_ref().map(|monitor| monitor.size()));
        let target_scale_factor = self
            .fullscreen_target_scale_factor
            .unwrap_or_else(|| fullscreen_target_scale_factor(target_monitor.as_ref(), window));

        self.log_fullscreen_debug("repair-before", Some(window));
        force_borderless_fullscreen(window, target_monitor.clone());
        if let Some(size) = target_size {
            let _ = window.request_inner_size(size);
        }

        self.window_fullscreen = true;
        self.fullscreen_target_monitor = target_monitor;
        self.fullscreen_target_size = target_size;
        self.fullscreen_target_scale_factor = Some(target_scale_factor);
        let _ = self.apply_window_scale_factor(target_scale_factor);
        self.schedule_fullscreen_reaffirm();
        self.reset_fullscreen_overlay_state();
        self.log_fullscreen_debug("repair-after", Some(window));
    }

    fn reassert_fullscreen(&mut self, window: &Window) {
        let target_monitor = self
            .fullscreen_target_monitor
            .clone()
            .or_else(|| window.current_monitor());
        let target_size = self
            .fullscreen_target_size
            .or_else(|| target_monitor.as_ref().map(|monitor| monitor.size()));
        let target_scale_factor = self
            .fullscreen_target_scale_factor
            .unwrap_or_else(|| fullscreen_target_scale_factor(target_monitor.as_ref(), window));

        self.log_fullscreen_debug("reassert-before", Some(window));
        apply_borderless_fullscreen(window, target_monitor.clone());
        if let Some(size) = target_size {
            let _ = window.request_inner_size(size);
        }

        self.fullscreen_target_monitor = target_monitor;
        self.fullscreen_target_size = target_size;
        self.fullscreen_target_scale_factor = Some(target_scale_factor);
        let _ = self.apply_window_scale_factor(target_scale_factor);
        self.log_fullscreen_debug("reassert-after", Some(window));
    }

    fn schedule_fullscreen_reaffirm(&mut self) {
        let now = Instant::now();
        self.fullscreen_reaffirm_until = now.checked_add(FULLSCREEN_REAFFIRM_DURATION);
        self.fullscreen_next_reaffirm = Some(now);
    }

    fn apply_fullscreen_reaffirm_timeout(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if !self.window_fullscreen {
            self.fullscreen_reaffirm_until = None;
            self.fullscreen_next_reaffirm = None;
            return false;
        }

        let Some(until) = self.fullscreen_reaffirm_until else {
            self.fullscreen_next_reaffirm = None;
            return false;
        };

        let now = Instant::now();
        if now >= until {
            self.fullscreen_reaffirm_until = None;
            self.fullscreen_next_reaffirm = None;
            return false;
        }

        let Some(next) = self.fullscreen_next_reaffirm else {
            self.fullscreen_next_reaffirm = Some(now);
            return false;
        };

        if now < next {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            return false;
        }

        if let Some(window) = self.window.as_ref().cloned() {
            self.reassert_fullscreen(&window);
        }
        self.fullscreen_next_reaffirm = now.checked_add(FULLSCREEN_REAFFIRM_INTERVAL);
        if let Some(next) = self.fullscreen_next_reaffirm {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        }
        true
    }

    fn apply_pending_windowed_restore(&mut self, window: &Window) -> bool {
        let Some(size) = self.pending_windowed_restore_size else {
            return false;
        };

        if self.window_fullscreen || window.fullscreen().is_some() {
            return false;
        }

        if !fullscreen_inner_size_is_degraded(window.inner_size(), size)
            && !fullscreen_inner_size_is_degraded(size, window.inner_size())
        {
            self.pending_windowed_restore_size = None;
            self.pending_windowed_restore_attempts = 0;
            return false;
        }

        if self.pending_windowed_restore_attempts >= WINDOWED_RESTORE_MAX_ATTEMPTS {
            self.pending_windowed_restore_size = None;
            self.pending_windowed_restore_attempts = 0;
            return false;
        }

        self.pending_windowed_restore_attempts =
            self.pending_windowed_restore_attempts.saturating_add(1);
        let _ = window.request_inner_size(size);
        true
    }

    fn reset_fullscreen_overlay_state(&mut self) {
        self.fullscreen_titlebar_visible = false;
        self.fullscreen_titlebar_hide_deadline = None;
        self.titlebar_menu_open = false;
        self.hovered_titlebar_hit = None;
    }

    fn log_fullscreen_debug(&self, label: &str, window: Option<&Window>) {
        if !fullscreen_debug_enabled() {
            return;
        }

        let window = window.or(self.window.as_deref());
        let (
            winit_fullscreen,
            inner_size,
            window_scale_factor,
            current_monitor_name,
            current_monitor_size,
            current_monitor_scale_factor,
        ) = window
            .map(fullscreen_window_debug_state)
            .unwrap_or_default();

        tracing::debug!(
            target: "witty_app::fullscreen",
            label,
            app_fullscreen = self.window_fullscreen,
            winit_fullscreen,
            inner_size = ?inner_size,
            window_scale_factor = ?window_scale_factor,
            current_monitor = ?current_monitor_name,
            current_monitor_size = ?current_monitor_size,
            current_monitor_scale_factor = ?current_monitor_scale_factor,
            target_size = ?self.fullscreen_target_size,
            target_scale_factor = ?self.fullscreen_target_scale_factor,
            restore_size = ?self.fullscreen_windowed_restore_size,
            pending_restore = ?self.pending_windowed_restore_size,
            reaffirm_active = self.fullscreen_reaffirm_until.is_some(),
            "fullscreen state"
        );
    }

    fn window_fullscreen_is_effective(&self, window: &Window) -> bool {
        window_fullscreen_is_effective(window, self.fullscreen_target_size)
    }

    fn sync_window_fullscreen_state_from_window(&mut self) -> bool {
        let Some(window) = self.window.as_ref().cloned() else {
            let changed = self.window_fullscreen
                || self.fullscreen_target_monitor.is_some()
                || self.fullscreen_target_size.is_some()
                || self.fullscreen_target_scale_factor.is_some()
                || self.fullscreen_windowed_restore_size.is_some()
                || self.fullscreen_reaffirm_until.is_some()
                || self.fullscreen_next_reaffirm.is_some()
                || self.pending_windowed_restore_size.is_some()
                || self.pending_windowed_restore_attempts != 0;
            self.window_fullscreen = false;
            self.fullscreen_target_monitor = None;
            self.fullscreen_target_size = None;
            self.fullscreen_target_scale_factor = None;
            self.fullscreen_windowed_restore_size = None;
            self.fullscreen_reaffirm_until = None;
            self.fullscreen_next_reaffirm = None;
            self.pending_windowed_restore_size = None;
            self.pending_windowed_restore_attempts = 0;
            self.reset_fullscreen_overlay_state();
            return changed;
        };

        if self.window_fullscreen && !self.window_fullscreen_is_effective(&window) {
            self.repair_fullscreen(&window);
            return true;
        }

        let fullscreen = window.fullscreen().is_some();
        if !self.window_fullscreen && fullscreen {
            self.fullscreen_target_monitor = window.current_monitor();
            self.fullscreen_target_size = self
                .fullscreen_target_monitor
                .as_ref()
                .map(|monitor| monitor.size());
            self.fullscreen_target_scale_factor = Some(fullscreen_target_scale_factor(
                self.fullscreen_target_monitor.as_ref(),
                &window,
            ));
        } else if !fullscreen {
            self.fullscreen_target_monitor = None;
            self.fullscreen_target_size = None;
            self.fullscreen_target_scale_factor = None;
            self.fullscreen_reaffirm_until = None;
            self.fullscreen_next_reaffirm = None;
            let _ = self.apply_pending_windowed_restore(&window);
        }

        let state_changed = self.window_fullscreen != fullscreen;
        if !state_changed {
            return false;
        }

        self.window_fullscreen = fullscreen;
        self.reset_fullscreen_overlay_state();
        true
    }

    fn copy_selection_to_clipboard(&mut self) {
        match copy_selection_to_clipboard_and_clear(&mut self.terminal, self.clipboard.as_mut()) {
            Ok(_) => self.rebuild_frame(),
            Err(err) => {
                let message = format!("\r\n[clipboard copy failed: {err:#}]\r\n");
                self.terminal.feed(message.as_bytes());
                self.rebuild_frame();
            }
        }
    }

    fn copy_command_block_to_clipboard(&mut self, target: CommandBlockCopyTarget) {
        if let Err(err) = copy_command_block_to_clipboard(
            &self.terminal,
            &self.shell_integration,
            target,
            self.clipboard.as_mut(),
        ) {
            let message = format!("\r\n[clipboard copy failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
            self.rebuild_frame();
        }
    }

    fn open_command_block_action_menu(&mut self) -> bool {
        self.ime_composition.clear_preedit();
        self.terminal_search.close();
        self.command_palette.close();
        if !self
            .command_block_action_menu
            .open_for_selected_block(&self.shell_integration)
        {
            return false;
        }
        self.rebuild_frame();
        true
    }

    fn paste_clipboard_to_terminal(&mut self) {
        let clipboard = self.clipboard.as_mut();
        let app = &mut self.app;
        let result = paste_clipboard_to_input(
            clipboard,
            self.terminal.bracketed_paste_enabled(),
            |bytes| app.write_input(bytes),
        );

        if let Err(err) = result {
            let message = format!("\r\n[clipboard paste failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
            self.rebuild_frame();
        }
    }

    fn paste_primary_selection_to_terminal(&mut self) {
        let clipboard = self.clipboard.as_mut();
        let app = &mut self.app;
        let result = paste_selection_to_input(
            clipboard,
            ClipboardSelection::Primary,
            self.terminal.bracketed_paste_enabled(),
            |bytes| app.write_input(bytes),
        );

        if let Err(err) = result {
            let message = format!("\r\n[primary selection paste failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
            self.rebuild_frame();
        }
    }

    fn advance_session_switcher(&mut self, direction: i32) {
        let rows = self.profile_action_sessions.tab_rows();
        let changed = if self.session_switcher.is_open() {
            self.session_switcher.move_selection(&rows, direction)
        } else {
            self.ime_composition.clear_preedit();
            self.command_palette.close();
            self.command_block_action_menu.close();
            self.terminal_search.close();
            self.about_status_deadline = None;
            self.session_switcher.open(&rows, direction)
        };

        if changed {
            self.hovered_session_tab = None;
            self.rebuild_frame();
        }
    }

    fn handle_session_switcher_key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.session_switcher.close();
                self.rebuild_frame();
            }
            Key::Named(NamedKey::Enter) => {
                self.confirm_session_switcher();
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.move_session_switcher_selection(-1);
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.move_session_switcher_selection(1);
            }
            Key::Named(NamedKey::Tab) => {
                let direction = if self.modifiers.state().shift_key() {
                    -1
                } else {
                    1
                };
                self.move_session_switcher_selection(direction);
            }
            _ => {}
        }
    }

    fn move_session_switcher_selection(&mut self, direction: i32) {
        let rows = self.profile_action_sessions.tab_rows();
        if self.session_switcher.move_selection(&rows, direction) {
            self.rebuild_frame();
        }
    }

    fn confirm_session_switcher(&mut self) -> bool {
        let selected = self.session_switcher.selected_session_id;
        self.session_switcher.close();
        let switched = selected
            .map(|session_id| self.switch_profile_action_session_runtime(session_id))
            .unwrap_or(false);
        self.hovered_session_tab = None;
        self.rebuild_frame();
        switched || selected.is_some()
    }

    fn handle_modifiers_changed(&mut self, modifiers: Modifiers) -> bool {
        let was_control = self.modifiers.state().control_key();
        let has_control = modifiers.state().control_key();
        self.modifiers = modifiers;
        if was_control && !has_control && self.session_switcher.is_open() {
            return self.confirm_session_switcher();
        }
        false
    }

    fn handle_command_palette_key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.close_command_palette(),
            Key::Named(NamedKey::Enter) => {
                self.confirm_command_palette();
                return;
            }
            Key::Named(NamedKey::Backspace) => self.command_palette.backspace(),
            Key::Named(NamedKey::ArrowUp) => self.command_palette.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.command_palette.move_selection(1),
            Key::Named(NamedKey::PageUp) => self.command_palette.move_selection(-5),
            Key::Named(NamedKey::PageDown) => self.command_palette.move_selection(5),
            _ if text_input_allowed(self.modifiers) => {
                if let Some(text) = event.text.as_deref() {
                    self.command_palette.input_text(text);
                }
            }
            _ => {}
        }
        self.rebuild_frame();
    }

    fn handle_command_block_action_menu_key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.close_command_block_action_menu(),
            Key::Named(NamedKey::Enter) => {
                self.confirm_command_block_action_menu();
                return;
            }
            Key::Named(NamedKey::ArrowUp) => self.command_block_action_menu.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.command_block_action_menu.move_selection(1),
            _ => {}
        }
        self.rebuild_frame();
    }

    fn handle_search_key(&mut self, event: &KeyEvent) {
        match search_key_action(&event.logical_key, event.text.as_deref(), self.modifiers) {
            SearchKeyAction::Close => self.close_search(),
            SearchKeyAction::Next => {
                self.terminal_search.next_match();
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::Previous => {
                self.terminal_search.previous_match();
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::HistoryPrevious => {
                self.terminal_search
                    .previous_history_query(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::HistoryNext => {
                self.terminal_search
                    .next_history_query(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::Backspace => {
                self.terminal_search
                    .backspace(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::ToggleCaseSensitive => {
                self.terminal_search
                    .toggle_case_sensitive(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::ToggleRegex => {
                self.terminal_search
                    .toggle_regex(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::ToggleWholeWord => {
                self.terminal_search
                    .toggle_whole_word(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::ToggleNormalizeNfc => {
                self.terminal_search
                    .toggle_normalize_nfc(&self.terminal.search_text_rows());
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::InputText(text) => {
                self.terminal_search
                    .input_text(&self.terminal.search_text_rows(), &text);
                self.scroll_to_active_search_match();
            }
            SearchKeyAction::None => {}
        }
        self.rebuild_frame();
    }

    fn refresh_search_after_terminal_change(&mut self) {
        if !self.terminal_search.is_open() {
            return;
        }

        self.terminal_search
            .rebuild(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
    }

    fn scroll_to_active_search_match(&mut self) {
        scroll_terminal_to_active_search_match(&mut self.terminal, &self.terminal_search);
    }

    fn commit_ime_text_to_search(&mut self, text: &str) {
        self.terminal_search
            .input_text(&self.terminal.search_text_rows(), text);
        self.scroll_to_active_search_match();
    }

    fn close_search(&mut self) {
        self.ime_composition.clear_preedit();
        self.terminal_search.close();
    }

    fn close_command_palette(&mut self) {
        self.ime_composition.clear_preedit();
        self.command_palette.close();
    }

    fn close_command_block_action_menu(&mut self) {
        self.ime_composition.clear_preedit();
        self.command_block_action_menu.close();
    }

    fn confirm_command_palette(&mut self) {
        self.ime_composition.clear_preedit();
        let command_id = self.command_palette.confirm();
        self.rebuild_frame();
        if let Some(command_id) = command_id {
            self.invoke_window_command(&command_id);
        }
    }

    fn confirm_command_block_action_menu(&mut self) {
        self.ime_composition.clear_preedit();
        let command_id = self.command_block_action_menu.confirm();
        self.rebuild_frame();
        if let Some(command_id) = command_id {
            self.invoke_window_command(command_id);
        }
    }

    fn invoke_window_command(&mut self, command_id: &str) {
        if command_id == WITTY_NEW_LOCAL_SHELL_COMMAND_ID {
            self.start_new_local_shell_command();
            return;
        }
        if command_id == "witty.about" {
            self.show_about_status();
            return;
        }
        if apply_search_command(&mut self.terminal, &mut self.terminal_search, command_id) {
            self.close_command_palette();
            self.command_block_action_menu.close();
            self.about_status_deadline = None;
            self.rebuild_frame();
            return;
        }
        if command_id == COMMAND_BLOCK_ACTION_MENU_COMMAND_ID {
            self.open_command_block_action_menu();
            return;
        }
        if let Some(target) = command_block_copy_target(command_id) {
            self.copy_command_block_to_clipboard(target);
            return;
        }
        if apply_command_block_command(
            &mut self.shell_integration,
            self.terminal.active_screen(),
            command_id,
        ) {
            self.rebuild_frame();
            return;
        }

        let context = self
            .shell_integration
            .command_invocation_context_for_screen(self.terminal.active_screen());
        match self
            .app
            .invoke_command_with_context(command_id, serde_json::Value::Null, context)
        {
            Ok(actions) => {
                let profile_event = self.refresh_pending_profile_actions();
                let mut changed = self.feed_command_feedback(&actions);
                if plugin_actions_request_profile_action(&actions) {
                    if let Some(NativeProfileActionBridgeEvent::SnapshotRefreshed(snapshot)) =
                        profile_event
                    {
                        changed |= self.feed_pending_profile_action_feedback(&snapshot);
                    }
                }
                if changed {
                    self.rebuild_frame();
                }
            }
            Err(err) => {
                let message = format!("\r\n[command failed: {err:#}]\r\n");
                self.terminal.feed(message.as_bytes());
                self.rebuild_frame();
            }
        }
    }

    fn feed_command_feedback(&mut self, actions: &[PluginAction]) -> bool {
        let mut changed = false;
        for action in actions {
            match action {
                PluginAction::ShowMessage { message } => {
                    let message = format!("\r\n[plugin message: {message}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                }
                PluginAction::RegisterCommand(_)
                | PluginAction::WriteTerminal { .. }
                | PluginAction::RenderOverlay(_)
                | PluginAction::RequestProfilePicker(_)
                | PluginAction::RequestProfileLaunch(_) => {}
            }
        }
        changed
    }

    fn feed_pending_profile_action_feedback(
        &mut self,
        snapshot: &NativeProfileActionSnapshot,
    ) -> bool {
        let Some(message) = pending_profile_action_feedback(snapshot) else {
            return false;
        };
        self.terminal.feed(message.as_bytes());
        true
    }

    fn handle_empty_session_welcome_key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.start_empty_session_local_shell(),
            Key::Character(value)
                if value == " " && !self.modifiers.state().intersects(ModifiersState::all()) =>
            {
                self.start_empty_session_local_shell();
            }
            _ => {}
        }
    }

    fn scroll_viewport(&mut self, delta: MouseScrollDelta) -> bool {
        let lines = scroll_lines_for_delta(delta, self.metrics);
        if lines == 0 {
            return false;
        }

        self.terminal.scroll_viewport_lines(lines);
        self.rebuild_frame();
        true
    }

    fn begin_selection(&mut self) -> bool {
        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(point) = self.terminal_cell_point_for_position(position) else {
            return false;
        };
        let action =
            selection_for_left_press(&self.terminal, self.last_left_click, point, Instant::now());

        self.last_left_click = Some(action.click);
        self.selection_anchor = action.anchor;
        self.terminal.set_selection(Some(action.range));
        if action.completed {
            self.publish_selection_to_primary();
        }
        self.rebuild_frame();
        true
    }

    fn update_selection(&mut self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position = Some(position);
        let Some(anchor) = self.selection_anchor else {
            return false;
        };
        let Some(current) = self.terminal_cell_point_for_position(position) else {
            return false;
        };

        self.terminal
            .set_selection(Some(ordered_cell_range(anchor, current)));
        self.rebuild_frame();
        true
    }

    fn end_selection(&mut self) {
        if selection_release_should_publish(
            self.selection_anchor,
            self.terminal.snapshot().selection,
        ) {
            self.publish_selection_to_primary();
        }
        self.selection_anchor = None;
    }

    fn publish_selection_to_primary(&mut self) {
        if let Err(err) = copy_selection_to_primary(&self.terminal, self.clipboard.as_mut()) {
            eprintln!("failed to publish primary selection: {err:#}");
        }
    }

    fn mouse_reporting_active(&self) -> bool {
        self.terminal.input_modes().mouse.reports_mouse()
    }

    fn terminal_pixel_point_for_position(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<PixelPoint> {
        let mut visual_point = pixel_point_for_position(position)?;
        let content_y_offset = self.terminal_content_y_offset();
        if visual_point.y < content_y_offset {
            return None;
        }
        visual_point.y -= content_y_offset;
        command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            visual_point,
            self.metrics,
            self.size,
        )
    }

    fn terminal_cell_point_for_position(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<CellPoint> {
        self.terminal_pixel_point_for_position(position)
            .map(|point| cell_point_for_pixel_point(point, self.metrics, self.size))
    }

    fn terminal_pixel_mouse_position_for_position(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<PixelMousePosition> {
        self.terminal_pixel_point_for_position(position)
            .and_then(pixel_position_for_pixel_point)
    }

    fn compact_visual_cell_for_terminal_cell(&self, point: CellPoint) -> Option<CellPoint> {
        let compact_rows = self.shell_integration.folded_compact_visual_rows(
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        );
        let row = compact_rows.get(usize::from(point.row))?;
        let compact_row = row.compact_row?;
        Some(CellPoint::new(compact_row, point.col))
    }

    fn set_hovered_hyperlink_for_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let snapshot = self.terminal.snapshot();
        let hovered = self
            .terminal_pixel_point_for_position(position)
            .and_then(|point| hyperlink_for_pixel_point(&snapshot, point, self.metrics, self.size))
            .map(|link| link.id);
        if self.hovered_hyperlink == hovered {
            return false;
        }

        self.hovered_hyperlink = hovered;
        true
    }

    fn set_hovered_command_block_for_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let visible_row_anchors = self.terminal.visible_row_anchors();
        let hovered = self
            .terminal_pixel_point_for_position(position)
            .and_then(|point| {
                command_block_gutter_hit_for_pixel_point(
                    &self.shell_integration,
                    self.terminal.active_screen(),
                    &visible_row_anchors,
                    point,
                    self.metrics,
                    self.size,
                )
            })
            .map(|hit| hit.id);
        if self.hovered_command_block_id == hovered {
            return false;
        }

        self.hovered_command_block_id = hovered;
        true
    }

    fn set_hovered_profile_action_for_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let hovered = profile_action_overlay_hit_for_position(
            self.profile_actions.snapshot(),
            position,
            self.metrics,
            self.size,
        );
        if self.hovered_profile_action == hovered {
            return false;
        }

        self.hovered_profile_action = hovered;
        true
    }

    fn set_hovered_update_notice_for_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let hovered = native_update_notice_hit_for_position(
            self.update_notice.as_ref(),
            position,
            self.metrics,
            self.size,
        );
        if self.hovered_update_notice == hovered {
            return false;
        }

        self.hovered_update_notice = hovered;
        true
    }

    fn set_hovered_empty_session_welcome_for_position(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> bool {
        let hovered = if self.empty_session_welcome_visible() {
            empty_session_welcome_hit_for_position(position, self.metrics, self.size)
        } else {
            None
        };
        if self.hovered_empty_session_welcome == hovered {
            return false;
        }

        self.hovered_empty_session_welcome = hovered;
        true
    }

    fn set_hovered_session_tab_for_position(&mut self, position: PhysicalPosition<f64>) -> bool {
        let rows = self.profile_action_sessions.tab_rows();
        let hovered = if self.session_tab_display_policy.should_show(rows.len()) {
            native_session_tab_strip_hit_for_position(
                &rows,
                self.session_tab_notice,
                self.session_tab_position,
                self.session_tab_label_style,
                position,
                self.metrics,
                self.size,
            )
        } else {
            None
        };
        if self.hovered_session_tab == hovered {
            return false;
        }

        self.hovered_session_tab = hovered;
        true
    }

    fn switch_profile_action_session_runtime(
        &mut self,
        target_session_id: NativeSessionId,
    ) -> bool {
        let Some(current_session_id) = self
            .profile_action_sessions
            .active()
            .map(|record| record.id)
        else {
            return false;
        };
        if current_session_id == target_session_id {
            return false;
        }

        if !switch_native_session_runtime(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_session_runtimes,
            current_session_id,
            target_session_id,
        ) {
            return false;
        }

        let switched = self.profile_action_sessions.set_active(target_session_id);
        if switched {
            self.sync_all_session_runtime_sizes();
        }
        switched
    }

    fn close_profile_action_session(
        &mut self,
        session_id: NativeSessionId,
    ) -> NativeSessionCloseResult {
        let Some(active_session_id) = self
            .profile_action_sessions
            .active()
            .map(|record| record.id)
        else {
            return NativeSessionCloseResult::Ignored;
        };

        if active_session_id == session_id {
            if close_active_native_session_by_switching_to_parked(
                &mut self.app,
                &mut self.terminal,
                &mut self.terminal_search,
                &mut self.shell_integration,
                &mut self.profile_action_sessions,
                &mut self.profile_action_session_runtimes,
            ) {
                self.sync_all_session_runtime_sizes();
                return NativeSessionCloseResult::Closed;
            }
            if !has_inactive_parked_native_session_runtime(
                &self.profile_action_sessions,
                &self.profile_action_session_runtimes,
            ) {
                return native_session_close_result_for_last_active_policy(
                    self.active_session_close_fallback_policy,
                );
            }
            return NativeSessionCloseResult::Ignored;
        }

        if close_parked_native_session_runtime(
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            session_id,
        ) {
            NativeSessionCloseResult::Closed
        } else {
            NativeSessionCloseResult::Ignored
        }
    }

    fn handle_session_tab_strip_click(&mut self, state: ElementState, button: MouseButton) -> bool {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let rows = self.profile_action_sessions.tab_rows();
        if !self.session_tab_display_policy.should_show(rows.len()) {
            return false;
        }
        let Some(hit) = native_session_tab_strip_hit_for_position(
            &rows,
            self.session_tab_notice,
            self.session_tab_position,
            self.session_tab_label_style,
            position,
            self.metrics,
            self.size,
        ) else {
            return false;
        };

        let switched = match hit.target {
            NativeSessionTabStripTarget::Select => {
                self.switch_profile_action_session_runtime(hit.session_id)
            }
            NativeSessionTabStripTarget::Close => false,
        };
        let close_result = match hit.target {
            NativeSessionTabStripTarget::Select => NativeSessionCloseResult::Ignored,
            NativeSessionTabStripTarget::Close => self.close_profile_action_session(hit.session_id),
        };
        let closed = close_result == NativeSessionCloseResult::Closed;
        let blocked = close_result == NativeSessionCloseResult::BlockedLastActive;
        let requests = native_session_close_event_requests(close_result);
        let hover_changed = self.hovered_session_tab != Some(hit);
        requests.apply_to(
            &mut self.window_close_requested,
            &mut self.fallback_local_session_requested,
        );
        if blocked {
            self.enter_empty_session_welcome();
        }
        if switched || closed || requests.fallback_local_session {
            self.command_block_action_menu.close();
            self.hovered_update_notice = None;
            self.hovered_empty_session_welcome = None;
            self.hovered_profile_action = None;
            self.synchronized_output_deadline = None;
            self.cursor_blink = CursorBlinkState::default();
            self.text_blink = TextBlinkState::default();
        }
        if switched || closed || blocked || requests.fallback_local_session {
            self.session_tab_notice = None;
        } else {
            self.session_tab_notice =
                native_session_tab_notice_after_close_result(self.session_tab_notice, close_result);
        }
        let hover_after_state_change_changed = if closed || requests.fallback_local_session {
            let changed = self.hovered_session_tab.is_some();
            self.hovered_session_tab = None;
            changed
        } else {
            self.refresh_session_tab_hover_for_current_pointer()
        };
        self.hovered_hyperlink = None;
        self.hovered_command_block_id = None;
        self.selection_anchor = None;
        self.last_left_click = None;
        if switched
            || closed
            || blocked
            || requests.any()
            || hover_changed
            || hover_after_state_change_changed
        {
            self.rebuild_frame();
        }
        true
    }

    fn handle_hyperlink_activation_click(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        if !is_hyperlink_activation_click(state, button, self.modifiers.state()) {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let snapshot = self.terminal.snapshot();
        let Some(point) = self.terminal_pixel_point_for_position(position) else {
            return false;
        };
        let Some(link) = hyperlink_for_pixel_point(&snapshot, point, self.metrics, self.size)
        else {
            return false;
        };
        let uri = link.uri.clone();

        if let Err(err) = open_external_url(&uri) {
            eprintln!("failed to open hyperlink {uri:?}: {err:#}");
        }

        true
    }

    fn handle_command_block_gutter_click(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        if state != ElementState::Pressed
            || !matches!(button, MouseButton::Left | MouseButton::Right)
        {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let visible_row_anchors = self.terminal.visible_row_anchors();
        let Some(point) = self.terminal_pixel_point_for_position(position) else {
            return false;
        };
        let Some(id) = select_command_block_gutter_hit_for_pixel_point(
            &mut self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            point,
            self.metrics,
            self.size,
        ) else {
            return false;
        };

        self.hovered_command_block_id = Some(id);
        if button == MouseButton::Right {
            self.command_block_action_menu.open_for_block(id);
        } else {
            self.command_block_action_menu.close();
        }
        self.rebuild_frame();
        true
    }

    fn handle_profile_action_overlay_click(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(hit) = profile_action_overlay_hit_for_position(
            self.profile_actions.snapshot(),
            position,
            self.metrics,
            self.size,
        ) else {
            return false;
        };

        let is_start_success_hit =
            profile_action_overlay_start_success_for_hit(self.profile_actions.snapshot(), hit)
                .is_some();
        let is_start_failure_hit =
            profile_action_overlay_start_failure_for_hit(self.profile_actions.snapshot(), hit)
                .is_some();
        match hit.target {
            NativeProfileActionOverlayTarget::Dismiss => {
                if is_start_success_hit {
                    self.dismiss_profile_action_start_success_from_overlay();
                } else if is_start_failure_hit {
                    self.dismiss_profile_action_start_failure_from_overlay();
                } else {
                    self.dismiss_pending_profile_action_from_overlay(hit.key);
                }
            }
            NativeProfileActionOverlayTarget::Confirm
            | NativeProfileActionOverlayTarget::ConfirmNewTab => {
                if is_start_success_hit {
                    // Success rows are status-only and do not expose an action.
                } else if is_start_failure_hit {
                    self.retry_profile_action_start_from_overlay();
                } else if let Some(confirmation) = profile_action_overlay_confirmation_for_hit(
                    self.profile_actions.snapshot(),
                    hit,
                ) {
                    if let Some(start_mode) =
                        native_profile_action_start_mode_for_overlay_target(hit.target)
                    {
                        self.confirm_pending_profile_action_from_overlay(confirmation, start_mode);
                    }
                }
            }
            NativeProfileActionOverlayTarget::Row => {}
        }
        true
    }

    fn handle_update_notice_click(&mut self, state: ElementState, button: MouseButton) -> bool {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(hit) = native_update_notice_hit_for_position(
            self.update_notice.as_ref(),
            position,
            self.metrics,
            self.size,
        ) else {
            return false;
        };

        if hit.target == NativeUpdateNoticeTarget::Restart {
            self.restart_to_installed_update();
        }
        true
    }

    fn handle_empty_session_welcome_click(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> bool {
        if !self.empty_session_welcome_visible()
            || state != ElementState::Pressed
            || button != MouseButton::Left
        {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(hit) = empty_session_welcome_hit_for_position(position, self.metrics, self.size)
        else {
            return false;
        };

        match hit.target {
            NativeEmptySessionWelcomeTarget::NewLocalShell => {
                self.start_empty_session_local_shell();
            }
            NativeEmptySessionWelcomeTarget::CommandPalette => {
                self.open_command_palette();
            }
        }
        true
    }

    fn restart_to_installed_update(&mut self) {
        let Some(notice) = self.update_notice.clone() else {
            return;
        };
        match self.prepare_restart_to_update_plan(&notice) {
            Ok(plan) => match spawn_restart_plan(&plan) {
                Ok(()) => {
                    self.restart_exit_requested = true;
                }
                Err(err) => {
                    self.feed_restart_failure(format!("failed to spawn restart: {err:#}"));
                }
            },
            Err(err) => {
                self.feed_restart_failure(format!("failed to prepare restart: {err:#}"));
            }
        }
    }

    fn prepare_restart_to_update_plan(
        &mut self,
        notice: &NativeUpdateNotice,
    ) -> Result<RestartExecutionPlan> {
        let snapshot_path = default_restart_state_path()?;
        let snapshot = self.restart_snapshot_v1(Some(notice.installed_build_id()));
        write_restart_snapshot_atomic(&snapshot_path, &snapshot)?;
        Ok(plan_restart_execution(
            &notice.installed_marker,
            snapshot_path,
        ))
    }

    fn restart_snapshot_v1(&self, installed_build_id: Option<&str>) -> RestartSnapshotV1 {
        let inner_size = self.window.as_ref().map(|window| window.inner_size());
        restart_snapshot_v1_for_native_state(
            &self.profile_action_sessions,
            &self.local_tab_config,
            self.size,
            inner_size,
            &self.update_monitor.running.build_id,
            installed_build_id,
        )
    }

    fn feed_restart_failure(&mut self, message: String) {
        let message = format!("\r\n[{message}]\r\n");
        self.terminal.feed(message.as_bytes());
        self.rebuild_frame();
    }

    fn confirm_pending_profile_action_from_overlay(
        &mut self,
        confirmation: PendingProfileActionConfirmation,
        start_mode: NativeProfileActionStartMode,
    ) {
        let store = match default_profile_action_store_snapshot() {
            Ok(store) => store,
            Err(err) => {
                eprintln!("failed to load profile store snapshot for confirmation: {err:#}");
                return;
            }
        };
        match self
            .profile_actions
            .confirm(&mut self.app, &store, confirmation, self.size)
        {
            Ok(NativeProfileActionBridgeEvent::Confirmed { resolved, .. }) => {
                self.resolved_profile_action_handoffs
                    .push(native_resolved_profile_action_handoff(resolved));
                if self.apply_next_resolved_profile_action_handoff_policy(
                    NativeResolvedProfileActionSessionPolicy::DeferStart,
                ) {
                    if self.plan_next_deferred_profile_action_start(start_mode) {
                        match self.apply_next_profile_action_start_plan() {
                            Ok(Some(_)) | Ok(None) => {}
                            Err(err) => {
                                eprintln!("failed to start confirmed profile action: {err:#}");
                                self.record_profile_action_start_failure_from_next_plan();
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("failed to confirm pending profile action: {err:#}");
                let _ = self.refresh_pending_profile_actions();
            }
        }
        self.hovered_session_tab = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.rebuild_frame();
    }

    fn retry_profile_action_start_from_overlay(&mut self) {
        match self.apply_next_profile_action_start_plan() {
            Ok(Some(_)) => {}
            Ok(None) => {
                self.profile_actions.set_start_failure(None);
            }
            Err(err) => {
                eprintln!("failed to retry confirmed profile action start: {err:#}");
                self.record_profile_action_start_failure_from_next_plan();
            }
        }
        self.hovered_session_tab = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.rebuild_frame();
    }

    fn dismiss_profile_action_start_failure_from_overlay(&mut self) {
        if self.profile_action_start_plans.take_next().is_none() {
            eprintln!("failed to dismiss profile action start failure: no queued start plan");
        }
        self.profile_actions.set_start_failure(None);
        self.hovered_session_tab = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.rebuild_frame();
    }

    fn dismiss_profile_action_start_success_from_overlay(&mut self) {
        self.profile_actions.set_start_success(None);
        self.hovered_session_tab = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.rebuild_frame();
    }

    #[allow(dead_code)]
    fn take_next_resolved_profile_action_handoff(
        &mut self,
    ) -> Option<NativeResolvedProfileActionHandoff> {
        self.resolved_profile_action_handoffs.take_next()
    }

    fn apply_next_resolved_profile_action_handoff_policy(
        &mut self,
        policy: NativeResolvedProfileActionSessionPolicy,
    ) -> bool {
        apply_native_resolved_profile_action_session_policy(
            &mut self.resolved_profile_action_handoffs,
            &mut self.deferred_profile_action_starts,
            policy,
        )
    }

    #[allow(dead_code)]
    fn take_next_deferred_profile_action_start(
        &mut self,
    ) -> Option<NativeResolvedProfileActionHandoff> {
        self.deferred_profile_action_starts.take_next()
    }

    fn plan_next_deferred_profile_action_start(
        &mut self,
        mode: NativeProfileActionStartMode,
    ) -> bool {
        plan_next_native_profile_action_start(
            &mut self.deferred_profile_action_starts,
            &mut self.profile_action_start_plans,
            mode,
        )
    }

    #[allow(dead_code)]
    fn take_next_profile_action_start_plan(&mut self) -> Option<NativeProfileActionStartPlan> {
        self.profile_action_start_plans.take_next()
    }

    #[allow(dead_code)]
    fn current_profile_action_session(&self) -> Option<&NativeProfileActionCurrentSession> {
        self.profile_action_sessions
            .active()
            .map(|record| &record.profile_action)
    }

    #[allow(dead_code)]
    fn apply_profile_action_start_plan_with_transport(
        &mut self,
        plan: NativeProfileActionStartPlan,
        transport: LocalPtyTransport,
    ) -> NativeProfileActionStartExecution {
        let execution = apply_native_profile_action_start_plan_with_transport(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            plan,
            transport,
            self.size,
        );
        self.command_block_action_menu.close();
        self.selection_anchor = None;
        self.last_left_click = None;
        self.hovered_session_tab = None;
        self.session_tab_notice = None;
        self.hovered_profile_action = None;
        self.hovered_hyperlink = None;
        self.hovered_command_block_id = None;
        self.exited = false;
        self.synchronized_output_deadline = None;
        self.cursor_blink = CursorBlinkState::default();
        self.text_blink = TextBlinkState::default();
        self.clear_empty_session_welcome();
        self.profile_actions
            .set_start_success(Some(native_profile_action_start_success_row(
                &execution.plan,
            )));
        self.rebuild_frame();
        execution
    }

    fn apply_next_profile_action_start_plan(
        &mut self,
    ) -> Result<Option<NativeProfileActionStartExecution>> {
        let execution = apply_next_native_profile_action_start_plan_with_spawner(
            &mut self.app,
            &mut self.terminal,
            &mut self.terminal_search,
            &mut self.shell_integration,
            &mut self.profile_action_sessions,
            &mut self.profile_action_session_runtimes,
            &mut self.profile_action_start_plans,
            |config| {
                LocalPtyTransport::spawn(config)
                    .context("spawn local pty for confirmed profile action")
            },
            self.size,
        )?;
        if let Some(execution) = execution.as_ref() {
            self.profile_actions
                .set_start_success(Some(native_profile_action_start_success_row(
                    &execution.plan,
                )));
            self.command_block_action_menu.close();
            self.selection_anchor = None;
            self.last_left_click = None;
            self.hovered_session_tab = None;
            self.session_tab_notice = None;
            self.hovered_profile_action = None;
            self.hovered_hyperlink = None;
            self.hovered_command_block_id = None;
            self.exited = false;
            self.synchronized_output_deadline = None;
            self.cursor_blink = CursorBlinkState::default();
            self.text_blink = TextBlinkState::default();
            self.clear_empty_session_welcome();
            self.rebuild_frame();
        }
        Ok(execution)
    }

    fn record_profile_action_start_failure_from_next_plan(&mut self) {
        let failure = self
            .profile_action_start_plans
            .peek_next()
            .map(native_profile_action_start_failure_row);
        self.profile_actions.set_start_failure(failure);
    }

    fn dismiss_pending_profile_action_from_overlay(&mut self, key: PendingProfileActionKey) {
        let store = match default_profile_action_store_snapshot() {
            Ok(store) => store,
            Err(err) => {
                eprintln!("failed to load profile store snapshot for dismissal: {err:#}");
                return;
            }
        };
        if let Err(err) = self.profile_actions.dismiss(&mut self.app, &store, key) {
            eprintln!("failed to dismiss pending profile action: {err:#}");
            let _ = self.refresh_pending_profile_actions();
        }
        self.hovered_session_tab = None;
        self.hovered_profile_action = None;
        self.hovered_update_notice = None;
        self.rebuild_frame();
    }

    fn handle_titlebar_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };

        if state != ElementState::Pressed {
            return self.titlebar_consumes_position(position);
        }

        match self.titlebar_hit_test(position) {
            Some(NativeTitleBarHit::Button(NativeTitleBarButton::Menu)) => {
                self.titlebar_menu_open = !self.titlebar_menu_open;
                let _ = self.refresh_titlebar_hover_for_current_pointer();
                self.rebuild_frame();
                true
            }
            Some(NativeTitleBarHit::Button(NativeTitleBarButton::NewSession)) => {
                self.titlebar_menu_open = false;
                self.hovered_titlebar_hit = None;
                self.start_new_local_shell_command();
                true
            }
            Some(NativeTitleBarHit::Button(NativeTitleBarButton::Minimize)) => {
                self.titlebar_menu_open = false;
                if let Some(window) = &self.window {
                    window.set_minimized(true);
                }
                let _ = self.refresh_titlebar_hover_for_current_pointer();
                self.rebuild_frame();
                true
            }
            Some(NativeTitleBarHit::Button(NativeTitleBarButton::Maximize)) => {
                self.titlebar_menu_open = false;
                if let Some(window) = &self.window {
                    window.set_maximized(!window.is_maximized());
                }
                let _ = self.refresh_titlebar_hover_for_current_pointer();
                self.rebuild_frame();
                true
            }
            Some(NativeTitleBarHit::Button(NativeTitleBarButton::Close)) => {
                self.window_close_requested = true;
                true
            }
            Some(NativeTitleBarHit::MenuItem(item)) => {
                self.titlebar_menu_open = false;
                self.hovered_titlebar_hit = None;
                self.activate_titlebar_menu_item(item);
                true
            }
            Some(NativeTitleBarHit::DragRegion) => {
                self.titlebar_menu_open = false;
                if !self.window_fullscreen {
                    if let Some(window) = &self.window {
                        if let Err(err) = window.drag_window() {
                            eprintln!("failed to start window drag: {err:#}");
                        }
                    }
                }
                let _ = self.refresh_titlebar_hover_for_current_pointer();
                self.rebuild_frame();
                true
            }
            None if self.titlebar_menu_open => {
                self.titlebar_menu_open = false;
                self.hovered_titlebar_hit = None;
                self.rebuild_frame();
                true
            }
            None => false,
        }
    }

    fn handle_resize_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(direction) = self.resize_direction_for_position(position) else {
            return false;
        };

        if state == ElementState::Pressed {
            if let Some(window) = &self.window {
                if let Err(err) = window.drag_resize_window(direction) {
                    eprintln!("failed to start window resize: {err:#}");
                }
            }
        }
        true
    }

    fn resize_direction_for_position(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<ResizeDirection> {
        if self.window_fullscreen {
            return None;
        }
        if matches!(
            self.titlebar_hit_test(position),
            Some(NativeTitleBarHit::Button(_) | NativeTitleBarHit::MenuItem(_))
        ) {
            return None;
        }
        let window = self.window.as_ref()?;
        if window.is_maximized() {
            return None;
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return None;
        }

        let border = (CUSTOM_RESIZE_BORDER_WIDTH * window.scale_factor()).max(1.0);
        let x = position.x;
        let y = position.y;
        let left = x >= 0.0 && x <= border;
        let right = x >= (f64::from(size.width) - border) && x < f64::from(size.width);
        let top = y >= 0.0 && y <= border;
        let bottom = y >= (f64::from(size.height) - border) && y < f64::from(size.height);

        match (top, bottom, left, right) {
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (true, _, _, true) => Some(ResizeDirection::NorthEast),
            (_, true, true, _) => Some(ResizeDirection::SouthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (true, _, _, _) => Some(ResizeDirection::North),
            (_, true, _, _) => Some(ResizeDirection::South),
            (_, _, true, _) => Some(ResizeDirection::West),
            (_, _, _, true) => Some(ResizeDirection::East),
            _ => None,
        }
    }

    fn set_native_cursor_icon(&mut self, cursor: CursorIcon) {
        if self.native_cursor_icon == Some(cursor) {
            return;
        }
        let Some(window) = &self.window else {
            self.native_cursor_icon = None;
            return;
        };
        window.set_cursor(cursor);
        self.native_cursor_icon = Some(cursor);
    }

    fn sync_resize_cursor_for_position(&mut self, position: PhysicalPosition<f64>) {
        let cursor = self
            .resize_direction_for_position(position)
            .map(CursorIcon::from)
            .unwrap_or(CursorIcon::Default);
        self.set_native_cursor_icon(cursor);
    }

    fn activate_titlebar_menu_item(&mut self, item: NativeTitleBarMenuItem) {
        match item {
            NativeTitleBarMenuItem::NewSession => self.start_new_local_shell_command(),
            NativeTitleBarMenuItem::CommandPalette => self.open_command_palette(),
            NativeTitleBarMenuItem::Search => {
                if !self.empty_session_welcome_visible() {
                    self.open_search();
                }
            }
            NativeTitleBarMenuItem::About => self.invoke_window_command("witty.about"),
        }
    }

    fn handle_mouse_input(&mut self, state: ElementState, button: MouseButton) -> bool {
        if self.handle_resize_mouse_input(state, button) {
            return true;
        }
        if self.handle_titlebar_mouse_input(state, button) {
            return true;
        }
        if self.handle_update_notice_click(state, button) {
            return true;
        }
        if self.handle_profile_action_overlay_click(state, button) {
            return true;
        }
        if self.handle_empty_session_welcome_click(state, button) {
            return true;
        }
        if self.empty_session_welcome_visible() {
            return false;
        }
        if self.handle_session_tab_strip_click(state, button) {
            return true;
        }
        if self.handle_hyperlink_activation_click(state, button) {
            return true;
        }
        if self.handle_command_block_gutter_click(state, button) {
            return true;
        }

        match mouse_local_override_action(
            self.mouse_reporting_active(),
            self.mouse_override_policy,
            self.modifiers.state(),
            MouseLocalOverrideEvent::Button { state, button },
            self.selection_anchor,
        ) {
            MouseLocalOverrideAction::Selection => {
                return match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => self.begin_selection(),
                    (ElementState::Released, MouseButton::Left) => {
                        self.end_selection();
                        false
                    }
                    _ => false,
                };
            }
            MouseLocalOverrideAction::PrimaryPaste => {
                self.paste_primary_selection_to_terminal();
                return true;
            }
            MouseLocalOverrideAction::None | MouseLocalOverrideAction::Scrollback => {}
        }

        if self.mouse_reporting_active() {
            return self.handle_mouse_report_button(state, button);
        }

        match (state, button) {
            (ElementState::Pressed, MouseButton::Left) => self.begin_selection(),
            (ElementState::Released, MouseButton::Left) => {
                self.end_selection();
                false
            }
            (ElementState::Pressed, MouseButton::Middle) => {
                self.paste_primary_selection_to_terminal();
                true
            }
            _ => false,
        }
    }

    fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position = Some(position);
        self.sync_resize_cursor_for_position(position);
        let titlebar_changed = self.update_titlebar_for_cursor_position(position);
        if self.titlebar_consumes_position(position) {
            let content_hover_changed = self.clear_content_hover_state();
            if titlebar_changed || content_hover_changed {
                self.rebuild_frame();
            }
            return titlebar_changed || content_hover_changed;
        }

        let update_hover_changed = self.set_hovered_update_notice_for_position(position);
        if self.hovered_update_notice.is_some() {
            let profile_hover_changed = self.hovered_profile_action.take().is_some();
            let empty_hover_changed = self.hovered_empty_session_welcome.take().is_some();
            let session_hover_changed = self.hovered_session_tab.take().is_some();
            let hyperlink_hover_changed = self.hovered_hyperlink.take().is_some();
            let command_block_hover_changed = self.hovered_command_block_id.take().is_some();
            let hover_changed = titlebar_changed
                || update_hover_changed
                || profile_hover_changed
                || empty_hover_changed
                || session_hover_changed
                || hyperlink_hover_changed
                || command_block_hover_changed;
            if hover_changed {
                self.rebuild_frame();
            }
            return hover_changed;
        }

        let profile_hover_changed = self.set_hovered_profile_action_for_position(position);
        if self.hovered_profile_action.is_some() {
            let empty_hover_changed = self.hovered_empty_session_welcome.take().is_some();
            let session_hover_changed = self.hovered_session_tab.take().is_some();
            let hyperlink_hover_changed = self.hovered_hyperlink.take().is_some();
            let command_block_hover_changed = self.hovered_command_block_id.take().is_some();
            let hover_changed = titlebar_changed
                || profile_hover_changed
                || empty_hover_changed
                || session_hover_changed
                || hyperlink_hover_changed
                || command_block_hover_changed;
            if hover_changed {
                self.rebuild_frame();
            }
            return hover_changed;
        }

        let empty_hover_changed = self.set_hovered_empty_session_welcome_for_position(position);
        if self.hovered_empty_session_welcome.is_some() {
            let session_hover_changed = self.hovered_session_tab.take().is_some();
            let hyperlink_hover_changed = self.hovered_hyperlink.take().is_some();
            let command_block_hover_changed = self.hovered_command_block_id.take().is_some();
            let hover_changed = titlebar_changed
                || empty_hover_changed
                || session_hover_changed
                || hyperlink_hover_changed
                || command_block_hover_changed;
            if hover_changed {
                self.rebuild_frame();
            }
            return hover_changed;
        }

        let session_hover_changed = self.set_hovered_session_tab_for_position(position);
        if self.hovered_session_tab.is_some() {
            let hyperlink_hover_changed = self.hovered_hyperlink.take().is_some();
            let command_block_hover_changed = self.hovered_command_block_id.take().is_some();
            let hover_changed = titlebar_changed
                || session_hover_changed
                || hyperlink_hover_changed
                || command_block_hover_changed;
            if hover_changed {
                self.rebuild_frame();
            }
            return hover_changed;
        }

        let hyperlink_hover_changed = self.set_hovered_hyperlink_for_position(position);
        let command_block_hover_changed = self.set_hovered_command_block_for_position(position);
        let hover_changed = titlebar_changed
            || update_hover_changed
            || profile_hover_changed
            || empty_hover_changed
            || session_hover_changed
            || hyperlink_hover_changed
            || command_block_hover_changed;

        if self.empty_session_welcome_visible() {
            if hover_changed {
                self.rebuild_frame();
            }
            return hover_changed;
        }

        if mouse_local_override_action(
            self.mouse_reporting_active(),
            self.mouse_override_policy,
            self.modifiers.state(),
            MouseLocalOverrideEvent::Motion,
            self.selection_anchor,
        ) == MouseLocalOverrideAction::Selection
        {
            return self.update_selection(position) || hover_changed;
        }

        if self.mouse_reporting_active() {
            let reported = self.handle_mouse_report_motion(position);
            if hover_changed && !reported {
                self.rebuild_frame();
            }
            return reported || hover_changed;
        }

        let selected = self.update_selection(position);
        if hover_changed && !selected {
            self.rebuild_frame();
        }
        selected || hover_changed
    }

    fn handle_cursor_left(&mut self) -> bool {
        self.pointer_position = None;
        self.set_native_cursor_icon(CursorIcon::Default);
        let content_hover_changed = self.clear_content_hover_state();
        let titlebar_hover_changed = self.hovered_titlebar_hit.is_some();
        self.hovered_titlebar_hit = None;
        let mut titlebar_visibility_changed = false;
        if self.window_fullscreen
            && self.fullscreen_titlebar_visible
            && !self.titlebar_menu_open
            && self.fullscreen_titlebar_hide_deadline.is_none()
        {
            self.fullscreen_titlebar_hide_deadline =
                Instant::now().checked_add(CUSTOM_TITLEBAR_FULLSCREEN_HIDE_DELAY);
            titlebar_visibility_changed = true;
        }

        let changed =
            content_hover_changed || titlebar_hover_changed || titlebar_visibility_changed;
        if changed {
            self.rebuild_frame();
        }
        changed
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        if self.empty_session_welcome_visible() {
            return false;
        }

        if mouse_local_override_action(
            self.mouse_reporting_active(),
            self.mouse_override_policy,
            self.modifiers.state(),
            MouseLocalOverrideEvent::Wheel,
            self.selection_anchor,
        ) == MouseLocalOverrideAction::Scrollback
        {
            return self.scroll_viewport(delta);
        }

        if self.mouse_reporting_active() {
            return self.handle_mouse_report_wheel(delta);
        }

        self.scroll_viewport(delta)
    }

    fn handle_focus_event(&mut self, focused: bool) -> bool {
        if self.empty_session_welcome_visible() {
            return false;
        }

        let kind = if focused {
            FocusEventKind::In
        } else {
            FocusEventKind::Out
        };
        let bytes = encode_terminal_focus_event(kind, self.terminal.input_modes().mouse);

        self.write_input_report(bytes, "focus event")
    }

    fn handle_mouse_report_button(&mut self, state: ElementState, button: MouseButton) -> bool {
        let Some(position) = self.pointer_position else {
            return false;
        };
        let Some(button) = mouse_button_code_from_winit(button) else {
            return false;
        };
        let Some(cell) = self.terminal_cell_point_for_position(position) else {
            return false;
        };
        let pixel = self.terminal_pixel_mouse_position_for_position(position);
        let modifiers = mouse_modifiers_from_winit(self.modifiers.state());
        let bytes = self.mouse_report.button(
            state,
            button,
            cell,
            pixel,
            modifiers,
            self.terminal.input_modes().mouse,
        );

        self.write_input_report(bytes, "mouse report")
    }

    fn handle_mouse_report_motion(&mut self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position = Some(position);
        let Some(cell) = self.terminal_cell_point_for_position(position) else {
            return false;
        };
        let pixel = self.terminal_pixel_mouse_position_for_position(position);
        let modifiers = mouse_modifiers_from_winit(self.modifiers.state());
        let bytes =
            self.mouse_report
                .motion(cell, pixel, modifiers, self.terminal.input_modes().mouse);

        self.write_input_report(bytes, "mouse report")
    }

    fn handle_mouse_report_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let lines = scroll_lines_for_delta(delta, self.metrics);
        if lines == 0 {
            return false;
        }

        let position = self
            .pointer_position
            .unwrap_or_else(|| PhysicalPosition::new(0.0, 0.0));
        let Some(cell) = self.terminal_cell_point_for_position(position) else {
            return false;
        };
        let pixel = self.terminal_pixel_mouse_position_for_position(position);
        let button = if lines > 0 {
            MouseButtonCode::WheelUp
        } else {
            MouseButtonCode::WheelDown
        };
        let modifiers = mouse_modifiers_from_winit(self.modifiers.state());
        let bytes = self.mouse_report.wheel(
            button,
            cell,
            pixel,
            modifiers,
            self.terminal.input_modes().mouse,
        );

        self.write_input_report(bytes, "mouse report")
    }

    fn write_input_report(&mut self, bytes: Option<Vec<u8>>, label: &str) -> bool {
        let Some(bytes) = bytes else {
            return false;
        };

        if let Err(err) = self.app.write_input(&bytes) {
            let message = format!("\r\n[{label} write failed: {err:#}]\r\n");
            self.terminal.feed(message.as_bytes());
            self.rebuild_frame();
            return true;
        }

        false
    }
}

impl ApplicationHandler<NativeWindowEvent> for TerminalWindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = witty_window_identity_attributes(
            Window::default_attributes()
                .with_title(self.window_title.clone())
                .with_decorations(false)
                .with_transparent(self.visual_config.requires_window_transparency())
                .with_inner_size(terminal_window_initial_inner_size(
                    self.initial_window_size,
                    self.metrics,
                )),
        );
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err:#}");
                event_loop.exit();
                return;
            }
        };
        window.set_ime_allowed(true);
        window.set_ime_purpose(ImePurpose::Terminal);
        self.apply_window_scale_factor(window.scale_factor());

        let size = window.inner_size();
        if self.report_startup {
            eprintln!(
                "{}",
                native_window_startup_report_line(
                    size,
                    self.active_session_close_fallback_policy,
                    &self.font_config,
                    &self.visual_config,
                    self.font_data.len(),
                )
            );
        }
        let renderer =
            match pollster::block_on(WgpuRectRenderer::new_with_font_config_data_and_visual(
                window.clone(),
                size.width,
                size.height,
                self.font_config.clone(),
                self.font_data.clone(),
                self.visual_config.clone(),
            )) {
                Ok(renderer) => renderer,
                Err(err) => {
                    eprintln!("{}", native_renderer_startup_error_message(&err));
                    event_loop.exit();
                    return;
                }
            };

        self.renderer = Some(renderer);
        self.window = Some(window);
        self.resize_grid(size);
        self.ensure_background_image_load(event_loop, size);
        self.sync_ime_cursor_area(self.active_ime_cursor(self.terminal.snapshot().cursor.position));
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: NativeWindowEvent) {
        match event {
            NativeWindowEvent::BackgroundImageReady => {
                let _ = self.apply_background_image_results();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let is_target_window = self
            .window
            .as_ref()
            .is_some_and(|window| window.id() == window_id);
        if !is_target_window {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => {
                let changed = self.handle_modifiers_changed(modifiers);
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_key(&event);
                self.request_redraw();
            }
            WindowEvent::Ime(event) => {
                let changed = self.handle_ime_event(event);
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let changed = self.handle_cursor_moved(position);
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::CursorLeft { .. } => {
                let changed = self.handle_cursor_left();
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let changed = self.handle_mouse_input(state, button);
                if self.take_restart_exit_request() {
                    event_loop.exit();
                    return;
                }
                if self.take_window_close_request() {
                    event_loop.exit();
                    return;
                }
                if self.take_fallback_local_session_request() {
                    if let Err(err) = self.start_fallback_local_session() {
                        eprintln!("failed to start fallback local session: {err:#}");
                        self.session_tab_notice =
                            Some(NativeSessionTabStripNotice::LastActiveCloseBlocked);
                        self.rebuild_frame();
                    }
                    self.request_redraw();
                    return;
                }
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let changed = self.handle_mouse_wheel(delta);
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::Focused(focused) => {
                self.log_fullscreen_debug(
                    if focused {
                        "focused-true"
                    } else {
                        "focused-false"
                    },
                    None,
                );
                let focus_changed = self.handle_focus_event(focused);
                let fullscreen_changed = self.sync_window_fullscreen_state_from_window();
                if self.window_fullscreen {
                    self.schedule_fullscreen_reaffirm();
                }
                let changed = focus_changed || fullscreen_changed;
                self.request_redraw_if_changed(changed);
            }
            WindowEvent::Resized(size) => {
                self.log_fullscreen_debug("resized", None);
                let scale_factor = self.window.as_ref().map(|window| {
                    self.effective_window_scale_factor(window, window.scale_factor())
                });
                let scale_changed = scale_factor
                    .map(|scale_factor| self.apply_window_scale_factor(scale_factor))
                    .unwrap_or(false);
                let fullscreen_changed = self.sync_window_fullscreen_state_from_window();
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.resize_grid(size);
                self.ensure_background_image_load(event_loop, size);
                if scale_changed {
                    let _ = self.refresh_titlebar_hover_for_current_pointer();
                    let _ = self.refresh_session_tab_hover_for_current_pointer();
                    let _ = self.refresh_profile_action_hover_for_current_pointer();
                    let _ = self.refresh_empty_session_welcome_hover_for_current_pointer();
                }
                if fullscreen_changed || scale_changed {
                    self.rebuild_frame();
                }
                if scale_changed {
                    self.sync_ime_cursor_area(
                        self.active_ime_cursor(self.terminal.snapshot().cursor.position),
                    );
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.log_fullscreen_debug("scale-factor-changed", None);
                let scale_factor = self
                    .window
                    .as_ref()
                    .map(|window| self.effective_window_scale_factor(window, scale_factor))
                    .unwrap_or(scale_factor);
                let scale_changed = self.apply_window_scale_factor(scale_factor);
                let size = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size())
                    .unwrap_or(PhysicalSize::new(0, 0));
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.resize_grid(size);
                self.ensure_background_image_load(event_loop, size);
                if scale_changed {
                    self.refresh_search_after_terminal_change();
                    let _ = self.refresh_titlebar_hover_for_current_pointer();
                    let _ = self.refresh_session_tab_hover_for_current_pointer();
                    let _ = self.refresh_profile_action_hover_for_current_pointer();
                    let _ = self.refresh_empty_session_welcome_hover_for_current_pointer();
                    self.rebuild_frame();
                    self.sync_ime_cursor_area(
                        self.active_ime_cursor(self.terminal.snapshot().cursor.position),
                    );
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let _ = self.apply_background_image_results();
                if let Some(renderer) = &mut self.renderer {
                    if let Err(err) = renderer.render(&self.frame) {
                        eprintln!("render failed: {err:#}");
                    } else if self.report_startup && !self.first_frame_reported {
                        let size = self
                            .window
                            .as_ref()
                            .map(|window| window.inner_size())
                            .unwrap_or(PhysicalSize::new(0, 0));
                        eprintln!(
                            "{}",
                            native_window_first_frame_report_line(
                                size,
                                self.frame.stats,
                                &self.font_config,
                                &self.visual_config,
                                self.font_data.len(),
                            )
                        );
                        self.first_frame_reported = true;
                    }
                }
                if self.background_image_load_pending {
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let background_image_changed = self.apply_background_image_results();
        if background_image_changed && !self.background_image_load_pending {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        let mut changed = background_image_changed;
        self.schedule_background_image_result_poll(event_loop);
        changed |= self.poll_transport();
        if self.take_window_close_request() {
            event_loop.exit();
            return;
        }
        if self.take_fallback_local_session_request() {
            if let Err(err) = self.start_fallback_local_session() {
                eprintln!("failed to start fallback local session: {err:#}");
                self.session_tab_notice = Some(NativeSessionTabStripNotice::LastActiveCloseBlocked);
                self.rebuild_frame();
            }
            self.request_redraw();
            return;
        }
        if changed {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }

        if self.apply_fullscreen_reaffirm_timeout(event_loop) {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }

        if let Some(window) = self.window.as_ref().cloned() {
            if self.apply_pending_windowed_restore(&window) {
                if let Some(deadline) = Instant::now().checked_add(FULLSCREEN_REAFFIRM_INTERVAL) {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
                self.schedule_background_image_result_poll(event_loop);
                return;
            }
        }

        if self.apply_synchronized_output_timeout(event_loop) {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }
        if self.synchronized_output_deadline.is_some() {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }
        if self.apply_installed_update_check_timeout(event_loop) {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }
        if self.apply_fullscreen_titlebar_hide_timeout(event_loop) {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }
        if self.apply_about_status_timeout(event_loop) {
            self.schedule_background_image_result_poll(event_loop);
            return;
        }
        self.apply_blink_timeouts(event_loop);
        self.schedule_background_image_result_poll(event_loop);
    }
}

#[cfg(target_os = "linux")]
fn witty_window_identity_attributes(attrs: WindowAttributes) -> WindowAttributes {
    let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(
        attrs,
        WITTY_LINUX_APP_ID,
        WITTY_LINUX_APP_ID,
    );
    winit::platform::x11::WindowAttributesExtX11::with_name(
        attrs,
        WITTY_LINUX_APP_ID,
        WITTY_LINUX_APP_ID,
    )
}

#[cfg(not(target_os = "linux"))]
fn witty_window_identity_attributes(attrs: WindowAttributes) -> WindowAttributes {
    attrs
}

fn terminal_window_title(title: Option<&str>, default_title: &str) -> String {
    title
        .filter(|title| !title.is_empty())
        .unwrap_or(default_title)
        .to_owned()
}

fn terminal_window_initial_inner_size(
    initial_size: Option<GridSize>,
    metrics: CellMetrics,
) -> LogicalSize<f64> {
    let Some(size) = initial_size else {
        return LogicalSize::new(960.0, 540.0);
    };

    LogicalSize::new(
        f64::from(metrics.padding.x * 2.0 + f32::from(size.cols.max(1)) * metrics.cell.width),
        f64::from(
            CUSTOM_TITLEBAR_HEIGHT
                + metrics.padding.y * 2.0
                + f32::from(size.rows.max(1)) * metrics.cell.height,
        ),
    )
}

fn fullscreen_debug_enabled() -> bool {
    tracing::enabled!(target: "witty_app::fullscreen", tracing::Level::DEBUG)
}

fn fullscreen_target_scale_factor(monitor: Option<&MonitorHandle>, window: &Window) -> f64 {
    monitor
        .map(|monitor| monitor.scale_factor())
        .unwrap_or_else(|| window.scale_factor())
}

fn fullscreen_window_debug_state(
    window: &Window,
) -> (
    bool,
    Option<PhysicalSize<u32>>,
    Option<f64>,
    Option<String>,
    Option<PhysicalSize<u32>>,
    Option<f64>,
) {
    let monitor = window.current_monitor();
    (
        window.fullscreen().is_some(),
        Some(window.inner_size()),
        Some(window.scale_factor()),
        monitor.as_ref().and_then(|monitor| monitor.name()),
        monitor.as_ref().map(|monitor| monitor.size()),
        monitor.as_ref().map(|monitor| monitor.scale_factor()),
    )
}

fn apply_borderless_fullscreen(window: &Window, monitor: Option<MonitorHandle>) {
    window.set_fullscreen(Some(Fullscreen::Borderless(monitor)));
}

fn force_borderless_fullscreen(window: &Window, monitor: Option<MonitorHandle>) {
    window.set_fullscreen(None);
    apply_borderless_fullscreen(window, monitor);
}

fn clear_fullscreen_size_guard(window: &Window) {
    window.set_min_inner_size::<PhysicalSize<u32>>(None);
}

fn window_fullscreen_is_effective(window: &Window, target_size: Option<PhysicalSize<u32>>) -> bool {
    let fullscreen = window.fullscreen().is_some();
    let monitor_size =
        target_size.or_else(|| window.current_monitor().map(|monitor| monitor.size()));
    fullscreen_is_effective_for_size(fullscreen, window.inner_size(), monitor_size)
}

fn fullscreen_is_effective_for_size(
    fullscreen: bool,
    inner_size: PhysicalSize<u32>,
    monitor_size: Option<PhysicalSize<u32>>,
) -> bool {
    if !fullscreen {
        return false;
    }

    let Some(monitor_size) = monitor_size else {
        return true;
    };

    !fullscreen_inner_size_is_degraded(inner_size, monitor_size)
}

fn fullscreen_inner_size_is_degraded(
    inner_size: PhysicalSize<u32>,
    monitor_size: PhysicalSize<u32>,
) -> bool {
    inner_size
        .width
        .saturating_add(FULLSCREEN_MONITOR_SIZE_TOLERANCE_PX)
        < monitor_size.width
        || inner_size
            .height
            .saturating_add(FULLSCREEN_MONITOR_SIZE_TOLERANCE_PX)
            < monitor_size.height
}

fn sane_window_scale_factor(scale_factor: f64) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor as f32
    } else {
        1.0
    }
}

fn native_window_startup_report_line(
    size: PhysicalSize<u32>,
    active_session_close_fallback_policy: NativeActiveSessionCloseFallbackPolicy,
    font_config: &RendererFontConfig,
    visual_config: &RendererVisualConfig,
    font_source_count: usize,
) -> String {
    serde_json::to_string(&native_window_startup_report_json(
        size,
        active_session_close_fallback_policy,
        font_config,
        visual_config,
        font_source_count,
    ))
    .unwrap_or_else(|err| {
        format!(
            "{{\"event\":\"witty.native_window_startup\",\"serialization_error\":{:?}}}",
            err.to_string()
        )
    })
}

fn native_window_startup_report_json(
    size: PhysicalSize<u32>,
    active_session_close_fallback_policy: NativeActiveSessionCloseFallbackPolicy,
    font_config: &RendererFontConfig,
    visual_config: &RendererVisualConfig,
    font_source_count: usize,
) -> serde_json::Value {
    let policy = native_wgpu_backend_policy();
    serde_json::json!({
        "event": "witty.native_window_startup",
        "renderer": "wgpu",
        "native_backend_policy": policy.label(),
        "last_active_close_policy": active_session_close_fallback_policy.as_config_value(),
        "opengl_only": policy.is_opengl_only(),
        "honors_wgpu_backend_env": policy.honors_wgpu_backend_env(),
        "surface_width": size.width,
        "surface_height": size.height,
        "font_family": font_config.family(),
        "font_size": font_config.font_size(),
        "font_scale_factor": font_config.font_scale_factor(),
        "terminal_padding": font_config.terminal_padding(),
        "effective_font_size": font_config.effective_font_size(),
        "background_opacity": visual_config.background_opacity(),
        "background_image": visual_config.has_background_image(),
        "background_image_fit": visual_config.background_image_fit().as_config_value(),
        "transparent_window": visual_config.requires_window_transparency(),
        "font_source_count": font_source_count,
        "will_request_adapter": true,
        "vulkan_enabled_by_witty": false,
        "chromium": false,
    })
}

fn native_window_first_frame_report_line(
    size: PhysicalSize<u32>,
    stats: FrameStats,
    font_config: &RendererFontConfig,
    visual_config: &RendererVisualConfig,
    font_source_count: usize,
) -> String {
    serde_json::to_string(&native_window_first_frame_report_json(
        size,
        stats,
        font_config,
        visual_config,
        font_source_count,
    ))
    .unwrap_or_else(|err| {
        format!(
            "{{\"event\":\"witty.native_window_first_frame\",\"serialization_error\":{:?}}}",
            err.to_string()
        )
    })
}

fn native_window_first_frame_report_json(
    size: PhysicalSize<u32>,
    stats: FrameStats,
    font_config: &RendererFontConfig,
    visual_config: &RendererVisualConfig,
    font_source_count: usize,
) -> serde_json::Value {
    let policy = native_wgpu_backend_policy();
    serde_json::json!({
        "event": "witty.native_window_first_frame",
        "renderer": "wgpu",
        "native_backend_policy": policy.label(),
        "opengl_only": policy.is_opengl_only(),
        "honors_wgpu_backend_env": policy.honors_wgpu_backend_env(),
        "surface_width": size.width,
        "surface_height": size.height,
        "font_family": font_config.family(),
        "font_size": font_config.font_size(),
        "font_scale_factor": font_config.font_scale_factor(),
        "terminal_padding": font_config.terminal_padding(),
        "effective_font_size": font_config.effective_font_size(),
        "background_opacity": visual_config.background_opacity(),
        "background_image": visual_config.has_background_image(),
        "background_image_fit": visual_config.background_image_fit().as_config_value(),
        "transparent_window": visual_config.requires_window_transparency(),
        "font_source_count": font_source_count,
        "visible_rows": stats.visible_rows,
        "visible_cols": stats.visible_cols,
        "glyph_runs": stats.glyph_runs,
        "glyph_chars": stats.glyph_chars,
        "rect_vertices": stats.rect_vertices,
        "cursor_visible": stats.cursor_visible,
        "full_damage": stats.full_damage,
        "damage_regions": stats.damage_regions,
        "vulkan_enabled_by_witty": false,
        "chromium": false,
    })
}

fn native_renderer_startup_error_message(err: &anyhow::Error) -> String {
    let policy = native_wgpu_backend_policy();
    format!(
        "failed to initialize wgpu renderer (native_backend_policy={}, opengl_only={}, honors_wgpu_backend_env={}, vulkan_enabled_by_witty=false): {err:#}",
        policy.label(),
        policy.is_opengl_only(),
        policy.honors_wgpu_backend_env(),
    )
}

fn synchronized_output_deadline_after_poll(
    existing_deadline: Option<Instant>,
    now: Instant,
) -> Option<Instant> {
    existing_deadline.or_else(|| now.checked_add(SYNCHRONIZED_OUTPUT_TIMEOUT))
}

#[cfg(test)]
fn grid_size_for_window(size: PhysicalSize<u32>, metrics: CellMetrics) -> GridSize {
    grid_size_for_window_with_reserved_height(size, metrics, 0.0)
}

fn grid_size_for_window_with_reserved_height(
    size: PhysicalSize<u32>,
    metrics: CellMetrics,
    reserved_height: f32,
) -> GridSize {
    let usable_width = (size.width as f32 - metrics.padding.x * 2.0).max(metrics.cell.width);
    let usable_height =
        (size.height as f32 - reserved_height - metrics.padding.y * 2.0).max(metrics.cell.height);
    GridSize::new(
        (usable_height / metrics.cell.height)
            .floor()
            .clamp(1.0, f32::from(u16::MAX)) as u16,
        (usable_width / metrics.cell.width)
            .floor()
            .clamp(1.0, f32::from(u16::MAX)) as u16,
    )
}

fn terminal_bottom_align_y_offset(
    surface_size: PhysicalSize<u32>,
    metrics: CellMetrics,
    size: GridSize,
    reserved_height: f32,
) -> f32 {
    let occupied_height =
        reserved_height + metrics.padding.y * 2.0 + f32::from(size.rows) * metrics.cell.height;
    (surface_size.height as f32 - occupied_height).max(0.0)
}

#[cfg(test)]
fn cell_point_for_position(
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    size: GridSize,
) -> CellPoint {
    cell_point_for_pixel_point(
        PixelPoint {
            x: position.x as f32,
            y: position.y as f32,
        },
        metrics,
        size,
    )
}

fn cell_point_for_pixel_point(
    point: PixelPoint,
    metrics: CellMetrics,
    size: GridSize,
) -> CellPoint {
    let max_row = size.rows.saturating_sub(1);
    let max_col = size.cols.saturating_sub(1);
    let col = cell_index_for_axis(
        f64::from(point.x),
        f64::from(metrics.padding.x),
        f64::from(metrics.cell.width),
        max_col,
    );
    let row = cell_index_for_axis(
        f64::from(point.y),
        f64::from(metrics.padding.y),
        f64::from(metrics.cell.height),
        max_row,
    );

    CellPoint::new(row, col)
}

#[cfg(test)]
fn hyperlink_for_position(
    snapshot: &RenderSnapshot,
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<&TerminalHyperlink> {
    let point = cell_point_for_position(position, metrics, size);
    snapshot.hyperlink_at(point)
}

fn hyperlink_for_pixel_point(
    snapshot: &RenderSnapshot,
    point: PixelPoint,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<&TerminalHyperlink> {
    let point = cell_point_for_pixel_point(point, metrics, size);
    snapshot.hyperlink_at(point)
}

#[cfg(test)]
fn command_block_gutter_hit_for_position(
    shell_integration: &ShellIntegrationState,
    screen: witty_core::TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<witty_ui::TerminalCommandBlockGutterHit> {
    let visual_point = pixel_point_for_position(position)?;
    let point = command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
        shell_integration,
        screen,
        visible_row_anchors,
        visual_point,
        metrics,
        size,
    )?;
    command_block_gutter_hit_for_pixel_point(
        shell_integration,
        screen,
        visible_row_anchors,
        point,
        metrics,
        size,
    )
}

fn command_block_gutter_hit_for_pixel_point(
    shell_integration: &ShellIntegrationState,
    screen: witty_core::TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    point: PixelPoint,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<witty_ui::TerminalCommandBlockGutterHit> {
    command_block_gutter_hit_test_with_anchors(
        shell_integration,
        screen,
        visible_row_anchors,
        point,
        metrics,
        size,
    )
}

#[cfg(test)]
fn select_command_block_gutter_hit_for_position(
    shell_integration: &mut ShellIntegrationState,
    screen: witty_core::TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<u64> {
    let hit = command_block_gutter_hit_for_position(
        shell_integration,
        screen,
        visible_row_anchors,
        position,
        metrics,
        size,
    )?;
    shell_integration.select_completed_block(hit.id)?;
    Some(hit.id)
}

fn select_command_block_gutter_hit_for_pixel_point(
    shell_integration: &mut ShellIntegrationState,
    screen: witty_core::TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    point: PixelPoint,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<u64> {
    let hit = command_block_gutter_hit_for_pixel_point(
        shell_integration,
        screen,
        visible_row_anchors,
        point,
        metrics,
        size,
    )?;
    shell_integration.select_completed_block(hit.id)?;
    Some(hit.id)
}

fn pixel_point_for_position(position: PhysicalPosition<f64>) -> Option<PixelPoint> {
    if !position.x.is_finite() || !position.y.is_finite() {
        return None;
    }

    Some(PixelPoint {
        x: position.x as f32,
        y: position.y as f32,
    })
}

fn cell_index_for_axis(position: f64, padding: f64, cell_extent: f64, max_index: u16) -> u16 {
    if cell_extent <= 0.0 || !position.is_finite() {
        return 0;
    }

    ((position - padding) / cell_extent)
        .floor()
        .clamp(0.0, f64::from(max_index)) as u16
}

fn pixel_position_for_pixel_point(point: PixelPoint) -> Option<PixelMousePosition> {
    Some(PixelMousePosition::new(
        pixel_axis_for_position(f64::from(point.x))?,
        pixel_axis_for_position(f64::from(point.y))?,
    ))
}

fn pixel_axis_for_position(position: f64) -> Option<u16> {
    position
        .is_finite()
        .then(|| position.floor().clamp(0.0, f64::from(u16::MAX)) as u16)
}

fn ordered_cell_range(anchor: CellPoint, current: CellPoint) -> CellRange {
    if (current.row, current.col) < (anchor.row, anchor.col) {
        CellRange {
            start: current,
            end: anchor,
        }
    } else {
        CellRange {
            start: anchor,
            end: current,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ClickStamp {
    point: CellPoint,
    at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct MouseSelectionPress {
    range: CellRange,
    anchor: Option<CellPoint>,
    click: ClickStamp,
    completed: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::ShiftSelect => "shift-select",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouseLocalOverrideAction {
    None,
    Selection,
    PrimaryPaste,
    Scrollback,
}

#[derive(Clone, Copy, Debug)]
enum MouseLocalOverrideEvent {
    Button {
        state: ElementState,
        button: MouseButton,
    },
    Motion,
    Wheel,
}

#[derive(Clone, Copy, Debug, Default)]
struct NativeMouseReportState {
    pressed_button: Option<MouseButtonCode>,
    last_reported_cell: Option<CellPoint>,
}

impl NativeMouseReportState {
    fn button(
        &mut self,
        state: ElementState,
        button: MouseButtonCode,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
        modifiers: MouseModifiers,
        modes: witty_core::TerminalMouseModes,
    ) -> Option<Vec<u8>> {
        if !modes.reports_mouse() {
            return None;
        }

        let kind = match state {
            ElementState::Pressed => {
                self.pressed_button = Some(button);
                MouseEventKind::Press
            }
            ElementState::Released => MouseEventKind::Release,
        };
        let bytes = self.encode(kind, button, cell, pixel, modifiers, modes);
        if state == ElementState::Released && self.pressed_button == Some(button) {
            self.pressed_button = None;
        }
        bytes
    }

    fn motion(
        &mut self,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
        modifiers: MouseModifiers,
        modes: witty_core::TerminalMouseModes,
    ) -> Option<Vec<u8>> {
        if !modes.reports_mouse() {
            return None;
        }

        if self.last_reported_cell == Some(cell) {
            return None;
        }

        let button = self.pressed_button.unwrap_or(MouseButtonCode::None);
        self.encode(
            MouseEventKind::Motion,
            button,
            cell,
            pixel,
            modifiers,
            modes,
        )
    }

    fn wheel(
        &mut self,
        button: MouseButtonCode,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
        modifiers: MouseModifiers,
        modes: witty_core::TerminalMouseModes,
    ) -> Option<Vec<u8>> {
        if !modes.reports_mouse() {
            return None;
        }

        self.encode(MouseEventKind::Wheel, button, cell, pixel, modifiers, modes)
    }

    fn encode(
        &mut self,
        kind: MouseEventKind,
        button: MouseButtonCode,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
        modifiers: MouseModifiers,
        modes: witty_core::TerminalMouseModes,
    ) -> Option<Vec<u8>> {
        let bytes = encode_terminal_mouse_event(
            TerminalMouseEvent {
                kind,
                button,
                cell,
                pixel,
                modifiers,
            },
            modes,
        )?;
        self.last_reported_cell = Some(cell);
        Some(bytes)
    }
}

fn mouse_button_code_from_winit(button: MouseButton) -> Option<MouseButtonCode> {
    match button {
        MouseButton::Left => Some(MouseButtonCode::Left),
        MouseButton::Middle => Some(MouseButtonCode::Middle),
        MouseButton::Right => Some(MouseButtonCode::Right),
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => None,
    }
}

fn mouse_modifiers_from_winit(modifiers: ModifiersState) -> MouseModifiers {
    MouseModifiers {
        shift: modifiers.shift_key(),
        alt: modifiers.alt_key(),
        control: modifiers.control_key(),
    }
}

fn is_hyperlink_activation_click(
    state: ElementState,
    button: MouseButton,
    modifiers: ModifiersState,
) -> bool {
    if state != ElementState::Pressed || button != MouseButton::Left {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        modifiers.super_key()
    }

    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control_key()
    }
}

fn mouse_local_override_action(
    reporting_active: bool,
    policy: MouseSelectionOverridePolicy,
    modifiers: ModifiersState,
    event: MouseLocalOverrideEvent,
    selection_anchor: Option<CellPoint>,
) -> MouseLocalOverrideAction {
    if !reporting_active {
        return MouseLocalOverrideAction::None;
    }

    if selection_anchor.is_some() {
        return match event {
            MouseLocalOverrideEvent::Button {
                state: ElementState::Released,
                button: MouseButton::Left,
            }
            | MouseLocalOverrideEvent::Motion => MouseLocalOverrideAction::Selection,
            _ => MouseLocalOverrideAction::None,
        };
    }

    if policy != MouseSelectionOverridePolicy::ShiftSelect || !modifiers.shift_key() {
        return MouseLocalOverrideAction::None;
    }

    match event {
        MouseLocalOverrideEvent::Button {
            state: ElementState::Pressed,
            button: MouseButton::Left,
        } => MouseLocalOverrideAction::Selection,
        MouseLocalOverrideEvent::Button {
            state: ElementState::Pressed,
            button: MouseButton::Middle,
        } => MouseLocalOverrideAction::PrimaryPaste,
        MouseLocalOverrideEvent::Wheel => MouseLocalOverrideAction::Scrollback,
        MouseLocalOverrideEvent::Button { .. } | MouseLocalOverrideEvent::Motion => {
            MouseLocalOverrideAction::None
        }
    }
}

fn selection_for_left_press(
    terminal: &BasicTerminal,
    previous_click: Option<ClickStamp>,
    point: CellPoint,
    now: Instant,
) -> MouseSelectionPress {
    let is_double_click = is_left_double_click(previous_click, point, now);
    let range = if is_double_click {
        terminal
            .word_range_at(point)
            .unwrap_or_else(|| collapsed_range(point))
    } else {
        collapsed_range(point)
    };

    MouseSelectionPress {
        range,
        anchor: (!is_double_click).then_some(point),
        click: ClickStamp { point, at: now },
        completed: is_double_click,
    }
}

fn collapsed_range(point: CellPoint) -> CellRange {
    CellRange {
        start: point,
        end: point,
    }
}

fn selection_release_should_publish(
    anchor: Option<CellPoint>,
    selection: Option<CellRange>,
) -> bool {
    let Some(anchor) = anchor else {
        return false;
    };
    let Some(selection) = selection else {
        return false;
    };

    selection != collapsed_range(anchor)
}

fn is_left_double_click(
    previous_click: Option<ClickStamp>,
    point: CellPoint,
    now: Instant,
) -> bool {
    let Some(previous_click) = previous_click else {
        return false;
    };
    let Some(elapsed) = now.checked_duration_since(previous_click.at) else {
        return false;
    };

    elapsed <= DOUBLE_CLICK_MAX_INTERVAL
        && point.row.abs_diff(previous_click.point.row) <= DOUBLE_CLICK_MAX_CELL_DISTANCE
        && point.col.abs_diff(previous_click.point.col) <= DOUBLE_CLICK_MAX_CELL_DISTANCE
}

fn scroll_lines_for_delta(delta: MouseScrollDelta, metrics: CellMetrics) -> i16 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => rounded_scroll_lines(f64::from(y)),
        MouseScrollDelta::PixelDelta(position) => {
            rounded_scroll_lines(position.y / f64::from(metrics.cell.height))
        }
    }
}

fn rounded_scroll_lines(value: f64) -> i16 {
    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round();
    let effective = if rounded == 0.0 && value != 0.0 {
        value.signum()
    } else {
        rounded
    };

    effective.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn command_shortcut_for_key(logical_key: &Key, commands: &[CommandRegistration]) -> Option<String> {
    match logical_key {
        Key::Named(NamedKey::F1) if has_command(commands, "witty.about") => {
            Some("witty.about".to_owned())
        }
        Key::Named(NamedKey::F2) => commands
            .iter()
            .find(|command| command.source_plugin != "builtin")
            .map(|command| command.id.clone()),
        _ => None,
    }
}

fn is_command_palette_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && state.shift_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case("p"))
}

fn is_search_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && state.shift_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case("f"))
}

fn is_toggle_fullscreen_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    !state.control_key()
        && !state.alt_key()
        && !state.super_key()
        && !state.shift_key()
        && matches!(logical_key, Key::Named(NamedKey::F11))
}

fn is_new_local_tab_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && state.shift_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case("t"))
}

fn is_session_switcher_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && !state.alt_key()
        && !state.super_key()
        && matches!(logical_key, Key::Named(NamedKey::Tab))
}

fn is_frame_diagnostics_shortcut(logical_key: &Key) -> bool {
    matches!(logical_key, Key::Named(NamedKey::F3))
}

fn runtime_font_size_shortcut_action(
    event: &KeyEvent,
    modifiers: Modifiers,
) -> Option<RuntimeFontSizeAction> {
    runtime_font_size_shortcut_action_for_parts(&event.logical_key, event.physical_key, modifiers)
}

fn runtime_font_size_shortcut_action_for_parts(
    logical_key: &Key,
    physical_key: PhysicalKey,
    modifiers: Modifiers,
) -> Option<RuntimeFontSizeAction> {
    let state = modifiers.state();
    if !state.control_key() || state.alt_key() || state.super_key() {
        return None;
    }

    match logical_key {
        Key::Character(value) if value == "+" || value == "=" => {
            Some(RuntimeFontSizeAction::Increase)
        }
        Key::Character(value) if value == "-" || value == "_" => {
            Some(RuntimeFontSizeAction::Decrease)
        }
        Key::Character(value) if value == "0" => Some(RuntimeFontSizeAction::Reset),
        _ => match physical_key {
            PhysicalKey::Code(KeyCode::Equal) | PhysicalKey::Code(KeyCode::NumpadAdd) => {
                Some(RuntimeFontSizeAction::Increase)
            }
            PhysicalKey::Code(KeyCode::Minus) | PhysicalKey::Code(KeyCode::NumpadSubtract) => {
                Some(RuntimeFontSizeAction::Decrease)
            }
            PhysicalKey::Code(KeyCode::Digit0) | PhysicalKey::Code(KeyCode::Numpad0) => {
                Some(RuntimeFontSizeAction::Reset)
            }
            _ => None,
        },
    }
}

fn runtime_font_config_after_action(
    config: &RendererFontConfig,
    action: RuntimeFontSizeAction,
) -> RendererFontConfig {
    RendererFontConfig::with_font_size(
        config.family().map(str::to_owned),
        runtime_font_size_after_action(config.font_size(), action),
    )
    .with_scale_factor(config.font_scale_factor())
    .with_terminal_padding(config.terminal_padding())
}

fn runtime_font_size_after_action(current: u16, action: RuntimeFontSizeAction) -> u16 {
    match action {
        RuntimeFontSizeAction::Increase => current
            .saturating_add(1)
            .clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE),
        RuntimeFontSizeAction::Decrease => current
            .saturating_sub(1)
            .clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE),
        RuntimeFontSizeAction::Reset => {
            DEFAULT_TERMINAL_FONT_SIZE.clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE)
        }
    }
}

fn is_copy_selection_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && state.shift_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case("c"))
}

fn is_paste_clipboard_shortcut(logical_key: &Key, modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    state.control_key()
        && state.shift_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case("v"))
}

fn text_input_allowed(modifiers: Modifiers) -> bool {
    let state = modifiers.state();
    !state.control_key() && !state.alt_key() && !state.super_key()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SearchKeyAction {
    Close,
    Next,
    Previous,
    HistoryPrevious,
    HistoryNext,
    Backspace,
    ToggleCaseSensitive,
    ToggleRegex,
    ToggleWholeWord,
    ToggleNormalizeNfc,
    InputText(String),
    None,
}

fn search_key_action(
    logical_key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
) -> SearchKeyAction {
    match logical_key {
        Key::Named(NamedKey::Escape) => SearchKeyAction::Close,
        Key::Named(NamedKey::Enter) if modifiers.state().shift_key() => SearchKeyAction::Previous,
        Key::Named(NamedKey::Enter) => SearchKeyAction::Next,
        Key::Named(NamedKey::ArrowUp) => SearchKeyAction::HistoryPrevious,
        Key::Named(NamedKey::ArrowDown) => SearchKeyAction::HistoryNext,
        Key::Named(NamedKey::Backspace) => SearchKeyAction::Backspace,
        _ if is_search_option_shortcut(logical_key, modifiers, "c") => {
            SearchKeyAction::ToggleCaseSensitive
        }
        _ if is_search_option_shortcut(logical_key, modifiers, "r") => SearchKeyAction::ToggleRegex,
        _ if is_search_option_shortcut(logical_key, modifiers, "w") => {
            SearchKeyAction::ToggleWholeWord
        }
        _ if is_search_option_shortcut(logical_key, modifiers, "n") => {
            SearchKeyAction::ToggleNormalizeNfc
        }
        _ if text_input_allowed(modifiers) => text
            .filter(|text| !text.is_empty())
            .map(|text| SearchKeyAction::InputText(text.to_owned()))
            .unwrap_or(SearchKeyAction::None),
        _ => SearchKeyAction::None,
    }
}

fn is_search_option_shortcut(logical_key: &Key, modifiers: Modifiers, key: &str) -> bool {
    modifiers.state().alt_key()
        && !modifiers.state().control_key()
        && !modifiers.state().super_key()
        && matches!(logical_key, Key::Character(value) if value.eq_ignore_ascii_case(key))
}

fn apply_search_command(
    terminal: &mut BasicTerminal,
    search: &mut TerminalSearch,
    command_id: &str,
) -> bool {
    match command_id {
        SEARCH_OPEN_COMMAND_ID => {
            let selected_text = terminal.selected_text();
            search.open(&terminal.search_text_rows(), selected_text.as_deref());
            scroll_terminal_to_active_search_match(terminal, search);
            true
        }
        SEARCH_CLOSE_COMMAND_ID => {
            search.close();
            true
        }
        SEARCH_NEXT_COMMAND_ID => {
            search.repeat_next(&terminal.search_text_rows());
            scroll_terminal_to_active_search_match(terminal, search);
            true
        }
        SEARCH_PREVIOUS_COMMAND_ID => {
            search.repeat_previous(&terminal.search_text_rows());
            scroll_terminal_to_active_search_match(terminal, search);
            true
        }
        _ => false,
    }
}

fn scroll_terminal_to_active_search_match(terminal: &mut BasicTerminal, search: &TerminalSearch) {
    let Some(active) = search.active_match() else {
        return;
    };

    terminal.scroll_to_search_match(active.row, SEARCH_SCROLL_BUFFER_ROWS);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClipboardSelection {
    Clipboard,
    Primary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClipboardImage {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

trait ClipboardSink {
    fn set_text(&mut self, selection: ClipboardSelection, text: &str) -> Result<()>;
    fn get_text(&mut self, selection: ClipboardSelection) -> Result<String>;
    fn get_image(&mut self, _selection: ClipboardSelection) -> Result<Option<ClipboardImage>> {
        Ok(None)
    }
    fn save_image_as_png(&mut self, selection: ClipboardSelection, path: &Path) -> Result<bool> {
        let Some(image) = self.get_image(selection)? else {
            return Ok(false);
        };
        write_clipboard_image_png(&image, path)?;
        Ok(true)
    }
}

struct SystemClipboardSink {
    clipboard: Option<arboard::Clipboard>,
    init_error: Option<String>,
}

#[derive(Default)]
struct MemoryClipboardSink {
    clipboard_text: String,
    primary_text: String,
    clipboard_writes: usize,
    primary_writes: usize,
}

impl SystemClipboardSink {
    fn new() -> Self {
        match arboard::Clipboard::new() {
            Ok(clipboard) => Self {
                clipboard: Some(clipboard),
                init_error: None,
            },
            Err(err) => Self {
                clipboard: None,
                init_error: Some(err.to_string()),
            },
        }
    }
}

impl ClipboardSink for SystemClipboardSink {
    fn set_text(&mut self, selection: ClipboardSelection, text: &str) -> Result<()> {
        let clipboard = self.clipboard_mut()?;
        match selection {
            ClipboardSelection::Clipboard => clipboard
                .set_text(text.to_owned())
                .context("failed to write selected text to system clipboard"),
            ClipboardSelection::Primary => set_system_primary_text(clipboard, text),
        }
    }

    fn get_text(&mut self, selection: ClipboardSelection) -> Result<String> {
        let clipboard = self.clipboard_mut()?;
        match selection {
            ClipboardSelection::Clipboard => read_system_clipboard_text(clipboard),
            ClipboardSelection::Primary => get_system_primary_text(clipboard),
        }
    }

    fn get_image(&mut self, selection: ClipboardSelection) -> Result<Option<ClipboardImage>> {
        let clipboard = self.clipboard_mut()?;
        match selection {
            ClipboardSelection::Clipboard => read_arboard_clipboard_image(clipboard),
            ClipboardSelection::Primary => Ok(None),
        }
    }

    fn save_image_as_png(&mut self, selection: ClipboardSelection, path: &Path) -> Result<bool> {
        match selection {
            ClipboardSelection::Clipboard => match &mut self.clipboard {
                Some(clipboard) => save_system_clipboard_image_as_png(clipboard, path),
                None => {
                    let init_error = self.init_error.as_deref().unwrap_or("unknown init error");
                    tracing::info!(
                        target: "witty_app::clipboard",
                        init_error,
                        "arboard clipboard is unavailable; trying GTK/GDK image path"
                    );
                    save_preferred_gtk_clipboard_image_as_png(path).map_err(|fallback_err| {
                        anyhow!(
                            "system clipboard unavailable: {init_error}; \
                             GTK/GDK image fallback failed: {fallback_err:#}"
                        )
                    })
                }
            },
            ClipboardSelection::Primary => Ok(false),
        }
    }
}

fn read_arboard_clipboard_image(
    clipboard: &mut arboard::Clipboard,
) -> Result<Option<ClipboardImage>> {
    match clipboard.get_image() {
        Ok(image) => Ok(Some(ClipboardImage {
            width: image.width,
            height: image.height,
            rgba: image.bytes.into_owned(),
        })),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(err) => Err(err).context("failed to read image from system clipboard"),
    }
}

fn read_system_clipboard_text(clipboard: &mut arboard::Clipboard) -> Result<String> {
    if should_prefer_gtk_clipboard() {
        return match read_preferred_gtk_clipboard_text() {
            Ok(text) => Ok(text),
            Err(gtk_err) => {
                tracing::warn!(
                    target: "witty_app::clipboard",
                    error = %gtk_err,
                    "preferred GTK/GDK clipboard text path failed; trying arboard fallback"
                );
                clipboard.get_text().with_context(|| {
                    format!(
                        "{gtk_err:#}; arboard fallback failed to read text from system clipboard"
                    )
                })
            }
        };
    }

    match clipboard
        .get_text()
        .context("failed to read text from system clipboard")
    {
        Ok(text) => Ok(text),
        Err(arboard_err) => match read_gtk_clipboard_text() {
            Ok(Some(text)) => {
                tracing::info!(
                    target: "witty_app::clipboard",
                    arboard_error = %arboard_err,
                    bytes = text.len(),
                    "used GTK/GDK clipboard text fallback after arboard failed"
                );
                Ok(text)
            }
            Ok(None) => Err(anyhow!(
                "{arboard_err:#}; GTK/GDK text fallback found no text in the clipboard"
            )),
            Err(fallback_err) => Err(anyhow!(
                "{arboard_err:#}; GTK/GDK text fallback failed: {fallback_err:#}"
            )),
        },
    }
}

fn read_preferred_gtk_clipboard_text() -> Result<String> {
    match read_gtk_clipboard_text()? {
        Some(text) => {
            tracing::info!(
                target: "witty_app::clipboard",
                bytes = text.len(),
                "read clipboard text with preferred GTK/GDK Wayland path"
            );
            Ok(text)
        }
        None => {
            tracing::info!(
                target: "witty_app::clipboard",
                "preferred GTK/GDK Wayland path found no clipboard text"
            );
            bail!("clipboard does not contain text");
        }
    }
}

fn save_system_clipboard_image_as_png(
    clipboard: &mut arboard::Clipboard,
    path: &Path,
) -> Result<bool> {
    if should_prefer_gtk_clipboard() {
        return save_preferred_gtk_clipboard_image_as_png(path);
    }

    match read_arboard_clipboard_image(clipboard) {
        Ok(Some(image)) => {
            write_clipboard_image_png(&image, path)?;
            tracing::info!(
                target: "witty_app::clipboard",
                path = %path.display(),
                "saved clipboard image with arboard"
            );
            Ok(true)
        }
        Ok(None) => try_save_gtk_clipboard_image_after_arboard_miss(path),
        Err(arboard_err) => match save_gtk_clipboard_image_as_png(path) {
            Ok(true) => {
                tracing::info!(
                    target: "witty_app::clipboard",
                    arboard_error = %arboard_err,
                    path = %path.display(),
                    "used GTK/GDK clipboard image fallback after arboard failed"
                );
                Ok(true)
            }
            Ok(false) => Err(anyhow!(
                "{arboard_err:#}; GTK/GDK image fallback found no image in the clipboard"
            )),
            Err(fallback_err) => Err(anyhow!(
                "{arboard_err:#}; GTK/GDK image fallback failed: {fallback_err:#}"
            )),
        },
    }
}

fn save_preferred_gtk_clipboard_image_as_png(path: &Path) -> Result<bool> {
    match save_gtk_clipboard_image_as_png(path) {
        Ok(true) => {
            tracing::info!(
                target: "witty_app::clipboard",
                path = %path.display(),
                "saved clipboard image with preferred GTK/GDK Wayland path"
            );
            Ok(true)
        }
        Ok(false) => {
            tracing::info!(
                target: "witty_app::clipboard",
                "preferred GTK/GDK Wayland path found no clipboard image"
            );
            Ok(false)
        }
        Err(err) => {
            tracing::warn!(
                target: "witty_app::clipboard",
                error = %err,
                "preferred GTK/GDK Wayland clipboard image path failed"
            );
            Err(err)
        }
    }
}

fn try_save_gtk_clipboard_image_after_arboard_miss(path: &Path) -> Result<bool> {
    match save_gtk_clipboard_image_as_png(path) {
        Ok(true) => {
            tracing::info!(
                target: "witty_app::clipboard",
                path = %path.display(),
                "saved clipboard image with GTK/GDK fallback after arboard reported no image"
            );
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(err) => {
            tracing::info!(
                target: "witty_app::clipboard",
                error = %err,
                "GTK/GDK clipboard image fallback failed after arboard reported no image"
            );
            Ok(false)
        }
    }
}

#[cfg(target_os = "linux")]
fn should_prefer_gtk_clipboard() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(not(target_os = "linux"))]
fn should_prefer_gtk_clipboard() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn read_gtk_clipboard_text() -> Result<Option<String>> {
    let output = process::Command::new("/usr/bin/python3")
        .arg("-c")
        .arg(GTK_CLIPBOARD_TEXT_HELPER)
        .stdin(process::Stdio::null())
        .output()
        .context("run GTK/GDK clipboard text helper")?;

    match output.status.code() {
        Some(0) => String::from_utf8(output.stdout)
            .context("GTK/GDK clipboard text helper returned non-UTF-8 text")
            .map(Some),
        Some(2) => Ok(None),
        code => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            let status = code
                .map(|code| format!("exit status {code}"))
                .unwrap_or_else(|| "terminated by signal".to_owned());
            if detail.is_empty() {
                bail!("GTK/GDK clipboard text helper failed with {status}");
            }
            bail!("GTK/GDK clipboard text helper failed with {status}: {detail}");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn read_gtk_clipboard_text() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(target_os = "linux")]
fn save_gtk_clipboard_image_as_png(path: &Path) -> Result<bool> {
    let output = process::Command::new("/usr/bin/python3")
        .arg("-c")
        .arg(GTK_CLIPBOARD_IMAGE_HELPER)
        .arg(path)
        .stdin(process::Stdio::null())
        .output()
        .context("run GTK/GDK clipboard image helper")?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(2) => Ok(false),
        code => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            let status = code
                .map(|code| format!("exit status {code}"))
                .unwrap_or_else(|| "terminated by signal".to_owned());
            if detail.is_empty() {
                bail!("GTK/GDK clipboard image helper failed with {status}");
            }
            bail!("GTK/GDK clipboard image helper failed with {status}: {detail}");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn save_gtk_clipboard_image_as_png(_path: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "linux")]
const GTK_CLIPBOARD_TEXT_HELPER: &str = r#"
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

ok, _ = Gtk.init_check([])
if not ok:
    print("GTK init failed", file=sys.stderr)
    sys.exit(3)

display = Gdk.Display.get_default()
if display is None:
    print("no GDK display available", file=sys.stderr)
    sys.exit(3)

clipboard = Gtk.Clipboard.get_default(display)
loop = GLib.MainLoop()
state = {"status": 3, "error": "clipboard text unavailable", "text": ""}

def done(clipboard, text, _data):
    try:
        if text is None:
            state["status"] = 2
            state["error"] = "clipboard does not contain text"
        else:
            state["status"] = 0
            state["text"] = text
            state["error"] = ""
    except Exception as exc:
        message = str(exc)
        state["error"] = message
        lowered = message.lower()
        if (
            "no compatible" in lowered
            or "not contain" in lowered
            or "not available" in lowered
            or "contents" in lowered
        ):
            state["status"] = 2
        else:
            state["status"] = 3
    finally:
        loop.quit()

def timeout():
    state["status"] = 3
    state["error"] = "GDK clipboard text read timed out"
    loop.quit()
    return False

GLib.timeout_add(3000, timeout)
clipboard.request_text(done, None)
loop.run()

if state["status"] == 0:
    print(state["text"], end="")
else:
    print(state["error"], file=sys.stderr)
sys.exit(state["status"])
"#;

#[cfg(target_os = "linux")]
const GTK_CLIPBOARD_IMAGE_HELPER: &str = r#"
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, GLib

ok, _ = Gtk.init_check([])
if not ok:
    print("GTK init failed", file=sys.stderr)
    sys.exit(3)

path = sys.argv[1]
display = Gdk.Display.get_default()
if display is None:
    print("no GDK display available", file=sys.stderr)
    sys.exit(3)

clipboard = Gtk.Clipboard.get_default(display)
loop = GLib.MainLoop()
state = {"status": 3, "error": "clipboard image unavailable"}

def done(clipboard, pixbuf, _data):
    try:
        if pixbuf is None:
            state["status"] = 2
            state["error"] = "clipboard does not contain an image"
        else:
            pixbuf.savev(path, "png", [], [])
            state["status"] = 0
            state["error"] = ""
    except Exception as exc:
        message = str(exc)
        state["error"] = message
        lowered = message.lower()
        if (
            "no compatible" in lowered
            or "not contain" in lowered
            or "not available" in lowered
            or "contents" in lowered
        ):
            state["status"] = 2
        else:
            state["status"] = 3
    finally:
        loop.quit()

def timeout():
    state["status"] = 3
    state["error"] = "GDK clipboard image read timed out"
    loop.quit()
    return False

GLib.timeout_add(3000, timeout)
clipboard.request_image(done, None)
loop.run()

if state["status"] != 0:
    print(state["error"], file=sys.stderr)
sys.exit(state["status"])
"#;

impl SystemClipboardSink {
    fn clipboard_mut(&mut self) -> Result<&mut arboard::Clipboard> {
        let Some(clipboard) = &mut self.clipboard else {
            let reason = self.init_error.as_deref().unwrap_or("unknown init error");
            bail!("system clipboard unavailable: {reason}");
        };

        Ok(clipboard)
    }
}

impl ClipboardSink for MemoryClipboardSink {
    fn set_text(&mut self, selection: ClipboardSelection, text: &str) -> Result<()> {
        match selection {
            ClipboardSelection::Clipboard => {
                self.clipboard_text = text.to_owned();
                self.clipboard_writes += 1;
            }
            ClipboardSelection::Primary => {
                self.primary_text = text.to_owned();
                self.primary_writes += 1;
            }
        }
        Ok(())
    }

    fn get_text(&mut self, selection: ClipboardSelection) -> Result<String> {
        Ok(match selection {
            ClipboardSelection::Clipboard => self.clipboard_text.clone(),
            ClipboardSelection::Primary => self.primary_text.clone(),
        })
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn set_system_primary_text(clipboard: &mut arboard::Clipboard, text: &str) -> Result<()> {
    use arboard::{LinuxClipboardKind, SetExtLinux};

    clipboard
        .set()
        .clipboard(LinuxClipboardKind::Primary)
        .text(text.to_owned())
        .context("failed to write selected text to Linux primary selection")
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
fn set_system_primary_text(_clipboard: &mut arboard::Clipboard, _text: &str) -> Result<()> {
    bail!("primary selection is only supported on Linux-like Unix targets")
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn get_system_primary_text(clipboard: &mut arboard::Clipboard) -> Result<String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    clipboard
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .context("failed to read text from Linux primary selection")
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
fn get_system_primary_text(_clipboard: &mut arboard::Clipboard) -> Result<String> {
    bail!("primary selection is only supported on Linux-like Unix targets")
}

fn copy_selection_to_clipboard(
    terminal: &BasicTerminal,
    clipboard: &mut dyn ClipboardSink,
) -> Result<bool> {
    copy_selection_to_target(terminal, clipboard, ClipboardSelection::Clipboard)
}

fn copy_selection_to_clipboard_and_clear(
    terminal: &mut BasicTerminal,
    clipboard: &mut dyn ClipboardSink,
) -> Result<bool> {
    let had_selection = terminal.selected_text().is_some();
    let copied = copy_selection_to_clipboard(terminal, clipboard)?;
    if had_selection {
        terminal.set_selection(None);
    }
    Ok(copied)
}

fn copy_selection_to_primary(
    terminal: &BasicTerminal,
    clipboard: &mut dyn ClipboardSink,
) -> Result<bool> {
    copy_selection_to_target(terminal, clipboard, ClipboardSelection::Primary)
}

fn copy_selection_to_target(
    terminal: &BasicTerminal,
    clipboard: &mut dyn ClipboardSink,
    selection: ClipboardSelection,
) -> Result<bool> {
    let Some(text) = terminal.selected_text().filter(|text| !text.is_empty()) else {
        return Ok(false);
    };

    clipboard.set_text(selection, &text)?;
    Ok(true)
}

fn copy_command_block_to_clipboard(
    terminal: &BasicTerminal,
    shell_integration: &ShellIntegrationState,
    target: CommandBlockCopyTarget,
    clipboard: &mut dyn ClipboardSink,
) -> Result<bool> {
    let Some(text) = selected_command_block_copy_text(terminal, shell_integration, target)
        .filter(|text| !text.is_empty())
    else {
        return Ok(false);
    };

    clipboard.set_text(ClipboardSelection::Clipboard, &text)?;
    Ok(true)
}

fn clipboard_paste_text(clipboard: &mut dyn ClipboardSink) -> Result<Option<String>> {
    selection_paste_text(clipboard, ClipboardSelection::Clipboard)
}

fn selection_paste_text(
    clipboard: &mut dyn ClipboardSink,
    selection: ClipboardSelection,
) -> Result<Option<String>> {
    let text = clipboard.get_text(selection)?;
    Ok((!text.is_empty()).then_some(text))
}

fn apply_terminal_host_actions(
    actions: Vec<TerminalHostAction>,
    policy: Osc52ClipboardPolicy,
    clipboard: &mut dyn ClipboardSink,
    shell_integration: &mut ShellIntegrationState,
    mut write_reply: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    apply_terminal_host_actions_at_ms(
        actions,
        policy,
        clipboard,
        shell_integration,
        None,
        &mut write_reply,
    )
}

fn apply_terminal_host_actions_at_ms(
    actions: Vec<TerminalHostAction>,
    policy: Osc52ClipboardPolicy,
    clipboard: &mut dyn ClipboardSink,
    shell_integration: &mut ShellIntegrationState,
    observed_at_ms: Option<u64>,
    mut write_reply: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    for action in actions {
        match action {
            TerminalHostAction::ClipboardWrite(write) => {
                apply_osc52_clipboard_write(write, policy, clipboard)?;
            }
            TerminalHostAction::TerminalReply(reply) => {
                write_reply(&reply.bytes)?;
            }
            TerminalHostAction::ShellIntegration(event) => {
                if let Some(observed_at_ms) = observed_at_ms {
                    shell_integration.apply_event_at_ms(event, observed_at_ms);
                } else {
                    shell_integration.apply_event(event);
                }
            }
            TerminalHostAction::CurrentDirectory(directory) => {
                shell_integration.apply_current_directory(directory);
            }
            TerminalHostAction::Bell => {}
        }
    }

    Ok(())
}

fn apply_osc52_clipboard_write(
    write: TerminalClipboardWrite,
    policy: Osc52ClipboardPolicy,
    clipboard: &mut dyn ClipboardSink,
) -> Result<()> {
    match policy {
        Osc52ClipboardPolicy::Disabled => Ok(()),
        Osc52ClipboardPolicy::Confirm => {
            bail!("OSC 52 clipboard confirmation is not implemented")
        }
        Osc52ClipboardPolicy::Allow => {
            let selection = clipboard_selection_for_terminal_selection(write.selection)?;
            clipboard.set_text(selection, &write.text)
        }
    }
}

fn clipboard_selection_for_terminal_selection(
    selection: TerminalClipboardSelection,
) -> Result<ClipboardSelection> {
    match selection {
        TerminalClipboardSelection::Clipboard => Ok(ClipboardSelection::Clipboard),
        TerminalClipboardSelection::Primary => {
            if cfg!(all(
                unix,
                not(any(
                    target_os = "macos",
                    target_os = "android",
                    target_os = "emscripten"
                ))
            )) {
                Ok(ClipboardSelection::Primary)
            } else {
                bail!("OSC 52 primary selection is only supported on Linux-like Unix targets")
            }
        }
    }
}

fn paste_clipboard_to_input(
    clipboard: &mut dyn ClipboardSink,
    bracketed_paste: bool,
    mut write_input: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let text = match clipboard_paste_text(clipboard) {
        Ok(Some(text)) => text,
        Ok(None) => return Ok(false),
        Err(text_err) => {
            tracing::info!(
                target: "witty_app::clipboard",
                error = %text_err,
                "clipboard text paste failed; trying clipboard image paste"
            );
            let Some(path) = save_clipboard_image_to_temp_file(clipboard)? else {
                return Err(text_err);
            };
            path.to_string_lossy().into_owned()
        }
    };

    paste_text_to_input(&text, bracketed_paste, &mut write_input)
}

fn paste_selection_to_input(
    clipboard: &mut dyn ClipboardSink,
    selection: ClipboardSelection,
    bracketed_paste: bool,
    mut write_input: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let Some(text) = selection_paste_text(clipboard, selection)? else {
        return Ok(false);
    };

    paste_text_to_input(&text, bracketed_paste, &mut write_input)
}

fn paste_text_to_input(
    text: &str,
    bracketed_paste: bool,
    mut write_input: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let payload = paste_payload(text, bracketed_paste);
    write_input(&payload).context("failed to write clipboard text to terminal")?;
    Ok(true)
}

fn save_clipboard_image_to_temp_file(clipboard: &mut dyn ClipboardSink) -> Result<Option<PathBuf>> {
    let path = clipboard_image_temp_path();
    if !clipboard.save_image_as_png(ClipboardSelection::Clipboard, &path)? {
        return Ok(None);
    }
    Ok(Some(path))
}

fn clipboard_image_temp_path() -> PathBuf {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "witty-clipboard-image-{}-{now_ms}.png",
        process::id()
    ))
}

fn write_clipboard_image_png(image: &ClipboardImage, path: &Path) -> Result<()> {
    let expected_len = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .context("clipboard image dimensions overflow")?;
    if image.rgba.len() != expected_len {
        bail!(
            "clipboard image RGBA payload has {} bytes, expected {} for {}x{}",
            image.rgba.len(),
            expected_len,
            image.width,
            image.height
        );
    }
    let width = u32::try_from(image.width).context("clipboard image width is too large")?;
    let height = u32::try_from(image.height).context("clipboard image height is too large")?;
    image::save_buffer_with_format(
        path,
        &image.rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .with_context(|| format!("write clipboard image PNG {}", path.display()))
}

fn apply_native_ime_event(composition: &mut ImeComposition, event: Ime) -> NativeImeEventResult {
    match event {
        Ime::Enabled => {
            let changed = !composition.is_enabled();
            composition.enable();
            NativeImeEventResult {
                changed,
                committed_text: None,
            }
        }
        Ime::Preedit(text, caret) => {
            let before = (
                composition.is_enabled(),
                composition.preedit().to_owned(),
                composition.caret(),
            );
            composition.set_preedit(text, caret);
            let after = (
                composition.is_enabled(),
                composition.preedit().to_owned(),
                composition.caret(),
            );
            NativeImeEventResult {
                changed: before != after,
                committed_text: None,
            }
        }
        Ime::Commit(text) => {
            let was_active = composition.is_active();
            let committed_text = composition.commit_text(text);
            NativeImeEventResult {
                changed: was_active || committed_text.is_some(),
                committed_text,
            }
        }
        Ime::Disabled => {
            let changed = composition.is_enabled() || composition.is_active();
            composition.disable();
            NativeImeEventResult {
                changed,
                committed_text: None,
            }
        }
    }
}

fn has_command(commands: &[CommandRegistration], command_id: &str) -> bool {
    commands.iter().any(|command| command.id == command_id)
}

fn native_window_command_registrations() -> Vec<CommandRegistration> {
    vec![CommandRegistration {
        id: WITTY_NEW_LOCAL_SHELL_COMMAND_ID.to_owned(),
        title: "New Session".to_owned(),
        source_plugin: "builtin".to_owned(),
    }]
}

fn apply_empty_session_welcome_overlay(
    frame: &mut FramePlan,
    visible: bool,
    notice: Option<&str>,
    hovered: Option<NativeEmptySessionWelcomeHit>,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !visible {
        return;
    }

    frame.backgrounds.clear();
    frame.overlay_backgrounds.clear();
    frame.glyphs.clear();
    frame.cursor = None;
    frame.selection.clear();
    frame.search_highlights.clear();
    frame.hyperlink_hover.clear();
    frame.hyperlink_underlines.clear();
    frame.text_decorations.clear();
    frame.ime_preedit.clear();

    if grid_size.rows == 0 || grid_size.cols == 0 {
        return;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: cell_origin(CellPoint::new(0, 0), metrics),
        size: PixelSize {
            width: f32::from(grid_size.cols) * metrics.cell.width,
            height: f32::from(grid_size.rows) * metrics.cell.height,
        },
        color: Rgba::rgb(10, 14, 18),
    });

    let Some(panel) = empty_session_welcome_panel(grid_size) else {
        return;
    };

    push_centered_palette_text(
        frame,
        panel,
        metrics,
        0,
        "No active shell",
        Rgba::rgb(232, 238, 236),
    );
    push_centered_palette_text(
        frame,
        panel,
        metrics,
        1,
        "Last shell exited.",
        Rgba::rgb(154, 168, 172),
    );
    if let Some(notice) = empty_session_notice_text(notice) {
        push_centered_palette_text(frame, panel, metrics, 2, &notice, Rgba::rgb(238, 190, 148));
    }

    let button_row = empty_session_welcome_button_row(panel);
    let content_width = panel.cols.saturating_sub(2);
    if content_width == 0 {
        return;
    }
    for span in empty_session_welcome_button_spans(content_width) {
        if hovered.is_some_and(|hit| hit.target == span.target) {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(
                        panel.start.row + button_row,
                        panel.start.col + 1 + span.start_col,
                    ),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(span.end_col.saturating_sub(span.start_col))
                        * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: empty_session_welcome_hover_color(span.target),
            });
        }
    }
    push_palette_text(
        frame,
        panel,
        metrics,
        button_row,
        1,
        &empty_session_welcome_buttons_text(content_width),
        Rgba::rgb(214, 228, 226),
    );
}

fn empty_session_welcome_panel(grid_size: GridSize) -> Option<PalettePanel> {
    if grid_size.rows < 3 || grid_size.cols < 12 {
        return None;
    }

    let panel_cols = if grid_size.cols > 72 {
        64
    } else {
        grid_size.cols.saturating_sub(4).max(1)
    };
    let panel_rows = 4.min(grid_size.rows);
    Some(PalettePanel {
        start: CellPoint::new(
            grid_size.rows.saturating_sub(panel_rows) / 2,
            grid_size.cols.saturating_sub(panel_cols) / 2,
        ),
        cols: panel_cols,
        rows: panel_rows,
        item_rows: 0,
    })
}

fn empty_session_notice_text(notice: Option<&str>) -> Option<String> {
    let notice = notice?;
    let text = notice.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

fn empty_session_welcome_button_row(panel: PalettePanel) -> u16 {
    panel.rows.saturating_sub(1)
}

fn empty_session_welcome_buttons_text(width: u16) -> String {
    let spans = empty_session_welcome_button_spans(width);
    if spans.is_empty() {
        return String::new();
    }

    let mut text = String::new();
    let mut col = 0u16;
    for span in spans {
        if span.start_col > col {
            text.push_str(&" ".repeat(usize::from(span.start_col - col)));
        }
        text.push_str(empty_session_welcome_button_text(span.target).as_str());
        col = span.end_col;
    }
    truncate_cells(&text, width)
}

fn empty_session_welcome_button_spans(width: u16) -> Vec<NativeEmptySessionWelcomeButtonSpan> {
    let targets = [
        NativeEmptySessionWelcomeTarget::NewLocalShell,
        NativeEmptySessionWelcomeTarget::CommandPalette,
    ];
    let mut count = targets.len();
    while count > 0 {
        let button_width = targets
            .iter()
            .take(count)
            .map(|target| text_cell_width(&empty_session_welcome_button_text(*target)))
            .fold(0u16, |total, width| total.saturating_add(width));
        let total_width = button_width.saturating_add(count.saturating_sub(1) as u16);
        if total_width <= width {
            let mut spans = Vec::with_capacity(count);
            let mut col = width.saturating_sub(total_width) / 2;
            for target in targets.iter().take(count) {
                let button_width = text_cell_width(&empty_session_welcome_button_text(*target));
                spans.push(NativeEmptySessionWelcomeButtonSpan {
                    target: *target,
                    start_col: col,
                    end_col: col.saturating_add(button_width),
                });
                col = col.saturating_add(button_width).saturating_add(1);
            }
            return spans;
        }
        count -= 1;
    }
    Vec::new()
}

fn empty_session_welcome_button_text(target: NativeEmptySessionWelcomeTarget) -> String {
    match target {
        NativeEmptySessionWelcomeTarget::NewLocalShell => "[New Session]".to_owned(),
        NativeEmptySessionWelcomeTarget::CommandPalette => "[Command Palette]".to_owned(),
    }
}

fn empty_session_welcome_hit_for_position(
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeEmptySessionWelcomeHit> {
    let point = pixel_point_for_position(position)?;
    empty_session_welcome_hit_for_pixel_point(point, metrics, grid_size)
}

fn empty_session_welcome_hit_for_pixel_point(
    point: PixelPoint,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeEmptySessionWelcomeHit> {
    let panel = empty_session_welcome_panel(grid_size)?;
    let cell = cell_point_for_pixel_point(point, metrics, grid_size);
    if cell.row
        != panel
            .start
            .row
            .saturating_add(empty_session_welcome_button_row(panel))
        || cell.col <= panel.start.col
        || cell.col >= panel.start.col.saturating_add(panel.cols).saturating_sub(1)
    {
        return None;
    }

    let text_col = cell.col.saturating_sub(panel.start.col).saturating_sub(1);
    empty_session_welcome_button_spans(panel.cols.saturating_sub(2))
        .into_iter()
        .find(|span| text_col >= span.start_col && text_col < span.end_col)
        .map(|span| NativeEmptySessionWelcomeHit {
            target: span.target,
        })
}

fn empty_session_welcome_hover_color(target: NativeEmptySessionWelcomeTarget) -> Rgba {
    match target {
        NativeEmptySessionWelcomeTarget::NewLocalShell => Rgba::with_alpha(48, 108, 72, 170),
        NativeEmptySessionWelcomeTarget::CommandPalette => Rgba::with_alpha(48, 82, 132, 170),
    }
}

fn apply_search_bar_overlay(
    frame: &mut FramePlan,
    search: &TerminalSearch,
    ime: Option<&ImeComposition>,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !search.is_open() || grid_size.rows == 0 || grid_size.cols == 0 {
        return;
    }

    let panel = search_bar_panel(grid_size);
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .selection
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, panel_origin, panel_size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(18, 24, 30),
    });

    push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        &search_bar_text(search, ime, panel.cols.saturating_sub(2)),
        Rgba::rgb(232, 238, 226),
    );
}

fn apply_about_status_overlay(
    frame: &mut FramePlan,
    visible: bool,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !visible || grid_size.rows == 0 || grid_size.cols == 0 {
        return;
    }

    let panel = search_bar_panel(grid_size);
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .selection
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, panel_origin, panel_size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(18, 24, 30),
    });

    push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        &about_status_text(panel.cols.saturating_sub(2)),
        Rgba::rgb(232, 238, 226),
    );
}

fn about_status_text(width: u16) -> String {
    truncate_cells(
        &format!(
            "Witty {} - Rust/wgpu terminal prototype | Ctrl+Shift+P Commands | Ctrl+Shift+F Search | F11 Fullscreen",
            env!("CARGO_PKG_VERSION")
        ),
        width,
    )
}

fn search_bar_panel(grid_size: GridSize) -> PalettePanel {
    PalettePanel {
        start: CellPoint::new(grid_size.rows.saturating_sub(1), 0),
        cols: grid_size.cols,
        rows: 1,
        item_rows: 0,
    }
}

fn search_bar_text(search: &TerminalSearch, ime: Option<&ImeComposition>, width: u16) -> String {
    let query = search_display_query(search, ime);
    let left = format!("Find: {} {}", query, search_options_label(search));
    let status = search_count_label(search);
    let width = usize::from(width);
    let left_width = usize::from(text_cell_width(&left));
    let status_width = usize::from(text_cell_width(&status));

    if left_width + status_width < width {
        let spacer = " ".repeat(width - left_width - status_width);
        format!("{left}{spacer}{status}")
    } else {
        truncate_cells(&format!("{left}  {status}"), width as u16)
    }
}

fn search_display_query(search: &TerminalSearch, ime: Option<&ImeComposition>) -> String {
    let mut query = search.query().to_owned();
    if let Some(ime) = ime.filter(|ime| ime.is_active()) {
        query.push_str(ime.preedit());
    }
    query
}

fn search_count_label(search: &TerminalSearch) -> String {
    if let Some(error) = search.error_text() {
        return error;
    }

    if search.query().is_empty() {
        return "0/0".to_owned();
    }

    if search.match_count() == 0 {
        return "No results".to_owned();
    }

    let active = search.active_index().map(|index| index + 1).unwrap_or(0);
    format!("{active}/{}", search.match_count())
}

fn search_options_label(search: &TerminalSearch) -> String {
    let options = search.options();
    let case = if options.case_sensitive { "Aa" } else { "aa" };
    let pattern = if options.regex { ".*" } else { "lit" };
    let scope = if options.whole_word { "word" } else { "part" };
    let normalization = if options.normalize_nfc { "nfc" } else { "raw" };
    format!("[{case} {pattern} {scope} {normalization}]")
}

fn apply_command_palette_overlay(
    frame: &mut FramePlan,
    palette: &CommandPalette,
    ime: Option<&ImeComposition>,
    commands: &[CommandRegistration],
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !palette.is_open() {
        return;
    }

    let Some(panel) = palette_panel(grid_size, palette.filtered_count()) else {
        return;
    };
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .hyperlink_hover
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .hyperlink_underlines
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .text_decorations
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .ime_preedit
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame.selection.clear();
    frame.cursor = None;

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(18, 22, 28),
    });

    push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        &palette_title_with_ime(palette.query(), ime, panel.cols.saturating_sub(2)),
        Rgba::rgb(220, 230, 235),
    );

    let items = palette.visible_items(panel.item_rows);
    if items.is_empty() && panel.item_rows > 0 {
        push_palette_text(
            frame,
            panel,
            metrics,
            1,
            1,
            "No matching commands",
            Rgba::rgb(150, 160, 165),
        );
        return;
    }

    for (offset, item) in items.iter().enumerate() {
        let row = offset as u16 + 1;
        if item.selected {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: Rgba::rgb(42, 76, 118),
            });
        }

        let text = palette_item_text(
            item.command,
            item.selected,
            commands,
            panel.cols.saturating_sub(2),
        );
        push_palette_text(
            frame,
            panel,
            metrics,
            row,
            1,
            &text,
            Rgba::rgb(238, 242, 245),
        );
    }
}

fn apply_session_switcher_overlay(
    frame: &mut FramePlan,
    switcher: &NativeSessionSwitcher,
    rows: &[NativeSessionTabRow],
    label_style: NativeSessionTabLabelStyle,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !switcher.is_open() || rows.len() < 2 {
        return;
    }

    let Some(panel) = session_switcher_panel(grid_size, rows.len()) else {
        return;
    };
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .hyperlink_hover
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .hyperlink_underlines
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .text_decorations
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .ime_preedit
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame.selection.clear();
    frame.cursor = None;

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(18, 22, 28),
    });

    push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        "Switch Session",
        Rgba::rgb(220, 230, 235),
    );

    let selected_index = switcher
        .selected_session_id
        .and_then(|id| rows.iter().position(|row| row.session_id == id))
        .or_else(|| rows.iter().position(|row| row.active))
        .unwrap_or(0);
    let visible_count = panel.item_rows.min(rows.len());
    let start_index = selected_index
        .saturating_add(1)
        .saturating_sub(visible_count);

    for (offset, row) in rows
        .iter()
        .enumerate()
        .skip(start_index)
        .take(visible_count)
    {
        let row_offset = (offset - start_index) as u16 + 1;
        let selected = offset == selected_index;
        if selected {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row_offset, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: Rgba::rgb(42, 76, 118),
            });
        }
        push_palette_text(
            frame,
            panel,
            metrics,
            row_offset,
            1,
            &session_switcher_row_text(
                row,
                offset,
                selected,
                label_style,
                panel.cols.saturating_sub(2),
            ),
            Rgba::rgb(238, 242, 245),
        );
    }
}

fn apply_profile_action_overlay(
    frame: &mut FramePlan,
    snapshot: &NativeProfileActionSnapshot,
    hovered: Option<NativeProfileActionOverlayHit>,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    let body_rows = profile_action_overlay_body_row_count(snapshot);
    if body_rows == 0 {
        return;
    }

    let Some(panel) = profile_action_panel(grid_size, body_rows) else {
        return;
    };
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    frame
        .selection
        .retain(|rect| !rect_origin_inside(rect, panel_origin, panel_size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, panel_origin, panel_size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(20, 28, 34),
    });

    push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        "Profile Actions",
        Rgba::rgb(226, 234, 236),
    );

    let visible_items = panel.item_rows.min(body_rows);
    let visible_action_items = visible_items.min(snapshot.display_rows.len());
    for (offset, row) in snapshot
        .display_rows
        .iter()
        .take(visible_action_items)
        .enumerate()
    {
        if let Some(hit) = hovered.filter(|hit| hit.row_index == offset && hit.key == row.key) {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + offset as u16 + 1, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: profile_action_hover_color(hit.target),
            });
        }
        push_palette_text(
            frame,
            panel,
            metrics,
            offset as u16 + 1,
            1,
            &profile_action_overlay_row_text(row, panel.cols.saturating_sub(2)),
            profile_action_status_color(row.status),
        );
    }

    let start_success_row_index = snapshot.display_rows.len();
    let mut visible_start_success_items = 0;
    if let Some(success) = snapshot
        .start_success
        .as_ref()
        .filter(|_| visible_items > visible_action_items)
    {
        visible_start_success_items = 1;
        let row_offset = visible_action_items + 1;
        if let Some(hit) =
            hovered.filter(|hit| hit.row_index == start_success_row_index && hit.key == success.key)
        {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row_offset as u16, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: profile_action_hover_color(hit.target),
            });
        }
        push_palette_text(
            frame,
            panel,
            metrics,
            row_offset as u16,
            1,
            &profile_start_success_overlay_row_text(success, panel.cols.saturating_sub(2)),
            profile_start_success_status_color(),
        );
    }

    let start_failure_row_index =
        snapshot.display_rows.len() + profile_action_start_success_row_count(snapshot);
    let mut visible_start_failure_items = 0;
    if let Some(failure) = snapshot
        .start_failure
        .as_ref()
        .filter(|_| visible_items > visible_action_items + visible_start_success_items)
    {
        visible_start_failure_items = 1;
        let row_offset = visible_action_items + visible_start_success_items + 1;
        if let Some(hit) =
            hovered.filter(|hit| hit.row_index == start_failure_row_index && hit.key == failure.key)
        {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row_offset as u16, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: profile_action_hover_color(hit.target),
            });
        }
        push_palette_text(
            frame,
            panel,
            metrics,
            row_offset as u16,
            1,
            &profile_start_failure_overlay_row_text(failure, panel.cols.saturating_sub(2)),
            profile_start_failure_status_color(),
        );
    }

    let visible_option_items = visible_items.saturating_sub(
        visible_action_items + visible_start_success_items + visible_start_failure_items,
    );
    for (offset, option) in snapshot
        .picker_options
        .iter()
        .take(visible_option_items)
        .enumerate()
    {
        let overlay_row_index = snapshot.display_rows.len()
            + profile_action_start_success_row_count(snapshot)
            + profile_action_start_failure_row_count(snapshot)
            + offset;
        let row_offset = visible_action_items
            + visible_start_success_items
            + visible_start_failure_items
            + offset
            + 1;
        if let Some(hit) = hovered
            .filter(|hit| hit.row_index == overlay_row_index && hit.key == option.request_key)
        {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row_offset as u16, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: profile_action_hover_color(hit.target),
            });
        }
        push_palette_text(
            frame,
            panel,
            metrics,
            row_offset as u16,
            1,
            &profile_picker_option_overlay_row_text(option, panel.cols.saturating_sub(2)),
            profile_picker_option_status_color(option.status),
        );
    }

    let hidden = body_rows.saturating_sub(visible_items);
    if hidden > 0 {
        push_palette_text(
            frame,
            panel,
            metrics,
            panel.rows.saturating_sub(1),
            1,
            &truncate_cells(&format!("... {hidden} more"), panel.cols.saturating_sub(2)),
            Rgba::rgb(162, 174, 178),
        );
    }
}

fn apply_native_session_tab_strip_overlay(
    frame: &mut FramePlan,
    rows: &[NativeSessionTabRow],
    hovered: Option<NativeSessionTabStripHit>,
    notice: Option<NativeSessionTabStripNotice>,
    position: NativeSessionTabPosition,
    label_style: NativeSessionTabLabelStyle,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if rows.is_empty() || grid_size.rows == 0 || grid_size.cols < 8 {
        return;
    }

    let Some(tab_row) = native_session_tab_strip_row(position, grid_size) else {
        return;
    };

    let origin = cell_origin(CellPoint::new(tab_row, 0), metrics);
    let size = PixelSize {
        width: f32::from(grid_size.cols) * metrics.cell.width,
        height: metrics.cell.height,
    };
    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, origin, size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, origin, size));
    frame
        .selection
        .retain(|rect| !rect_origin_inside(rect, origin, size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, origin, size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin,
        size,
        color: Rgba::rgb(18, 24, 28),
    });
    let width = grid_size.cols.saturating_sub(2);
    let action_width = native_session_tab_strip_action_width(rows, notice, width);
    for span in native_session_tab_strip_spans(rows, action_width, label_style) {
        if Some(span.hit) != hovered {
            continue;
        }
        frame.overlay_backgrounds.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(tab_row, 1 + span.start_col), metrics),
            size: PixelSize {
                width: f32::from(span.end_col.saturating_sub(span.start_col)) * metrics.cell.width,
                height: metrics.cell.height,
            },
            color: native_session_tab_hover_color(span.hit.target),
        });
    }
    frame.glyphs.push(GlyphBatchItem {
        origin: cell_origin(CellPoint::new(tab_row, 1), metrics),
        text: native_session_tab_strip_text_with_notice(
            rows,
            notice,
            grid_size.cols.saturating_sub(2),
            label_style,
        ),
        color: Rgba::rgb(228, 235, 236),
        style_flags: CellFlags::default(),
    });
}

fn native_session_tab_strip_hit_for_position(
    rows: &[NativeSessionTabRow],
    notice: Option<NativeSessionTabStripNotice>,
    position_kind: NativeSessionTabPosition,
    label_style: NativeSessionTabLabelStyle,
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeSessionTabStripHit> {
    if rows.is_empty() || grid_size.rows == 0 || grid_size.cols < 8 {
        return None;
    }
    let tab_row = native_session_tab_strip_row(position_kind, grid_size)?;

    let point = pixel_point_for_position(position)?;
    let cell = cell_point_for_pixel_point(point, metrics, grid_size);
    if cell.row != tab_row || cell.col == 0 {
        return None;
    }

    let text_col = cell.col.saturating_sub(1);
    let width = grid_size.cols.saturating_sub(2);
    let action_width = native_session_tab_strip_action_width(rows, notice, width);
    native_session_tab_strip_spans(rows, action_width, label_style)
        .into_iter()
        .find(|span| text_col >= span.start_col && text_col < span.end_col)
        .map(|span| span.hit)
}

fn native_session_tab_strip_row(
    position: NativeSessionTabPosition,
    grid_size: GridSize,
) -> Option<u16> {
    match position {
        NativeSessionTabPosition::Top => Some(0),
        NativeSessionTabPosition::Bottom => grid_size.rows.checked_sub(1),
    }
}

fn native_session_tab_strip_action_width(
    rows: &[NativeSessionTabRow],
    notice: Option<NativeSessionTabStripNotice>,
    width: u16,
) -> u16 {
    let Some(notice) = notice else {
        return width;
    };
    if rows.is_empty() {
        return 0;
    }

    let notice_width = text_cell_width(native_session_tab_strip_notice_text(notice));
    if notice_width >= width {
        return 0;
    }

    let separator_width = text_cell_width("  ");
    width
        .checked_sub(notice_width)
        .and_then(|remaining| remaining.checked_sub(separator_width))
        .unwrap_or(0)
}

fn native_session_tab_strip_spans(
    rows: &[NativeSessionTabRow],
    width: u16,
    label_style: NativeSessionTabLabelStyle,
) -> Vec<NativeSessionTabStripSpan> {
    let mut spans = Vec::new();
    let mut col = 0u16;
    for (row_index, row) in rows.iter().enumerate() {
        if col >= width {
            break;
        }
        let label_width = text_cell_width(&native_session_tab_label(row, row_index, label_style));
        let close_width = text_cell_width(native_session_tab_close_label());
        let summary_width = label_width.saturating_add(1).saturating_add(close_width);
        let select_end_col = col.saturating_add(label_width).min(width);
        if select_end_col > col {
            spans.push(NativeSessionTabStripSpan {
                hit: NativeSessionTabStripHit {
                    session_id: row.session_id,
                    row_index,
                    target: NativeSessionTabStripTarget::Select,
                },
                start_col: col,
                end_col: select_end_col,
            });
        }
        let close_start_col = col.saturating_add(label_width).saturating_add(1);
        let close_end_col = close_start_col.saturating_add(close_width);
        if close_end_col <= width {
            spans.push(NativeSessionTabStripSpan {
                hit: NativeSessionTabStripHit {
                    session_id: row.session_id,
                    row_index,
                    target: NativeSessionTabStripTarget::Close,
                },
                start_col: close_start_col,
                end_col: close_end_col,
            });
        }
        col = col.saturating_add(summary_width).saturating_add(2);
    }
    spans
}

fn native_session_tab_strip_text_with_notice(
    rows: &[NativeSessionTabRow],
    notice: Option<NativeSessionTabStripNotice>,
    width: u16,
    label_style: NativeSessionTabLabelStyle,
) -> String {
    let Some(notice) = notice else {
        let text = rows
            .iter()
            .enumerate()
            .map(|(row_index, row)| native_session_tab_summary(row, row_index, label_style))
            .collect::<Vec<_>>()
            .join("  ");
        return truncate_cells(&text, width);
    };

    let notice_text = native_session_tab_strip_notice_text(notice);
    if rows.is_empty() {
        return truncate_cells(notice_text, width);
    }

    let text = rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| native_session_tab_summary(row, row_index, label_style))
        .collect::<Vec<_>>()
        .join("  ");
    let notice_width = text_cell_width(notice_text);
    if notice_width >= width {
        return truncate_cells(notice_text, width);
    }

    let separator = "  ";
    let separator_width = text_cell_width(separator);
    let Some(text_width) = width
        .checked_sub(notice_width)
        .and_then(|remaining| remaining.checked_sub(separator_width))
    else {
        return truncate_cells(notice_text, width);
    };

    format!(
        "{}{}{}",
        truncate_cells(&text, text_width),
        separator,
        notice_text
    )
}

fn native_session_tab_summary(
    row: &NativeSessionTabRow,
    row_index: usize,
    label_style: NativeSessionTabLabelStyle,
) -> String {
    format!(
        "{} {}",
        native_session_tab_label(row, row_index, label_style),
        native_session_tab_close_label()
    )
}

fn native_session_tab_label(
    row: &NativeSessionTabRow,
    row_index: usize,
    label_style: NativeSessionTabLabelStyle,
) -> String {
    let marker = if row.active { "*" } else { "" };
    match label_style {
        NativeSessionTabLabelStyle::Index => format!("{marker}{row_index}"),
        NativeSessionTabLabelStyle::Profile => format!("{marker}{}", row.profile_id),
        NativeSessionTabLabelStyle::IndexProfile => {
            format!("{marker}{row_index}:{}", row.profile_id)
        }
    }
}

fn native_session_tab_close_label() -> &'static str {
    "x"
}

fn native_session_tab_strip_notice_text(notice: NativeSessionTabStripNotice) -> &'static str {
    match notice {
        NativeSessionTabStripNotice::LastActiveCloseBlocked => "[close blocked: last active]",
    }
}

fn native_session_tab_hover_color(target: NativeSessionTabStripTarget) -> Rgba {
    match target {
        NativeSessionTabStripTarget::Select => Rgba::rgb(42, 62, 70),
        NativeSessionTabStripTarget::Close => Rgba::rgb(98, 48, 48),
    }
}

fn offset_frame_y(frame: &mut FramePlan, offset: f32) {
    if offset == 0.0 {
        return;
    }

    for rect in frame
        .backgrounds
        .iter_mut()
        .chain(frame.overlay_backgrounds.iter_mut())
        .chain(frame.selection.iter_mut())
        .chain(frame.search_highlights.iter_mut())
        .chain(frame.hyperlink_hover.iter_mut())
        .chain(frame.hyperlink_underlines.iter_mut())
        .chain(frame.text_decorations.iter_mut())
        .chain(frame.ime_preedit.iter_mut())
    {
        rect.origin.y += offset;
    }

    if let Some(cursor) = &mut frame.cursor {
        cursor.origin.y += offset;
    }

    for glyph in &mut frame.glyphs {
        glyph.origin.y += offset;
    }
}

fn apply_native_titlebar_overlay(
    frame: &mut FramePlan,
    overlay: NativeTitleBarOverlay<'_>,
    metrics: CellMetrics,
) {
    if !overlay.visible && !overlay.menu_open {
        return;
    }

    let scale_factor = overlay.scale_factor;
    let titlebar_rect = native_titlebar_rect(overlay.window_width, scale_factor);
    let mut clear_regions = vec![titlebar_rect];
    if overlay.menu_open {
        clear_regions.push(native_titlebar_menu_rect(scale_factor));
    }
    clear_frame_regions(frame, &clear_regions);

    if overlay.visible {
        frame.overlay_backgrounds.push(rect_item(
            titlebar_rect,
            if overlay.menu_open {
                Rgba::rgb(25, 31, 36)
            } else {
                Rgba::rgb(21, 27, 32)
            },
        ));
        frame.overlay_backgrounds.push(RectBatchItem {
            origin: PixelPoint {
                x: 0.0,
                y: native_titlebar_height(scale_factor)
                    - native_titlebar_separator_height(scale_factor),
            },
            size: PixelSize {
                width: overlay.window_width.max(1.0),
                height: native_titlebar_separator_height(scale_factor),
            },
            color: Rgba::rgb(47, 58, 64),
        });

        for button in native_titlebar_buttons() {
            draw_native_titlebar_button(frame, button, overlay, metrics);
        }
        draw_native_titlebar_title(frame, overlay, metrics);
    }

    if overlay.menu_open {
        draw_native_titlebar_menu(frame, overlay, metrics);
    }
}

fn clear_frame_regions(frame: &mut FramePlan, regions: &[NativeOverlayRect]) {
    frame.backgrounds.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.overlay_backgrounds.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.selection.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.search_highlights.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.hyperlink_hover.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.hyperlink_underlines.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.text_decorations.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.ime_preedit.retain(|rect| {
        !regions
            .iter()
            .any(|region| rect_origin_inside(rect, region.origin, region.size))
    });
    frame.glyphs.retain(|glyph| {
        !regions
            .iter()
            .any(|region| glyph_origin_inside(glyph, region.origin, region.size))
    });
    if frame.cursor.as_ref().is_some_and(|cursor| {
        regions
            .iter()
            .any(|region| rect_origin_inside(cursor, region.origin, region.size))
    }) {
        frame.cursor = None;
    }
}

fn draw_native_titlebar_button(
    frame: &mut FramePlan,
    button: NativeTitleBarButton,
    overlay: NativeTitleBarOverlay<'_>,
    metrics: CellMetrics,
) {
    let scale_factor = overlay.scale_factor;
    let Some(rect) = native_titlebar_button_rect(button, overlay.window_width, scale_factor) else {
        return;
    };
    let hovered = overlay
        .hovered_hit
        .is_some_and(|hit| hit == NativeTitleBarHit::Button(button));
    let button_color = match (button, hovered) {
        (NativeTitleBarButton::Close, true) => Rgba::rgb(150, 45, 48),
        (_, true) => Rgba::rgb(44, 55, 62),
        _ => Rgba::rgb(29, 37, 43),
    };
    frame
        .overlay_backgrounds
        .push(rect_item(rect, button_color));

    if button == NativeTitleBarButton::Menu {
        draw_menu_button_icon(frame, rect, hovered, scale_factor);
        return;
    }

    let label = match button {
        NativeTitleBarButton::Menu => "",
        NativeTitleBarButton::NewSession => "+",
        NativeTitleBarButton::Minimize => "-",
        NativeTitleBarButton::Maximize if overlay.maximized => "][",
        NativeTitleBarButton::Maximize => "[]",
        NativeTitleBarButton::Close => "x",
    };
    let label_width = text_cell_width(label) as f32 * metrics.cell.width;
    frame.glyphs.push(GlyphBatchItem {
        origin: PixelPoint {
            x: rect.origin.x + ((rect.size.width - label_width) * 0.5).max(0.0),
            y: ((native_titlebar_height(scale_factor) - metrics.cell.height) * 0.5).max(0.0),
        },
        text: label.to_owned(),
        color: if button == NativeTitleBarButton::Close && hovered {
            Rgba::rgb(255, 244, 244)
        } else {
            Rgba::rgb(206, 218, 222)
        },
        style_flags: CellFlags::default(),
    });
}

fn draw_menu_button_icon(
    frame: &mut FramePlan,
    rect: NativeOverlayRect,
    hovered: bool,
    scale_factor: f32,
) {
    let color = if hovered {
        Rgba::rgb(235, 242, 240)
    } else {
        Rgba::rgb(190, 205, 205)
    };
    let line_width = 13.0 * scale_factor;
    let line_height = native_titlebar_separator_height(scale_factor) * 2.0;
    let line_top = 8.0 * scale_factor;
    let line_gap = 5.0 * scale_factor;
    let line_x = rect.origin.x + (rect.size.width - line_width) * 0.5;
    for line_index in 0..3 {
        frame.overlay_backgrounds.push(RectBatchItem {
            origin: PixelPoint {
                x: line_x,
                y: rect.origin.y + line_top + line_index as f32 * line_gap,
            },
            size: PixelSize {
                width: line_width,
                height: line_height,
            },
            color,
        });
    }
}

fn draw_native_titlebar_title(
    frame: &mut FramePlan,
    overlay: NativeTitleBarOverlay<'_>,
    metrics: CellMetrics,
) {
    let scale_factor = overlay.scale_factor;
    let Some(new_session_rect) = native_titlebar_button_rect(
        NativeTitleBarButton::NewSession,
        overlay.window_width,
        scale_factor,
    ) else {
        return;
    };
    let Some(minimize_rect) = native_titlebar_button_rect(
        NativeTitleBarButton::Minimize,
        overlay.window_width,
        scale_factor,
    ) else {
        return;
    };
    let title_margin = 12.0 * scale_factor;
    let start_x = new_session_rect.origin.x + new_session_rect.size.width + title_margin;
    let end_x = (minimize_rect.origin.x - title_margin).max(start_x);
    let available_cells = ((end_x - start_x) / metrics.cell.width)
        .floor()
        .clamp(0.0, f32::from(u16::MAX)) as u16;
    if available_cells == 0 {
        return;
    }

    frame.glyphs.push(GlyphBatchItem {
        origin: PixelPoint {
            x: start_x,
            y: ((native_titlebar_height(scale_factor) - metrics.cell.height) * 0.5).max(0.0),
        },
        text: truncate_cells(overlay.title, available_cells),
        color: Rgba::rgb(218, 227, 225),
        style_flags: CellFlags::default(),
    });
}

fn draw_native_titlebar_menu(
    frame: &mut FramePlan,
    overlay: NativeTitleBarOverlay<'_>,
    metrics: CellMetrics,
) {
    let scale_factor = overlay.scale_factor;
    let menu_rect = native_titlebar_menu_rect(scale_factor);
    let menu_width = menu_rect.size.width.min(overlay.window_width.max(1.0));
    let menu_rect = NativeOverlayRect {
        size: PixelSize {
            width: menu_width,
            height: menu_rect.size.height,
        },
        ..menu_rect
    };
    frame
        .overlay_backgrounds
        .push(rect_item(menu_rect, Rgba::rgb(27, 35, 40)));
    frame.overlay_backgrounds.push(RectBatchItem {
        origin: menu_rect.origin,
        size: PixelSize {
            width: menu_rect.size.width,
            height: native_titlebar_separator_height(scale_factor),
        },
        color: Rgba::rgb(72, 90, 96),
    });

    for (index, item) in native_titlebar_menu_items().into_iter().enumerate() {
        let row_origin = PixelPoint {
            x: menu_rect.origin.x,
            y: menu_rect.origin.y + index as f32 * native_titlebar_menu_item_height(scale_factor),
        };
        let hovered = overlay
            .hovered_hit
            .is_some_and(|hit| hit == NativeTitleBarHit::MenuItem(item));
        if hovered {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: row_origin,
                size: PixelSize {
                    width: menu_rect.size.width,
                    height: native_titlebar_menu_item_height(scale_factor),
                },
                color: Rgba::rgb(46, 66, 74),
            });
        }
        frame.glyphs.push(GlyphBatchItem {
            origin: PixelPoint {
                x: row_origin.x + 12.0 * scale_factor,
                y: row_origin.y
                    + ((native_titlebar_menu_item_height(scale_factor) - metrics.cell.height)
                        * 0.5)
                        .max(0.0),
            },
            text: native_titlebar_menu_item_label(item).to_owned(),
            color: Rgba::rgb(222, 232, 229),
            style_flags: CellFlags::default(),
        });
    }
}

fn native_titlebar_rect(window_width: f32, scale_factor: f32) -> NativeOverlayRect {
    NativeOverlayRect {
        origin: PixelPoint { x: 0.0, y: 0.0 },
        size: PixelSize {
            width: window_width.max(1.0),
            height: native_titlebar_height(scale_factor),
        },
    }
}

fn native_titlebar_buttons() -> [NativeTitleBarButton; 5] {
    [
        NativeTitleBarButton::Menu,
        NativeTitleBarButton::NewSession,
        NativeTitleBarButton::Minimize,
        NativeTitleBarButton::Maximize,
        NativeTitleBarButton::Close,
    ]
}

fn native_titlebar_scale(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

fn native_titlebar_height(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_HEIGHT * native_titlebar_scale(scale_factor)
}

fn native_titlebar_button_size(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_BUTTON_SIZE * native_titlebar_scale(scale_factor)
}

fn native_titlebar_button_gap(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_BUTTON_GAP * native_titlebar_scale(scale_factor)
}

fn native_titlebar_edge_padding(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_EDGE_PADDING * native_titlebar_scale(scale_factor)
}

fn native_titlebar_menu_width(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_MENU_WIDTH * native_titlebar_scale(scale_factor)
}

fn native_titlebar_menu_item_height(scale_factor: f32) -> f32 {
    CUSTOM_TITLEBAR_MENU_ITEM_HEIGHT * native_titlebar_scale(scale_factor)
}

fn native_titlebar_separator_height(scale_factor: f32) -> f32 {
    native_titlebar_scale(scale_factor).max(1.0)
}

fn native_titlebar_fullscreen_reveal_zone(scale_factor: f32) -> f64 {
    CUSTOM_TITLEBAR_FULLSCREEN_REVEAL_ZONE * f64::from(native_titlebar_scale(scale_factor))
}

fn native_titlebar_button_rect(
    button: NativeTitleBarButton,
    window_width: f32,
    scale_factor: f32,
) -> Option<NativeOverlayRect> {
    let button_size = native_titlebar_button_size(scale_factor);
    let y = ((native_titlebar_height(scale_factor) - button_size) * 0.5).max(0.0);
    let x = match button {
        NativeTitleBarButton::Menu => native_titlebar_edge_padding(scale_factor),
        NativeTitleBarButton::NewSession => {
            native_titlebar_edge_padding(scale_factor)
                + button_size
                + native_titlebar_button_gap(scale_factor)
        }
        NativeTitleBarButton::Close => {
            native_titlebar_right_button_x(window_width, 0, scale_factor)?
        }
        NativeTitleBarButton::Maximize => {
            native_titlebar_right_button_x(window_width, 1, scale_factor)?
        }
        NativeTitleBarButton::Minimize => {
            native_titlebar_right_button_x(window_width, 2, scale_factor)?
        }
    };

    Some(NativeOverlayRect {
        origin: PixelPoint { x, y },
        size: PixelSize {
            width: button_size,
            height: button_size,
        },
    })
}

fn native_titlebar_right_button_x(
    window_width: f32,
    index_from_right: usize,
    scale_factor: f32,
) -> Option<f32> {
    let button_size = native_titlebar_button_size(scale_factor);
    let x = window_width
        - native_titlebar_edge_padding(scale_factor)
        - button_size
        - index_from_right as f32 * (button_size + native_titlebar_button_gap(scale_factor));
    (x >= 0.0).then_some(x)
}

fn native_titlebar_menu_rect(scale_factor: f32) -> NativeOverlayRect {
    NativeOverlayRect {
        origin: PixelPoint {
            x: native_titlebar_edge_padding(scale_factor),
            y: native_titlebar_height(scale_factor),
        },
        size: PixelSize {
            width: native_titlebar_menu_width(scale_factor),
            height: native_titlebar_menu_items().len() as f32
                * native_titlebar_menu_item_height(scale_factor),
        },
    }
}

fn native_titlebar_menu_items() -> [NativeTitleBarMenuItem; 4] {
    [
        NativeTitleBarMenuItem::NewSession,
        NativeTitleBarMenuItem::CommandPalette,
        NativeTitleBarMenuItem::Search,
        NativeTitleBarMenuItem::About,
    ]
}

fn native_titlebar_menu_item_for_point(
    point: PixelPoint,
    scale_factor: f32,
) -> Option<NativeTitleBarMenuItem> {
    let rect = native_titlebar_menu_rect(scale_factor);
    if !rect.contains_point(point) {
        return None;
    }

    let row = ((point.y - rect.origin.y) / native_titlebar_menu_item_height(scale_factor)).floor()
        as usize;
    native_titlebar_menu_items().get(row).copied()
}

fn native_titlebar_menu_item_label(item: NativeTitleBarMenuItem) -> &'static str {
    match item {
        NativeTitleBarMenuItem::NewSession => "New Session",
        NativeTitleBarMenuItem::CommandPalette => "Command Palette",
        NativeTitleBarMenuItem::Search => "Search",
        NativeTitleBarMenuItem::About => "About Witty",
    }
}

fn rect_item(rect: NativeOverlayRect, color: Rgba) -> RectBatchItem {
    RectBatchItem {
        origin: rect.origin,
        size: rect.size,
        color,
    }
}

fn apply_native_update_notice_overlay(
    frame: &mut FramePlan,
    notice: Option<&NativeUpdateNotice>,
    hovered: Option<NativeUpdateNoticeHit>,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    let Some(notice) = notice else {
        return;
    };
    let Some(row) = native_update_notice_row(grid_size) else {
        return;
    };

    let origin = cell_origin(CellPoint::new(row, 0), metrics);
    let size = PixelSize {
        width: f32::from(grid_size.cols) * metrics.cell.width,
        height: metrics.cell.height,
    };
    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, origin, size));
    frame
        .search_highlights
        .retain(|rect| !rect_origin_inside(rect, origin, size));
    frame
        .selection
        .retain(|rect| !rect_origin_inside(rect, origin, size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, origin, size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin,
        size,
        color: Rgba::rgb(54, 43, 18),
    });
    if hovered.is_some_and(|hit| hit.target == NativeUpdateNoticeTarget::Restart) {
        let width = grid_size.cols.saturating_sub(2);
        let button_width = text_cell_width(&native_update_notice_button_text());
        if width > button_width {
            frame.overlay_backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(row, 1 + width.saturating_sub(button_width)),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(button_width) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: Rgba::rgb(96, 70, 24),
            });
        }
    }
    frame.glyphs.push(GlyphBatchItem {
        origin: cell_origin(CellPoint::new(row, 1), metrics),
        text: native_update_notice_text(notice, grid_size.cols.saturating_sub(2)),
        color: Rgba::rgb(246, 232, 184),
        style_flags: CellFlags::default(),
    });
}

fn native_update_notice_hit_for_position(
    notice: Option<&NativeUpdateNotice>,
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeUpdateNoticeHit> {
    notice?;
    let row = native_update_notice_row(grid_size)?;
    let point = pixel_point_for_position(position)?;
    let cell = cell_point_for_pixel_point(point, metrics, grid_size);
    if cell.row != row || cell.col == 0 {
        return None;
    }

    let width = grid_size.cols.saturating_sub(2);
    let text_col = cell.col.saturating_sub(1);
    Some(NativeUpdateNoticeHit {
        target: native_update_notice_target_for_text_col(text_col, width),
    })
}

fn native_update_notice_target_for_text_col(text_col: u16, width: u16) -> NativeUpdateNoticeTarget {
    let button_width = text_cell_width(&native_update_notice_button_text());
    if width > button_width && text_col >= width.saturating_sub(button_width) {
        NativeUpdateNoticeTarget::Restart
    } else {
        NativeUpdateNoticeTarget::Row
    }
}

fn native_update_notice_row(grid_size: GridSize) -> Option<u16> {
    (grid_size.rows > 0 && grid_size.cols >= 20).then(|| grid_size.rows.saturating_sub(1))
}

fn native_update_notice_text(notice: &NativeUpdateNotice, width: u16) -> String {
    let button = native_update_notice_button_text();
    let button_width = text_cell_width(&button);
    let summary = format!(
        "[update] Witty build installed | running={} installed={}",
        short_build_label(&notice.running_build_id),
        short_build_label(notice.installed_build_id()),
    );
    if width <= button_width.saturating_add(1) {
        return truncate_cells(&summary, width);
    }

    let left_width = width.saturating_sub(button_width).saturating_sub(1);
    let left = truncate_cells(&summary, left_width);
    let spacer = width
        .saturating_sub(text_cell_width(&left))
        .saturating_sub(button_width);
    format!("{left}{}{button}", " ".repeat(usize::from(spacer)))
}

fn native_update_notice_button_text() -> String {
    format!("[{RESTART_BUTTON_LABEL}]")
}

fn short_build_label(build_id: &str) -> String {
    let trimmed = build_id.trim();
    if trimmed.chars().count() <= 12 {
        trimmed.to_owned()
    } else {
        trimmed.chars().take(12).collect()
    }
}

fn profile_action_overlay_body_row_count(snapshot: &NativeProfileActionSnapshot) -> usize {
    snapshot
        .display_rows
        .len()
        .saturating_add(profile_action_start_success_row_count(snapshot))
        .saturating_add(profile_action_start_failure_row_count(snapshot))
        .saturating_add(snapshot.picker_options.len())
}

fn profile_action_start_success_row_count(snapshot: &NativeProfileActionSnapshot) -> usize {
    usize::from(snapshot.start_success.is_some())
}

fn profile_action_start_failure_row_count(snapshot: &NativeProfileActionSnapshot) -> usize {
    usize::from(snapshot.start_failure.is_some())
}

fn profile_action_overlay_row_text(row: &NativeProfileActionDisplayRow, width: u16) -> String {
    let buttons = profile_action_overlay_button_text(row);
    let button_width = text_cell_width(&buttons);
    if width <= button_width.saturating_add(1) {
        return truncate_cells(&profile_action_overlay_row_summary(row), width);
    }

    let left_width = width.saturating_sub(button_width).saturating_sub(1);
    let left = truncate_cells(&profile_action_overlay_row_summary(row), left_width);
    let spacer = width
        .saturating_sub(text_cell_width(&left))
        .saturating_sub(button_width);

    format!("{left}{}{buttons}", " ".repeat(usize::from(spacer)))
}

fn profile_action_overlay_row_summary(row: &NativeProfileActionDisplayRow) -> String {
    let status = profile_action_status_label(row.status);
    let reason = row
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .map(|reason| format!(" reason={reason}"))
        .unwrap_or_default();

    format!(
        "{status} {} | {} | plugin={}{}",
        row.title, row.detail, row.source_plugin, reason
    )
}

fn profile_action_overlay_button_text(row: &NativeProfileActionDisplayRow) -> String {
    profile_overlay_button_text(&profile_action_overlay_buttons(row))
}

fn profile_start_success_overlay_row_text(
    row: &NativeProfileActionStartSuccessRow,
    width: u16,
) -> String {
    let buttons = profile_start_success_overlay_button_text(row);
    let button_width = text_cell_width(&buttons);
    if width <= button_width.saturating_add(1) {
        return truncate_cells(&profile_start_success_overlay_row_summary(row), width);
    }

    let left_width = width.saturating_sub(button_width).saturating_sub(1);
    let left = truncate_cells(&profile_start_success_overlay_row_summary(row), left_width);
    let spacer = width
        .saturating_sub(text_cell_width(&left))
        .saturating_sub(button_width);

    format!("{left}{}{buttons}", " ".repeat(usize::from(spacer)))
}

fn profile_start_success_overlay_row_summary(row: &NativeProfileActionStartSuccessRow) -> String {
    let reason = row
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .map(|reason| format!(" reason={reason}"))
        .unwrap_or_default();

    format!(
        "[started] {} | {} | plugin={}{}",
        row.title, row.detail, row.source_plugin, reason
    )
}

fn profile_start_success_overlay_button_text(row: &NativeProfileActionStartSuccessRow) -> String {
    format!("[{}]", row.dismiss_label)
}

fn profile_start_failure_overlay_row_text(
    row: &NativeProfileActionStartFailureRow,
    width: u16,
) -> String {
    let buttons = profile_start_failure_overlay_button_text(row);
    let button_width = text_cell_width(&buttons);
    if width <= button_width.saturating_add(1) {
        return truncate_cells(&profile_start_failure_overlay_row_summary(row), width);
    }

    let left_width = width.saturating_sub(button_width).saturating_sub(1);
    let left = truncate_cells(&profile_start_failure_overlay_row_summary(row), left_width);
    let spacer = width
        .saturating_sub(text_cell_width(&left))
        .saturating_sub(button_width);

    format!("{left}{}{buttons}", " ".repeat(usize::from(spacer)))
}

fn profile_start_failure_overlay_row_summary(row: &NativeProfileActionStartFailureRow) -> String {
    let reason = row
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .map(|reason| format!(" reason={reason}"))
        .unwrap_or_default();

    format!(
        "[start failed] {} | {} | plugin={}{}",
        row.title, row.detail, row.source_plugin, reason
    )
}

fn profile_start_failure_overlay_button_text(row: &NativeProfileActionStartFailureRow) -> String {
    format!("[{}] [{}]", row.retry_label, row.dismiss_label)
}

fn profile_picker_option_overlay_row_text(
    option: &NativeProfilePickerOptionRow,
    width: u16,
) -> String {
    let buttons = profile_picker_option_overlay_button_text(option);
    let button_width = text_cell_width(&buttons);
    if width <= button_width.saturating_add(1) {
        return truncate_cells(&profile_picker_option_overlay_row_summary(option), width);
    }

    let left_width = width.saturating_sub(button_width).saturating_sub(1);
    let left = truncate_cells(
        &profile_picker_option_overlay_row_summary(option),
        left_width,
    );
    let spacer = width
        .saturating_sub(text_cell_width(&left))
        .saturating_sub(button_width);

    format!("{left}{}{buttons}", " ".repeat(usize::from(spacer)))
}

fn profile_picker_option_overlay_row_summary(option: &NativeProfilePickerOptionRow) -> String {
    format!(
        "  {} {} | {}",
        profile_picker_option_status_label(option.status),
        option.title,
        option.detail
    )
}

fn profile_picker_option_overlay_button_text(option: &NativeProfilePickerOptionRow) -> String {
    let buttons = profile_picker_option_overlay_buttons(option);
    if buttons.is_empty() {
        "[Credentials]".to_owned()
    } else {
        profile_overlay_button_text(&buttons)
    }
}

fn profile_action_overlay_buttons(
    row: &NativeProfileActionDisplayRow,
) -> Vec<(NativeProfileActionOverlayTarget, String)> {
    let mut buttons = Vec::new();
    if let Some(confirm) = row.confirm_label.as_ref() {
        buttons.push((NativeProfileActionOverlayTarget::Confirm, confirm.clone()));
    }
    if let Some(new_tab) = row.new_tab_label.as_ref() {
        buttons.push((
            NativeProfileActionOverlayTarget::ConfirmNewTab,
            new_tab.clone(),
        ));
    }
    buttons.push((
        NativeProfileActionOverlayTarget::Dismiss,
        row.dismiss_label.clone(),
    ));
    buttons
}

fn profile_picker_option_overlay_buttons(
    option: &NativeProfilePickerOptionRow,
) -> Vec<(NativeProfileActionOverlayTarget, String)> {
    let mut buttons = Vec::new();
    if let Some(select) = option.select_label.as_ref() {
        buttons.push((NativeProfileActionOverlayTarget::Confirm, select.clone()));
    }
    if let Some(new_tab) = option.new_tab_label.as_ref() {
        buttons.push((
            NativeProfileActionOverlayTarget::ConfirmNewTab,
            new_tab.clone(),
        ));
    }
    buttons
}

fn profile_overlay_button_text(buttons: &[(NativeProfileActionOverlayTarget, String)]) -> String {
    buttons
        .iter()
        .map(|(_, label)| format!("[{label}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn profile_action_overlay_hit_for_position(
    snapshot: &NativeProfileActionSnapshot,
    position: PhysicalPosition<f64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeProfileActionOverlayHit> {
    let point = pixel_point_for_position(position)?;
    profile_action_overlay_hit_for_pixel_point(snapshot, point, metrics, grid_size)
}

fn profile_action_overlay_hit_for_pixel_point(
    snapshot: &NativeProfileActionSnapshot,
    point: PixelPoint,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<NativeProfileActionOverlayHit> {
    let panel = profile_action_panel(grid_size, profile_action_overlay_body_row_count(snapshot))?;
    let cell = cell_point_for_pixel_point(point, metrics, grid_size);
    if cell.row < panel.start.row
        || cell.row >= panel.start.row.saturating_add(panel.rows)
        || cell.col < panel.start.col
        || cell.col >= panel.start.col.saturating_add(panel.cols)
    {
        return None;
    }

    let row_offset = cell.row.saturating_sub(panel.start.row);
    if row_offset == 0 {
        return None;
    }
    let row_index = usize::from(row_offset.saturating_sub(1));
    if row_index >= panel.item_rows {
        return None;
    }
    let col_offset = cell.col.saturating_sub(panel.start.col);
    let text_col = col_offset.saturating_sub(1);
    if let Some(success) = snapshot
        .start_success
        .as_ref()
        .filter(|_| row_index == snapshot.display_rows.len())
    {
        let target = profile_start_success_overlay_target_for_text_col(
            success,
            text_col,
            panel.cols.saturating_sub(2),
        );
        return Some(NativeProfileActionOverlayHit {
            key: success.key,
            row_index,
            target,
        });
    }

    let start_failure_row_index =
        snapshot.display_rows.len() + profile_action_start_success_row_count(snapshot);
    if let Some(failure) = snapshot
        .start_failure
        .as_ref()
        .filter(|_| row_index == start_failure_row_index)
    {
        let target = profile_start_failure_overlay_target_for_text_col(
            failure,
            text_col,
            panel.cols.saturating_sub(2),
        );
        return Some(NativeProfileActionOverlayHit {
            key: failure.key,
            row_index,
            target,
        });
    }

    let picker_option_start = snapshot.display_rows.len()
        + profile_action_start_success_row_count(snapshot)
        + profile_action_start_failure_row_count(snapshot);
    if row_index >= picker_option_start {
        let option_index = row_index.saturating_sub(picker_option_start);
        let option = snapshot.picker_options.get(option_index)?;
        let target = profile_picker_option_overlay_target_for_text_col(
            option,
            text_col,
            panel.cols.saturating_sub(2),
        );
        return Some(NativeProfileActionOverlayHit {
            key: option.request_key,
            row_index,
            target,
        });
    }

    let row = snapshot.display_rows.get(row_index)?;
    let target =
        profile_action_overlay_target_for_text_col(row, text_col, panel.cols.saturating_sub(2));

    Some(NativeProfileActionOverlayHit {
        key: row.key,
        row_index,
        target,
    })
}

fn profile_action_overlay_confirmation_for_hit(
    snapshot: &NativeProfileActionSnapshot,
    hit: NativeProfileActionOverlayHit,
) -> Option<PendingProfileActionConfirmation> {
    if !matches!(
        hit.target,
        NativeProfileActionOverlayTarget::Confirm | NativeProfileActionOverlayTarget::ConfirmNewTab
    ) {
        return None;
    }

    if hit.row_index >= snapshot.display_rows.len() {
        if profile_action_overlay_start_success_for_hit(snapshot, hit).is_some() {
            return None;
        }
        if profile_action_overlay_start_failure_for_hit(snapshot, hit).is_some() {
            return None;
        }

        let option_index = hit
            .row_index
            .saturating_sub(snapshot.display_rows.len())
            .saturating_sub(profile_action_start_success_row_count(snapshot))
            .saturating_sub(profile_action_start_failure_row_count(snapshot));
        let option = snapshot.picker_options.get(option_index)?;
        if option.request_key != hit.key
            || option.request_key.kind != witty_ui::PendingProfileActionKind::ProfilePicker
            || option.status != NativeProfilePickerOptionStatus::Launchable
            || !profile_picker_option_can_confirm(option, hit.target)
        {
            return None;
        }

        return Some(PendingProfileActionConfirmation::profile_picker(
            hit.key,
            option.profile_id.clone(),
        ));
    }

    let row = snapshot.display_rows.get(hit.row_index)?;
    if row.key != hit.key
        || row.key.kind != witty_ui::PendingProfileActionKind::ProfileLaunch
        || row.status != NativeProfileActionDisplayStatus::Launchable
        || !profile_action_row_can_confirm(row, hit.target)
    {
        return None;
    }

    Some(PendingProfileActionConfirmation::profile_launch(hit.key))
}

fn profile_action_row_can_confirm(
    row: &NativeProfileActionDisplayRow,
    target: NativeProfileActionOverlayTarget,
) -> bool {
    match target {
        NativeProfileActionOverlayTarget::Confirm => row.confirm_label.is_some(),
        NativeProfileActionOverlayTarget::ConfirmNewTab => row.new_tab_label.is_some(),
        NativeProfileActionOverlayTarget::Dismiss | NativeProfileActionOverlayTarget::Row => false,
    }
}

fn profile_picker_option_can_confirm(
    option: &NativeProfilePickerOptionRow,
    target: NativeProfileActionOverlayTarget,
) -> bool {
    match target {
        NativeProfileActionOverlayTarget::Confirm => option.select_label.is_some(),
        NativeProfileActionOverlayTarget::ConfirmNewTab => option.new_tab_label.is_some(),
        NativeProfileActionOverlayTarget::Dismiss | NativeProfileActionOverlayTarget::Row => false,
    }
}

fn native_profile_action_start_mode_for_overlay_target(
    target: NativeProfileActionOverlayTarget,
) -> Option<NativeProfileActionStartMode> {
    match target {
        NativeProfileActionOverlayTarget::Confirm => {
            Some(NativeProfileActionStartMode::ReplaceCurrentSession)
        }
        NativeProfileActionOverlayTarget::ConfirmNewTab => {
            Some(NativeProfileActionStartMode::NewTab)
        }
        NativeProfileActionOverlayTarget::Dismiss | NativeProfileActionOverlayTarget::Row => None,
    }
}

fn profile_action_overlay_start_success_for_hit(
    snapshot: &NativeProfileActionSnapshot,
    hit: NativeProfileActionOverlayHit,
) -> Option<&NativeProfileActionStartSuccessRow> {
    let row = snapshot.start_success.as_ref()?;
    (hit.row_index == snapshot.display_rows.len() && hit.key == row.key).then_some(row)
}

fn profile_action_overlay_start_failure_for_hit(
    snapshot: &NativeProfileActionSnapshot,
    hit: NativeProfileActionOverlayHit,
) -> Option<&NativeProfileActionStartFailureRow> {
    let row = snapshot.start_failure.as_ref()?;
    let row_index = snapshot.display_rows.len() + profile_action_start_success_row_count(snapshot);
    (hit.row_index == row_index && hit.key == row.key).then_some(row)
}

fn profile_picker_option_overlay_target_for_text_col(
    option: &NativeProfilePickerOptionRow,
    text_col: u16,
    width: u16,
) -> NativeProfileActionOverlayTarget {
    let buttons = profile_picker_option_overlay_buttons(option);
    if buttons.is_empty() {
        return NativeProfileActionOverlayTarget::Row;
    }
    profile_overlay_button_target_for_text_col(&buttons, text_col, width)
}

fn profile_start_success_overlay_target_for_text_col(
    row: &NativeProfileActionStartSuccessRow,
    text_col: u16,
    width: u16,
) -> NativeProfileActionOverlayTarget {
    let dismiss_label = format!("[{}]", row.dismiss_label);
    let dismiss_width = text_cell_width(&dismiss_label);
    if width <= dismiss_width.saturating_add(1) {
        return NativeProfileActionOverlayTarget::Row;
    }

    if text_col >= width.saturating_sub(dismiss_width) {
        NativeProfileActionOverlayTarget::Dismiss
    } else {
        NativeProfileActionOverlayTarget::Row
    }
}

fn profile_start_failure_overlay_target_for_text_col(
    row: &NativeProfileActionStartFailureRow,
    text_col: u16,
    width: u16,
) -> NativeProfileActionOverlayTarget {
    let retry_label = format!("[{}]", row.retry_label);
    let retry_width = text_cell_width(&retry_label);
    let dismiss_label = format!("[{}]", row.dismiss_label);
    let dismiss_width = text_cell_width(&dismiss_label);
    let buttons_width = retry_width.saturating_add(1).saturating_add(dismiss_width);
    if width <= buttons_width.saturating_add(1) {
        return NativeProfileActionOverlayTarget::Row;
    }

    let retry_start = width.saturating_sub(buttons_width);
    let dismiss_start = retry_start.saturating_add(retry_width).saturating_add(1);
    if text_col >= dismiss_start {
        NativeProfileActionOverlayTarget::Dismiss
    } else if text_col >= retry_start {
        NativeProfileActionOverlayTarget::Confirm
    } else {
        NativeProfileActionOverlayTarget::Row
    }
}

fn profile_action_overlay_target_for_text_col(
    row: &NativeProfileActionDisplayRow,
    text_col: u16,
    width: u16,
) -> NativeProfileActionOverlayTarget {
    profile_overlay_button_target_for_text_col(
        &profile_action_overlay_buttons(row),
        text_col,
        width,
    )
}

fn profile_overlay_button_target_for_text_col(
    buttons: &[(NativeProfileActionOverlayTarget, String)],
    text_col: u16,
    width: u16,
) -> NativeProfileActionOverlayTarget {
    let button_widths = buttons
        .iter()
        .map(|(_, label)| text_cell_width(&format!("[{label}]")))
        .collect::<Vec<_>>();
    let buttons_width = button_widths
        .iter()
        .copied()
        .fold(0u16, |width, button_width| {
            width.saturating_add(button_width)
        })
        .saturating_add(buttons.len().saturating_sub(1) as u16);
    if buttons.is_empty() || width <= buttons_width.saturating_add(1) {
        return NativeProfileActionOverlayTarget::Row;
    }

    let mut start = width.saturating_sub(buttons_width);
    for ((target, _), button_width) in buttons.iter().zip(button_widths) {
        let end = start.saturating_add(button_width);
        if text_col >= start && text_col < end {
            return *target;
        }
        start = end.saturating_add(1);
    }
    NativeProfileActionOverlayTarget::Row
}

fn profile_action_status_label(status: NativeProfileActionDisplayStatus) -> &'static str {
    match status {
        NativeProfileActionDisplayStatus::PickProfile => "[pick]",
        NativeProfileActionDisplayStatus::Launchable => "[ready]",
        NativeProfileActionDisplayStatus::RequiresCredentialResolver => "[credentials]",
        NativeProfileActionDisplayStatus::NotFound => "[missing]",
    }
}

fn native_profile_action_start_mode_label(mode: NativeProfileActionStartMode) -> &'static str {
    match mode {
        NativeProfileActionStartMode::ReplaceCurrentSession => "replace_current_session",
        NativeProfileActionStartMode::NewTab => "new_tab",
    }
}

fn profile_action_status_color(status: NativeProfileActionDisplayStatus) -> Rgba {
    match status {
        NativeProfileActionDisplayStatus::PickProfile => Rgba::rgb(210, 226, 238),
        NativeProfileActionDisplayStatus::Launchable => Rgba::rgb(190, 230, 178),
        NativeProfileActionDisplayStatus::RequiresCredentialResolver => Rgba::rgb(238, 204, 136),
        NativeProfileActionDisplayStatus::NotFound => Rgba::rgb(238, 150, 142),
    }
}

fn profile_start_failure_status_color() -> Rgba {
    Rgba::rgb(244, 184, 148)
}

fn profile_start_success_status_color() -> Rgba {
    Rgba::rgb(160, 220, 176)
}

fn profile_picker_option_status_label(status: NativeProfilePickerOptionStatus) -> &'static str {
    match status {
        NativeProfilePickerOptionStatus::Launchable => "[profile]",
        NativeProfilePickerOptionStatus::RequiresCredentialResolver => "[credentials]",
    }
}

fn profile_picker_option_status_color(status: NativeProfilePickerOptionStatus) -> Rgba {
    match status {
        NativeProfilePickerOptionStatus::Launchable => Rgba::rgb(204, 224, 238),
        NativeProfilePickerOptionStatus::RequiresCredentialResolver => Rgba::rgb(238, 204, 136),
    }
}

fn profile_action_hover_color(target: NativeProfileActionOverlayTarget) -> Rgba {
    match target {
        NativeProfileActionOverlayTarget::Row => Rgba::with_alpha(64, 84, 98, 150),
        NativeProfileActionOverlayTarget::Confirm => Rgba::with_alpha(48, 108, 72, 160),
        NativeProfileActionOverlayTarget::ConfirmNewTab => Rgba::with_alpha(48, 82, 132, 160),
        NativeProfileActionOverlayTarget::Dismiss => Rgba::with_alpha(118, 62, 58, 160),
    }
}

fn palette_item_text(
    command: &CommandRegistration,
    selected: bool,
    commands: &[CommandRegistration],
    width: u16,
) -> String {
    let marker = if selected { ">" } else { " " };
    let base = format!("{marker} {}  {}", command.title, command.id);
    let text = match shortcut_label_for_command(command, commands) {
        Some(shortcut) => format!("{base}  {shortcut}"),
        None => base,
    };

    truncate_cells(&text, width)
}

fn shortcut_label_for_command(
    command: &CommandRegistration,
    commands: &[CommandRegistration],
) -> Option<&'static str> {
    if command.id == "witty.about" && has_command(commands, "witty.about") {
        return Some("F1");
    }

    let first_external = commands
        .iter()
        .find(|candidate| candidate.source_plugin != "builtin")?;
    (first_external.id == command.id).then_some("F2")
}

fn apply_frame_diagnostics_overlay(
    frame: &mut FramePlan,
    stats: FrameStats,
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return;
    }

    let lines = frame_diagnostics_lines(stats);
    let content_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let panel_cols = content_width.saturating_add(2).min(grid_size.cols);
    let panel_rows = (lines.len() as u16).min(grid_size.rows);
    if panel_cols == 0 || panel_rows == 0 {
        return;
    }

    let panel = PalettePanel {
        start: CellPoint::new(0, grid_size.cols.saturating_sub(panel_cols)),
        cols: panel_cols,
        rows: panel_rows,
        item_rows: 0,
    };
    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));
    if frame
        .cursor
        .as_ref()
        .is_some_and(|cursor| rect_origin_inside(cursor, panel_origin, panel_size))
    {
        frame.cursor = None;
    }

    frame.overlay_backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(24, 30, 34),
    });

    for (index, line) in lines.iter().take(panel_rows as usize).enumerate() {
        push_palette_text(
            frame,
            panel,
            metrics,
            index as u16,
            1,
            &truncate_cells(line, panel.cols.saturating_sub(2)),
            Rgba::rgb(208, 224, 214),
        );
    }
}

fn frame_diagnostics_lines(stats: FrameStats) -> Vec<String> {
    vec![
        format!(
            "damage {} regions={}",
            if stats.full_damage { "full" } else { "rows" },
            stats.damage_regions
        ),
        format!(
            "rows reused={} rebuilt={}",
            stats.reused_rows, stats.rebuilt_rows
        ),
        format!(
            "runs bg={} glyph={} chars={} batches={} max={}",
            stats.background_runs,
            stats.glyph_runs,
            stats.glyph_chars,
            stats.glyph_prepare_batches,
            stats.max_glyph_run_chars
        ),
        format!(
            "rectv={} cap={} sel={} deco={}",
            stats.rect_vertices,
            stats.rect_vertex_capacity,
            stats.selection_rects,
            stats.text_decoration_rects
        ),
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PalettePanel {
    start: CellPoint,
    cols: u16,
    rows: u16,
    item_rows: usize,
}

fn palette_panel(grid_size: GridSize, filtered_count: usize) -> Option<PalettePanel> {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return None;
    }

    let cols = grid_size.cols;
    let panel_cols = if cols > 80 {
        76
    } else {
        cols.saturating_sub(4).max(1)
    };
    let start_col = cols.saturating_sub(panel_cols) / 2;

    let max_item_rows = usize::from(grid_size.rows.saturating_sub(1)).min(8);
    let item_rows = if max_item_rows == 0 {
        0
    } else {
        filtered_count.min(max_item_rows).max(1)
    };
    let panel_rows = u16::try_from(item_rows + 1)
        .unwrap_or(u16::MAX)
        .min(grid_size.rows);
    let start_row = if grid_size.rows > panel_rows + 4 {
        2
    } else {
        grid_size.rows.saturating_sub(panel_rows) / 2
    };

    Some(PalettePanel {
        start: CellPoint::new(start_row, start_col),
        cols: panel_cols,
        rows: panel_rows,
        item_rows,
    })
}

fn session_switcher_panel(grid_size: GridSize, row_count: usize) -> Option<PalettePanel> {
    if grid_size.rows < 2 || grid_size.cols == 0 || row_count < 2 {
        return None;
    }

    let cols = grid_size.cols;
    let panel_cols = if cols > 80 {
        64
    } else {
        cols.saturating_sub(4).max(1)
    };
    let start_col = cols.saturating_sub(panel_cols) / 2;
    let max_item_rows = usize::from(grid_size.rows.saturating_sub(1)).min(8);
    let item_rows = row_count.min(max_item_rows).max(1);
    let panel_rows = u16::try_from(item_rows + 1)
        .unwrap_or(u16::MAX)
        .min(grid_size.rows);
    let start_row = if grid_size.rows > panel_rows + 4 {
        2
    } else {
        grid_size.rows.saturating_sub(panel_rows) / 2
    };

    Some(PalettePanel {
        start: CellPoint::new(start_row, start_col),
        cols: panel_cols,
        rows: panel_rows,
        item_rows,
    })
}

fn session_switcher_row_text(
    row: &NativeSessionTabRow,
    row_index: usize,
    selected: bool,
    label_style: NativeSessionTabLabelStyle,
    width: u16,
) -> String {
    let marker = if selected { ">" } else { " " };
    let active = if row.active { "active" } else { "session" };
    let label = native_session_tab_label(row, row_index, label_style);
    truncate_cells(
        &format!("{marker} {label}  {}  {active}", row.profile_id),
        width,
    )
}

fn profile_action_panel(grid_size: GridSize, row_count: usize) -> Option<PalettePanel> {
    if row_count == 0 || grid_size.rows < 2 || grid_size.cols == 0 {
        return None;
    }

    let panel_cols = if grid_size.cols > 96 {
        72
    } else {
        grid_size.cols.saturating_sub(4).max(1)
    };
    let max_panel_rows = grid_size.rows.min(7);
    let max_body_rows = usize::from(max_panel_rows.saturating_sub(1));
    if max_body_rows == 0 {
        return None;
    }

    let mut item_rows = row_count.min(max_body_rows);
    let mut hidden = row_count.saturating_sub(item_rows);
    if hidden > 0 && item_rows > 0 {
        item_rows = item_rows.saturating_sub(1);
        hidden = row_count.saturating_sub(item_rows);
    }
    let panel_rows = 1usize
        .saturating_add(item_rows)
        .saturating_add(usize::from(hidden > 0));
    let panel_rows = u16::try_from(panel_rows)
        .unwrap_or(u16::MAX)
        .min(grid_size.rows);
    if panel_rows < 2 {
        return None;
    }

    let start_row = if grid_size.rows > panel_rows + 2 {
        1
    } else {
        0
    };
    let start_col = grid_size.cols.saturating_sub(panel_cols).saturating_sub(1);

    Some(PalettePanel {
        start: CellPoint::new(start_row, start_col),
        cols: panel_cols,
        rows: panel_rows,
        item_rows,
    })
}

fn push_palette_text(
    frame: &mut FramePlan,
    panel: PalettePanel,
    metrics: CellMetrics,
    row_offset: u16,
    col_offset: u16,
    text: &str,
    color: Rgba,
) {
    if row_offset >= panel.rows || col_offset >= panel.cols {
        return;
    }

    frame.glyphs.push(GlyphBatchItem {
        origin: cell_origin(
            CellPoint::new(panel.start.row + row_offset, panel.start.col + col_offset),
            metrics,
        ),
        text: text.to_owned(),
        color,
        style_flags: CellFlags::default(),
    });
}

fn push_centered_palette_text(
    frame: &mut FramePlan,
    panel: PalettePanel,
    metrics: CellMetrics,
    row_offset: u16,
    text: &str,
    color: Rgba,
) {
    if row_offset >= panel.rows || panel.cols == 0 {
        return;
    }

    let width = panel.cols.saturating_sub(2).max(1);
    let text = truncate_cells(text, width);
    let col_offset = if panel.cols <= 2 {
        0
    } else {
        let text_width = text_cell_width(&text).min(width);
        1 + width.saturating_sub(text_width) / 2
    };
    push_palette_text(frame, panel, metrics, row_offset, col_offset, &text, color);
}

fn search_ime_cursor_cell(
    search: &TerminalSearch,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> CellPoint {
    let row = grid_size.rows.saturating_sub(1);
    let visible_width = text_cell_width("Find: ")
        .saturating_add(text_cell_width(search.query()))
        .saturating_add(ime_preedit_caret_cell_width(composition));
    let col = 1u16
        .saturating_add(visible_width)
        .min(grid_size.cols.saturating_sub(1));

    CellPoint::new(row, col)
}

fn terminal_ime_cursor_cell(
    terminal_cursor: CellPoint,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> CellPoint {
    let row = terminal_cursor.row.min(grid_size.rows.saturating_sub(1));
    let col = terminal_cursor
        .col
        .saturating_add(composition.preedit_caret_cell_width())
        .min(grid_size.cols.saturating_sub(1));

    CellPoint::new(row, col)
}

fn command_palette_ime_cursor_cell(
    palette: &CommandPalette,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> Option<CellPoint> {
    let panel = palette_panel(grid_size, palette.filtered_count())?;
    let visible_width = text_cell_width("Command Palette  ")
        .saturating_add(text_cell_width(palette.query()))
        .saturating_add(ime_preedit_caret_cell_width(composition));
    let available_width = panel.cols.saturating_sub(2);
    let col = panel
        .start
        .col
        .saturating_add(1)
        .saturating_add(visible_width.min(available_width))
        .min(grid_size.cols.saturating_sub(1));

    Some(CellPoint::new(panel.start.row, col))
}

#[cfg(test)]
fn palette_title(query: &str, width: u16) -> String {
    palette_title_with_ime(query, None, width)
}

fn palette_title_with_ime(query: &str, ime: Option<&ImeComposition>, width: u16) -> String {
    let query = palette_display_query(query, ime);
    let title = if query.is_empty() {
        "Command Palette".to_owned()
    } else {
        format!("Command Palette  {query}")
    };

    truncate_cells(&title, width)
}

fn palette_display_query(query: &str, ime: Option<&ImeComposition>) -> String {
    let mut display = query.to_owned();
    if let Some(ime) = ime.filter(|ime| ime.is_active()) {
        display.push_str(ime.preedit());
    }
    display
}

fn ime_preedit_caret_cell_width(composition: &ImeComposition) -> u16 {
    composition.preedit_caret_cell_width()
}

fn glyph_row_containing_text(
    glyphs: &[GlyphBatchItem],
    metrics: CellMetrics,
    needle: &str,
) -> Option<u16> {
    if needle.is_empty() {
        return None;
    }

    let mut row_glyphs = glyphs
        .iter()
        .map(|glyph| {
            (
                ((glyph.origin.y - metrics.padding.y) / metrics.cell.height).floor() as u16,
                glyph.origin.x,
                glyph.text.as_str(),
            )
        })
        .collect::<Vec<_>>();
    row_glyphs.sort_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    let mut current_row = None;
    let mut current_text = String::new();
    for (row, _, text) in row_glyphs {
        if current_row.is_some_and(|current| current != row) {
            if current_text.contains(needle) {
                return current_row;
            }
            current_text.clear();
        }
        current_row = Some(row);
        current_text.push_str(text);
    }

    current_row.filter(|_| current_text.contains(needle))
}

fn text_cell_width(text: &str) -> u16 {
    text.chars().fold(0u16, |width, ch| {
        width.saturating_add(u16::from(terminal_char_width(ch)))
    })
}

fn truncate_cells(text: &str, width: u16) -> String {
    if text_cell_width(text) <= width {
        return text.to_owned();
    }
    if width <= 3 {
        return ".".repeat(usize::from(width));
    }

    let target_width = width.saturating_sub(3);
    let mut truncated = String::new();
    let mut cells = 0u16;
    for ch in text.chars() {
        let ch_width = u16::from(terminal_char_width(ch));
        if cells.saturating_add(ch_width) > target_width {
            break;
        }
        truncated.push(ch);
        cells = cells.saturating_add(ch_width);
    }
    truncated.push_str("...");
    truncated
}

fn glyph_origin_inside(glyph: &GlyphBatchItem, origin: PixelPoint, size: PixelSize) -> bool {
    glyph.origin.x >= origin.x
        && glyph.origin.x < origin.x + size.width
        && glyph.origin.y >= origin.y
        && glyph.origin.y < origin.y + size.height
}

fn rect_origin_inside(rect: &RectBatchItem, origin: PixelPoint, size: PixelSize) -> bool {
    rect.origin.x >= origin.x
        && rect.origin.x < origin.x + size.width
        && rect.origin.y >= origin.y
        && rect.origin.y < origin.y + size.height
}

fn cell_origin(point: CellPoint, metrics: CellMetrics) -> PixelPoint {
    PixelPoint {
        x: metrics.padding.x + f32::from(point.col) * metrics.cell.width,
        y: metrics.padding.y + f32::from(point.row) * metrics.cell.height,
    }
}

#[cfg(test)]
fn encode_key_input(
    logical_key: &Key,
    text: Option<&str>,
    control: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_key_input_with_modifiers(
        logical_key,
        text,
        TerminalKeyModifiers {
            control,
            ..TerminalKeyModifiers::default()
        },
        modes,
    )
}

#[cfg(test)]
fn encode_key_input_with_modifiers(
    logical_key: &Key,
    text: Option<&str>,
    modifiers: TerminalKeyModifiers,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_terminal_key_input(
        TerminalKeyInput {
            logical_key,
            text,
            modifiers,
            keypad_key: None,
        },
        modes,
    )
}

fn encode_key_event_input(
    event: &KeyEvent,
    modifiers: ModifiersState,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_terminal_key_input(
        TerminalKeyInput {
            logical_key: &event.logical_key,
            text: event.text.as_deref(),
            modifiers: TerminalKeyModifiers::from_winit(modifiers),
            keypad_key: keypad_key_from_winit_event(event),
        },
        modes,
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TerminalKeyModifiers {
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl TerminalKeyModifiers {
    fn from_winit(modifiers: ModifiersState) -> Self {
        Self {
            control: modifiers.control_key(),
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            meta: modifiers.super_key(),
        }
    }

    fn allows_application_keypad(self) -> bool {
        !self.control && !self.shift && !self.alt && !self.meta
    }

    fn xterm_parameter(self) -> Option<u8> {
        if self.meta {
            return None;
        }

        let mut parameter = 1;
        if self.shift {
            parameter += 1;
        }
        if self.alt {
            parameter += 2;
        }
        if self.control {
            parameter += 4;
        }

        (parameter > 1).then_some(parameter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeypadKey {
    Digit(u8),
    Decimal,
    Comma,
    Add,
    Subtract,
    Multiply,
    Divide,
    Enter,
    Equal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalKeyInput<'a> {
    logical_key: &'a Key,
    text: Option<&'a str>,
    modifiers: TerminalKeyModifiers,
    keypad_key: Option<KeypadKey>,
}

fn encode_terminal_key_input(
    input: TerminalKeyInput<'_>,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    if modes.keyboard_locked {
        return None;
    }

    if modes.application_keypad && input.modifiers.allows_application_keypad() {
        if let Some(bytes) = input.keypad_key.and_then(application_keypad_sequence) {
            return Some(bytes);
        }
    }

    if let Some(parameter) = input.modifiers.xterm_parameter() {
        if let Some(bytes) = modified_named_key_sequence(input.logical_key, parameter) {
            return Some(bytes);
        }
    }

    match input.logical_key {
        Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
        Key::Named(NamedKey::Tab) => Some(b"\t".to_vec()),
        Key::Named(NamedKey::Backspace) => Some(backspace_sequence(modes)),
        Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
        Key::Named(NamedKey::ArrowUp) => Some(cursor_key_sequence(b'A', modes)),
        Key::Named(NamedKey::ArrowDown) => Some(cursor_key_sequence(b'B', modes)),
        Key::Named(NamedKey::ArrowRight) => Some(cursor_key_sequence(b'C', modes)),
        Key::Named(NamedKey::ArrowLeft) => Some(cursor_key_sequence(b'D', modes)),
        Key::Named(NamedKey::Home) => Some(cursor_key_sequence(b'H', modes)),
        Key::Named(NamedKey::End) => Some(cursor_key_sequence(b'F', modes)),
        Key::Named(NamedKey::Insert) => Some(csi_tilde_sequence(2)),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
        Key::Named(NamedKey::F1) => Some(ss3_sequence(b'P')),
        Key::Named(NamedKey::F2) => Some(ss3_sequence(b'Q')),
        Key::Named(NamedKey::F3) => Some(ss3_sequence(b'R')),
        Key::Named(NamedKey::F4) => Some(ss3_sequence(b'S')),
        Key::Named(NamedKey::F5) => Some(csi_tilde_sequence(15)),
        Key::Named(NamedKey::F6) => Some(csi_tilde_sequence(17)),
        Key::Named(NamedKey::F7) => Some(csi_tilde_sequence(18)),
        Key::Named(NamedKey::F8) => Some(csi_tilde_sequence(19)),
        Key::Named(NamedKey::F9) => Some(csi_tilde_sequence(20)),
        Key::Named(NamedKey::F10) => Some(csi_tilde_sequence(21)),
        Key::Named(NamedKey::F11) => Some(csi_tilde_sequence(23)),
        Key::Named(NamedKey::F12) => Some(csi_tilde_sequence(24)),
        Key::Character(value) if input.modifiers.control => encode_control_character(value),
        _ => input.text.and_then(non_empty_bytes),
    }
}

fn backspace_sequence(modes: TerminalInputModes) -> Vec<u8> {
    if modes.backarrow_sends_backspace {
        b"\x08".to_vec()
    } else {
        b"\x7f".to_vec()
    }
}

fn modified_named_key_sequence(logical_key: &Key, modifier_parameter: u8) -> Option<Vec<u8>> {
    match logical_key {
        Key::Named(NamedKey::ArrowUp) => {
            Some(csi_modified_final_sequence(1, modifier_parameter, b'A'))
        }
        Key::Named(NamedKey::ArrowDown) => {
            Some(csi_modified_final_sequence(1, modifier_parameter, b'B'))
        }
        Key::Named(NamedKey::ArrowRight) => {
            Some(csi_modified_final_sequence(1, modifier_parameter, b'C'))
        }
        Key::Named(NamedKey::ArrowLeft) => {
            Some(csi_modified_final_sequence(1, modifier_parameter, b'D'))
        }
        Key::Named(NamedKey::Home) => {
            Some(csi_modified_final_sequence(1, modifier_parameter, b'H'))
        }
        Key::Named(NamedKey::End) => Some(csi_modified_final_sequence(1, modifier_parameter, b'F')),
        Key::Named(NamedKey::Insert) => Some(csi_modified_tilde_sequence(2, modifier_parameter)),
        Key::Named(NamedKey::Delete) => Some(csi_modified_tilde_sequence(3, modifier_parameter)),
        Key::Named(NamedKey::PageUp) => Some(csi_modified_tilde_sequence(5, modifier_parameter)),
        Key::Named(NamedKey::PageDown) => Some(csi_modified_tilde_sequence(6, modifier_parameter)),
        Key::Named(NamedKey::F1) => Some(csi_modified_final_sequence(1, modifier_parameter, b'P')),
        Key::Named(NamedKey::F2) => Some(csi_modified_final_sequence(1, modifier_parameter, b'Q')),
        Key::Named(NamedKey::F3) => Some(csi_modified_final_sequence(1, modifier_parameter, b'R')),
        Key::Named(NamedKey::F4) => Some(csi_modified_final_sequence(1, modifier_parameter, b'S')),
        Key::Named(NamedKey::F5) => Some(csi_modified_tilde_sequence(15, modifier_parameter)),
        Key::Named(NamedKey::F6) => Some(csi_modified_tilde_sequence(17, modifier_parameter)),
        Key::Named(NamedKey::F7) => Some(csi_modified_tilde_sequence(18, modifier_parameter)),
        Key::Named(NamedKey::F8) => Some(csi_modified_tilde_sequence(19, modifier_parameter)),
        Key::Named(NamedKey::F9) => Some(csi_modified_tilde_sequence(20, modifier_parameter)),
        Key::Named(NamedKey::F10) => Some(csi_modified_tilde_sequence(21, modifier_parameter)),
        Key::Named(NamedKey::F11) => Some(csi_modified_tilde_sequence(23, modifier_parameter)),
        Key::Named(NamedKey::F12) => Some(csi_modified_tilde_sequence(24, modifier_parameter)),
        _ => None,
    }
}

fn keypad_key_from_winit_event(event: &KeyEvent) -> Option<KeypadKey> {
    if let PhysicalKey::Code(code) = &event.physical_key {
        if let Some(keypad_key) = keypad_key_from_winit_key_code(*code) {
            return Some(keypad_key);
        }
    }

    keypad_key_from_winit_location(&event.logical_key, event.text.as_deref(), event.location)
}

fn keypad_key_from_winit_location(
    logical_key: &Key,
    text: Option<&str>,
    location: KeyLocation,
) -> Option<KeypadKey> {
    if location != KeyLocation::Numpad {
        return None;
    }

    match logical_key {
        Key::Named(NamedKey::Enter) => Some(KeypadKey::Enter),
        Key::Character(value) => keypad_key_from_text(value),
        _ => text.and_then(keypad_key_from_text),
    }
}

fn keypad_key_from_winit_key_code(code: KeyCode) -> Option<KeypadKey> {
    match code {
        KeyCode::Numpad0 => Some(KeypadKey::Digit(0)),
        KeyCode::Numpad1 => Some(KeypadKey::Digit(1)),
        KeyCode::Numpad2 => Some(KeypadKey::Digit(2)),
        KeyCode::Numpad3 => Some(KeypadKey::Digit(3)),
        KeyCode::Numpad4 => Some(KeypadKey::Digit(4)),
        KeyCode::Numpad5 => Some(KeypadKey::Digit(5)),
        KeyCode::Numpad6 => Some(KeypadKey::Digit(6)),
        KeyCode::Numpad7 => Some(KeypadKey::Digit(7)),
        KeyCode::Numpad8 => Some(KeypadKey::Digit(8)),
        KeyCode::Numpad9 => Some(KeypadKey::Digit(9)),
        KeyCode::NumpadDecimal => Some(KeypadKey::Decimal),
        KeyCode::NumpadComma => Some(KeypadKey::Comma),
        KeyCode::NumpadAdd => Some(KeypadKey::Add),
        KeyCode::NumpadSubtract => Some(KeypadKey::Subtract),
        KeyCode::NumpadMultiply => Some(KeypadKey::Multiply),
        KeyCode::NumpadDivide => Some(KeypadKey::Divide),
        KeyCode::NumpadEnter => Some(KeypadKey::Enter),
        KeyCode::NumpadEqual => Some(KeypadKey::Equal),
        _ => None,
    }
}

fn keypad_key_from_text(text: &str) -> Option<KeypadKey> {
    match text {
        "0" => Some(KeypadKey::Digit(0)),
        "1" => Some(KeypadKey::Digit(1)),
        "2" => Some(KeypadKey::Digit(2)),
        "3" => Some(KeypadKey::Digit(3)),
        "4" => Some(KeypadKey::Digit(4)),
        "5" => Some(KeypadKey::Digit(5)),
        "6" => Some(KeypadKey::Digit(6)),
        "7" => Some(KeypadKey::Digit(7)),
        "8" => Some(KeypadKey::Digit(8)),
        "9" => Some(KeypadKey::Digit(9)),
        "." => Some(KeypadKey::Decimal),
        "," => Some(KeypadKey::Comma),
        "+" => Some(KeypadKey::Add),
        "-" => Some(KeypadKey::Subtract),
        "*" => Some(KeypadKey::Multiply),
        "/" => Some(KeypadKey::Divide),
        "=" => Some(KeypadKey::Equal),
        _ => None,
    }
}

fn application_keypad_sequence(keypad_key: KeypadKey) -> Option<Vec<u8>> {
    let final_byte = match keypad_key {
        KeypadKey::Digit(0) => b'p',
        KeypadKey::Digit(1) => b'q',
        KeypadKey::Digit(2) => b'r',
        KeypadKey::Digit(3) => b's',
        KeypadKey::Digit(4) => b't',
        KeypadKey::Digit(5) => b'u',
        KeypadKey::Digit(6) => b'v',
        KeypadKey::Digit(7) => b'w',
        KeypadKey::Digit(8) => b'x',
        KeypadKey::Digit(9) => b'y',
        KeypadKey::Multiply => b'j',
        KeypadKey::Add => b'k',
        KeypadKey::Comma => b'l',
        KeypadKey::Subtract => b'm',
        KeypadKey::Decimal => b'n',
        KeypadKey::Divide => b'o',
        KeypadKey::Enter => b'M',
        KeypadKey::Equal | KeypadKey::Digit(_) => return None,
    };
    Some(ss3_sequence(final_byte))
}

fn cursor_key_sequence(final_byte: u8, modes: TerminalInputModes) -> Vec<u8> {
    let prefix = if modes.application_cursor_keys {
        b"\x1bO"
    } else {
        b"\x1b["
    };
    let mut bytes = prefix.to_vec();
    bytes.push(final_byte);
    bytes
}

fn ss3_sequence(final_byte: u8) -> Vec<u8> {
    vec![0x1b, b'O', final_byte]
}

fn csi_tilde_sequence(parameter: u8) -> Vec<u8> {
    format!("\x1b[{parameter}~").into_bytes()
}

fn csi_modified_final_sequence(
    base_parameter: u8,
    modifier_parameter: u8,
    final_byte: u8,
) -> Vec<u8> {
    let mut bytes = format!("\x1b[{base_parameter};{modifier_parameter}").into_bytes();
    bytes.push(final_byte);
    bytes
}

fn csi_modified_tilde_sequence(base_parameter: u8, modifier_parameter: u8) -> Vec<u8> {
    format!("\x1b[{base_parameter};{modifier_parameter}~").into_bytes()
}

fn encode_control_character(value: &str) -> Option<Vec<u8>> {
    let ch = value.chars().next()?.to_ascii_lowercase();
    match ch {
        'a'..='z' => Some(vec![(ch as u8) - b'a' + 1]),
        '[' => Some(vec![0x1b]),
        '\\' => Some(vec![0x1c]),
        ']' => Some(vec![0x1d]),
        '^' => Some(vec![0x1e]),
        '_' => Some(vec![0x1f]),
        '?' => Some(vec![0x7f]),
        _ => None,
    }
}

fn non_empty_bytes(text: &str) -> Option<Vec<u8>> {
    (!text.is_empty()).then(|| text.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::ModifiersState;
    use witty_core::{
        MouseEncodingMode, MouseTrackingMode, TerminalCurrentDirectory, TerminalMouseModes,
        TerminalScreen, TerminalShellIntegrationEvent, TerminalShellIntegrationMarker,
    };
    use witty_plugin_api::{
        NetworkPermission, PluginManifest, PluginPermissions, PluginProfileLaunchRequest,
        PluginProfilePickerRequest, PluginRuntime, TerminalReadPermission, TerminalWritePermission,
        VaultPermission,
    };
    use witty_transport::{LocalPtyConfig, MockTransport, SshProfile};
    use witty_ui::{BuiltInPlugin, PendingProfileActionKind};

    #[derive(Default)]
    struct RecordingClipboard {
        clipboard_read: String,
        primary_read: String,
        clipboard_write_error: Option<String>,
        clipboard_text_error: Option<String>,
        clipboard_image: Option<ClipboardImage>,
        writes: Vec<(ClipboardSelection, String)>,
    }

    impl ClipboardSink for RecordingClipboard {
        fn set_text(&mut self, selection: ClipboardSelection, text: &str) -> Result<()> {
            if selection == ClipboardSelection::Clipboard {
                if let Some(error) = &self.clipboard_write_error {
                    bail!("{error}");
                }
            }
            self.writes.push((selection, text.to_owned()));
            Ok(())
        }

        fn get_text(&mut self, selection: ClipboardSelection) -> Result<String> {
            if selection == ClipboardSelection::Clipboard {
                if let Some(error) = &self.clipboard_text_error {
                    bail!("{error}");
                }
            }
            Ok(match selection {
                ClipboardSelection::Clipboard => self.clipboard_read.clone(),
                ClipboardSelection::Primary => self.primary_read.clone(),
            })
        }

        fn get_image(&mut self, selection: ClipboardSelection) -> Result<Option<ClipboardImage>> {
            Ok(match selection {
                ClipboardSelection::Clipboard => self.clipboard_image.clone(),
                ClipboardSelection::Primary => None,
            })
        }
    }

    fn clipboard_write_action(
        selection: TerminalClipboardSelection,
        text: &str,
    ) -> TerminalHostAction {
        TerminalHostAction::ClipboardWrite(TerminalClipboardWrite {
            selection,
            text: text.to_owned(),
            decoded_bytes: text.len(),
        })
    }

    fn terminal_reply_action(bytes: &[u8]) -> TerminalHostAction {
        TerminalHostAction::TerminalReply(witty_core::TerminalHostReply {
            bytes: bytes.to_vec(),
        })
    }

    fn bell_action() -> TerminalHostAction {
        TerminalHostAction::Bell
    }

    #[test]
    fn local_pty_launch_config_defaults_to_shell() {
        let size = GridSize::new(24, 80);

        let config = local_pty_config_for_launch(size, None, Vec::new(), None, Vec::new()).unwrap();

        assert_eq!(config, LocalPtyConfig::new(size));
    }

    #[test]
    fn local_pty_launch_config_applies_cwd_to_default_shell() {
        let size = GridSize::new(24, 80);

        let config = local_pty_config_for_launch(
            size,
            None,
            Vec::new(),
            Some(PathBuf::from("/work/project")),
            Vec::new(),
        )
        .unwrap();

        let mut expected = LocalPtyConfig::new(size);
        expected.cwd("/work/project");
        assert_eq!(config, expected);
    }

    #[test]
    fn local_pty_launch_config_uses_explicit_program_args_and_cwd() {
        let size = GridSize::new(36, 120);

        let config = local_pty_config_for_launch(
            size,
            Some("/bin/zsh".to_owned()),
            vec!["-l".to_owned(), "-c".to_owned()],
            Some(PathBuf::from("/work/project")),
            Vec::new(),
        )
        .unwrap();

        let mut expected = LocalPtyConfig::new(size);
        expected.program = Some("/bin/zsh".to_owned());
        expected.args(["-l", "-c"]);
        expected.cwd("/work/project");
        assert_eq!(config, expected);
    }

    #[test]
    fn local_pty_launch_config_applies_env_and_overrides_default_term() {
        let size = GridSize::new(24, 80);

        let config = local_pty_config_for_launch(
            size,
            None,
            Vec::new(),
            None,
            vec![
                ("TERM".to_owned(), "xterm-witty".to_owned()),
                ("WITTY_SESSION".to_owned(), "daily".to_owned()),
            ],
        )
        .unwrap();

        let mut expected = LocalPtyConfig::new(size);
        expected.env = vec![
            ("TERM".to_owned(), "xterm-witty".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("WITTY_SESSION".to_owned(), "daily".to_owned()),
        ];
        assert_eq!(config, expected);
    }

    #[test]
    fn local_pty_launch_config_rejects_args_without_program() {
        let err = local_pty_config_for_launch(
            GridSize::new(24, 80),
            None,
            vec!["-l".to_owned()],
            Some(PathBuf::from("/work/project")),
            Vec::new(),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "native window launch args require an explicit program"
        );
    }

    fn shell_integration_action(
        marker: TerminalShellIntegrationMarker,
        row: u16,
        col: u16,
        exit_code: Option<i32>,
    ) -> TerminalHostAction {
        TerminalHostAction::ShellIntegration(TerminalShellIntegrationEvent {
            marker,
            screen: TerminalScreen::Main,
            point: CellPoint::new(row, col),
            anchor: None,
            exit_code,
        })
    }

    fn current_directory_action(path: &str) -> TerminalHostAction {
        TerminalHostAction::CurrentDirectory(TerminalCurrentDirectory {
            uri: format!("file://localhost{path}"),
            host: Some("localhost".to_owned()),
            path: path.to_owned(),
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: None,
        })
    }

    fn reply_sink() -> impl FnMut(&[u8]) -> Result<()> {
        |_| Ok(())
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn load_window_font_data_reads_non_empty_files_and_rejects_bad_paths() {
        let root = unique_temp_dir("witty-window-font-data");
        std::fs::create_dir_all(&root).unwrap();
        let font_path = root.join("font.ttf");
        let empty_path = root.join("empty.ttf");
        let missing_path = root.join("missing.ttf");
        std::fs::write(&font_path, [1_u8, 2, 3, 4]).unwrap();
        std::fs::File::create(&empty_path).unwrap();

        assert_eq!(
            load_window_font_data(std::slice::from_ref(&font_path)).unwrap(),
            vec![vec![1_u8, 2, 3, 4]]
        );
        assert!(load_window_font_data(&[empty_path]).is_err());
        assert!(load_window_font_data(&[missing_path]).is_err());

        std::fs::remove_dir_all(&root).unwrap();
    }

    fn physical_position_for_cell(point: CellPoint, metrics: CellMetrics) -> PhysicalPosition<f64> {
        let origin = cell_origin(point, metrics);
        PhysicalPosition::new(f64::from(origin.x) + 1.0, f64::from(origin.y) + 1.0)
    }

    fn test_install_marker(build_id: &str) -> InstalledBuildMarkerV1 {
        InstalledBuildMarkerV1::new(
            build_id,
            "0.1.0",
            "2026-06-08T10:00:00Z",
            "/home/test/.local/bin/witty",
            "/home/test/.local",
            Some("debug".to_owned()),
        )
    }

    #[test]
    fn restart_snapshot_v1_captures_tab_launch_metadata_without_terminal_text() {
        let size = GridSize::new(36, 120);
        let mut sessions = NativeSessionRegistry::default();
        let mut local_config = LocalPtyConfig::new(size);
        local_config.cwd("/work/project");
        local_config.env("WITTY_SESSION", "daily");
        local_config.env("SECRET_TOKEN", "not-stored");
        let local_session =
            native_local_session_metadata(1, NativeProfileActionStartMode::ReplaceCurrentSession);
        let active_id = sessions.replace_current(local_session.clone());
        sessions.set_launch_metadata(
            active_id,
            native_session_launch_metadata_for_local(&local_config, &local_session),
        );

        let mut ssh_config = LocalPtyConfig::command(size, "ssh");
        ssh_config.args(["-tt", "prod.example.com"]);
        let profile_session = NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        };
        let inactive_id = sessions.insert_inactive(profile_session.clone());
        sessions.set_launch_metadata(
            inactive_id,
            NativeSessionLaunchMetadata {
                kind: NativeSessionLaunchKind::ProfileLaunch,
                config: ssh_config,
                source_plugin: profile_session.source_plugin.clone(),
                profile_id: profile_session.profile_id.clone(),
                reason: profile_session.reason.clone(),
            },
        );

        let snapshot = restart_snapshot_v1_for_native_state_at(
            &sessions,
            &local_config,
            size,
            Some(PhysicalSize::new(1280, 720)),
            "old-build",
            Some("new-build"),
            42,
        );

        assert_eq!(snapshot.window.grid_rows, 36);
        assert_eq!(snapshot.window.grid_cols, 120);
        assert_eq!(snapshot.window.inner_width_px, Some(1280));
        assert_eq!(snapshot.window.inner_height_px, Some(720));
        assert_eq!(snapshot.active_tab_index, 0);
        assert_eq!(snapshot.tabs.len(), 2);
        assert!(snapshot.tabs[0].active);
        assert_eq!(snapshot.tabs[0].kind, RestartTabKindV1::Local);
        assert_eq!(
            snapshot.tabs[0].launch.cwd,
            Some(PathBuf::from("/work/project"))
        );
        assert!(snapshot.tabs[0]
            .launch
            .env
            .iter()
            .any(|entry| entry.key == "WITTY_SESSION" && entry.value.as_deref() == Some("daily")));
        assert!(snapshot.tabs[0]
            .launch
            .env
            .iter()
            .any(|entry| entry.key == "SECRET_TOKEN"
                && entry.value.is_none()
                && entry.redacted
                && !entry.restored));
        assert_eq!(
            snapshot.tabs[1].profile.as_ref().unwrap().profile_id,
            "prod"
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("not-stored"));
        assert!(!json.contains("old-visible-terminal-text"));
    }

    #[test]
    fn restart_snapshot_v1_synthesizes_single_local_tab_when_registry_is_empty() {
        let size = GridSize::new(24, 80);
        let mut local_config = LocalPtyConfig::new(size);
        local_config.program = Some("/bin/zsh".to_owned());
        local_config.args(["-l"]);

        let snapshot = restart_snapshot_v1_for_native_state_at(
            &NativeSessionRegistry::default(),
            &local_config,
            size,
            None,
            "running",
            None,
            7,
        );

        assert_eq!(snapshot.active_tab_index, 0);
        assert_eq!(snapshot.tabs.len(), 1);
        assert!(snapshot.tabs[0].active);
        assert_eq!(
            snapshot.tabs[0].source_plugin,
            NATIVE_LOCAL_SESSION_SOURCE_PLUGIN
        );
        assert_eq!(snapshot.tabs[0].launch.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(snapshot.tabs[0].launch.args, vec!["-l"]);
    }

    #[test]
    fn native_update_notice_overlay_renders_restart_button_and_hit_target() {
        let metrics = CellMetrics::default();
        let size = GridSize::new(6, 96);
        let notice = NativeUpdateNotice {
            running_build_id: "old-build-id-123456".to_owned(),
            installed_marker: test_install_marker("new-build-id-abcdef"),
        };
        let mut frame = FramePlan {
            glyphs: vec![GlyphBatchItem {
                origin: cell_origin(CellPoint::new(5, 1), metrics),
                text: "covered terminal text".to_owned(),
                color: Rgba::rgb(255, 255, 255),
                style_flags: CellFlags::default(),
            }],
            ..FramePlan::default()
        };

        apply_native_update_notice_overlay(&mut frame, Some(&notice), None, metrics, size);

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains(RESTART_BUTTON_LABEL)));
        assert!(!frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("covered terminal text")));

        let width = size.cols.saturating_sub(2);
        let button_width = text_cell_width(&native_update_notice_button_text());
        let restart_cell = CellPoint::new(
            size.rows.saturating_sub(1),
            1 + width.saturating_sub(button_width),
        );
        let hit = native_update_notice_hit_for_position(
            Some(&notice),
            physical_position_for_cell(restart_cell, metrics),
            metrics,
            size,
        )
        .unwrap();
        assert_eq!(hit.target, NativeUpdateNoticeTarget::Restart);
    }

    struct ProfileBridgePlugin;

    impl BuiltInPlugin for ProfileBridgePlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "profile-bridge".to_owned(),
                name: "Profile Bridge".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::None,
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: true,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![
                CommandRegistration {
                    id: "profile-bridge.pick".to_owned(),
                    title: "Pick Profile".to_owned(),
                    source_plugin: "profile-bridge".to_owned(),
                },
                CommandRegistration {
                    id: "profile-bridge.launch".to_owned(),
                    title: "Launch Profile".to_owned(),
                    source_plugin: "profile-bridge".to_owned(),
                },
            ]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            match invocation.command_id.as_str() {
                "profile-bridge.pick" => Ok(vec![PluginAction::RequestProfilePicker(
                    PluginProfilePickerRequest {
                        reason: Some("choose profile".to_owned()),
                    },
                )]),
                "profile-bridge.launch" => Ok(vec![PluginAction::RequestProfileLaunch(
                    PluginProfileLaunchRequest {
                        profile_id: "prod".to_owned(),
                        reason: Some("open production".to_owned()),
                    },
                )]),
                _ => Ok(Vec::new()),
            }
        }
    }

    fn profile_bridge_app() -> TerminalApp<MockTransport> {
        let size = GridSize::new(24, 80);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.install_builtin_plugin(ProfileBridgePlugin).unwrap();
        app.invoke_command("profile-bridge.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profile-bridge.launch", serde_json::Value::Null)
            .unwrap();
        app
    }

    fn profile_bridge_store() -> ProfileStoreV1 {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tags = vec!["work".to_owned()];
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vaulted.example.com");
        vaulted.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-prod".to_owned(),
        };
        ProfileStoreV1::with_profiles(vec![prod, vaulted])
    }

    fn test_profile_action_handoff(
        key: PendingProfileActionKey,
        kind: NativeResolvedProfileActionKind,
        profile_id: &str,
        reason: &str,
        host: &str,
    ) -> NativeResolvedProfileActionHandoff {
        let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
        config.args(["-tt", host]);
        NativeResolvedProfileActionHandoff {
            key,
            kind,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: profile_id.to_owned(),
            reason: Some(reason.to_owned()),
            config,
        }
    }

    fn test_session_tab_rows() -> Vec<NativeSessionTabRow> {
        (0..3)
            .map(|index| NativeSessionTabRow {
                session_id: NativeSessionId((index + 1) as u64),
                key: PendingProfileActionKey::profile_launch(index),
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: format!("local-{}", index + 1),
                mode: NativeProfileActionStartMode::NewTab,
                active: index == 0,
            })
            .collect()
    }

    fn test_current_directory(path: &str) -> TerminalCurrentDirectory {
        TerminalCurrentDirectory {
            uri: format!("file://localhost{path}"),
            host: Some("localhost".to_owned()),
            path: path.to_owned(),
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: None,
        }
    }

    #[test]
    fn grid_size_tracks_cell_metrics() {
        let size = grid_size_for_window(PhysicalSize::new(98, 44), CellMetrics::default());

        assert_eq!(size, GridSize::new(2, 10));
    }

    #[test]
    fn terminal_bottom_align_y_offset_moves_partial_row_gap_above_grid() {
        let offset = terminal_bottom_align_y_offset(
            PhysicalSize::new(98, 44),
            CellMetrics::default(),
            GridSize::new(2, 10),
            0.0,
        );

        assert_eq!(offset, 8.0);
    }

    #[test]
    fn terminal_bottom_align_y_offset_accounts_for_reserved_titlebar_height() {
        let offset = terminal_bottom_align_y_offset(
            PhysicalSize::new(98, 80),
            CellMetrics::default(),
            GridSize::new(2, 10),
            34.0,
        );

        assert_eq!(offset, 10.0);
    }

    #[test]
    fn terminal_window_initial_inner_size_preserves_default_or_uses_grid_metrics() {
        assert_eq!(
            terminal_window_initial_inner_size(None, CellMetrics::default()),
            LogicalSize::new(960.0, 540.0)
        );
        assert_eq!(
            terminal_window_initial_inner_size(
                Some(GridSize::new(36, 120)),
                CellMetrics::default()
            ),
            LogicalSize::new(1080.0, 682.0)
        );
    }

    #[test]
    fn titlebar_menu_hit_test_maps_menu_rows() {
        let scale_factor = 1.0;
        let first_row = PixelPoint {
            x: native_titlebar_edge_padding(scale_factor) + 8.0,
            y: native_titlebar_height(scale_factor) + 8.0,
        };
        let second_row = PixelPoint {
            y: first_row.y + native_titlebar_menu_item_height(scale_factor),
            ..first_row
        };

        assert_eq!(
            native_titlebar_menu_item_for_point(first_row, scale_factor),
            Some(NativeTitleBarMenuItem::NewSession)
        );
        assert_eq!(
            native_titlebar_menu_item_for_point(second_row, scale_factor),
            Some(NativeTitleBarMenuItem::CommandPalette)
        );
        assert_eq!(
            native_titlebar_menu_item_for_point(PixelPoint { x: 0.0, y: 0.0 }, scale_factor),
            None
        );
    }

    #[test]
    fn titlebar_button_rects_place_menu_left_and_close_right() {
        let scale_factor = 1.0;
        let menu =
            native_titlebar_button_rect(NativeTitleBarButton::Menu, 640.0, scale_factor).unwrap();
        let new_session =
            native_titlebar_button_rect(NativeTitleBarButton::NewSession, 640.0, scale_factor)
                .unwrap();
        let close =
            native_titlebar_button_rect(NativeTitleBarButton::Close, 640.0, scale_factor).unwrap();

        assert_eq!(menu.origin.x, native_titlebar_edge_padding(scale_factor));
        assert!(new_session.origin.x > menu.origin.x);
        assert!(new_session.origin.x < close.origin.x);
        assert!(close.origin.x > menu.origin.x);
        assert_eq!(menu.size.width, native_titlebar_button_size(scale_factor));
        assert_eq!(
            new_session.size.width,
            native_titlebar_button_size(scale_factor)
        );
        assert_eq!(close.size.height, native_titlebar_button_size(scale_factor));
    }

    #[test]
    fn titlebar_geometry_scales_for_hidpi() {
        let scale_factor = 2.0;
        let menu =
            native_titlebar_button_rect(NativeTitleBarButton::Menu, 1280.0, scale_factor).unwrap();
        let new_session =
            native_titlebar_button_rect(NativeTitleBarButton::NewSession, 1280.0, scale_factor)
                .unwrap();
        let menu_panel = native_titlebar_menu_rect(scale_factor);

        assert_eq!(native_titlebar_height(scale_factor), 68.0);
        assert_eq!(menu.origin.x, 12.0);
        assert_eq!(new_session.origin.x, 76.0);
        assert_eq!(menu.size.width, 56.0);
        assert_eq!(menu_panel.origin.y, 68.0);
        assert_eq!(menu_panel.size.height, 240.0);
    }

    #[test]
    fn terminal_window_title_uses_terminal_title_and_default() {
        assert_eq!(
            terminal_window_title(None, DEFAULT_WINDOW_TITLE),
            DEFAULT_WINDOW_TITLE
        );
        assert_eq!(
            terminal_window_title(Some(""), "Project Shell"),
            "Project Shell"
        );
        assert_eq!(
            terminal_window_title(Some("cargo test"), "Project Shell"),
            "cargo test"
        );
    }

    #[test]
    fn native_window_startup_report_names_opengl_policy_without_vulkan() {
        let report = native_window_startup_report_json(
            PhysicalSize::new(960, 540),
            NativeActiveSessionCloseFallbackPolicy::Block,
            &RendererFontConfig::default(),
            &RendererVisualConfig::default(),
            0,
        );

        assert_eq!(report["event"], "witty.native_window_startup");
        assert_eq!(report["renderer"], "wgpu");
        assert_eq!(report["last_active_close_policy"], "block");
        assert_eq!(report["surface_width"], 960);
        assert_eq!(report["surface_height"], 540);
        assert!(report["font_family"].is_null());
        assert_eq!(report["font_size"], 14);
        assert_eq!(report["terminal_padding"], 0.0);
        assert_eq!(report["background_opacity"], 1.0);
        assert_eq!(report["background_image"], false);
        assert_eq!(report["background_image_fit"], "cover");
        assert_eq!(report["transparent_window"], false);
        assert_eq!(report["font_source_count"], 0);
        assert_eq!(report["will_request_adapter"], true);
        assert_eq!(report["vulkan_enabled_by_witty"], false);
        assert_eq!(report["chromium"], false);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(report["native_backend_policy"], "gl");
            assert_eq!(report["opengl_only"], true);
            assert_eq!(report["honors_wgpu_backend_env"], false);
        }
    }

    #[test]
    fn native_window_first_frame_report_names_frame_stats_and_opengl_policy() {
        let font_config =
            RendererFontConfig::with_font_size(Some("JetBrainsMono Nerd Font".to_owned()), 18);
        let report = native_window_first_frame_report_json(
            PhysicalSize::new(960, 540),
            FrameStats {
                visible_rows: 24,
                visible_cols: 80,
                glyph_runs: 3,
                glyph_chars: 42,
                rect_vertices: 120,
                cursor_visible: true,
                full_damage: true,
                damage_regions: 1,
                ..FrameStats::default()
            },
            &font_config,
            &RendererVisualConfig::default(),
            2,
        );

        assert_eq!(report["event"], "witty.native_window_first_frame");
        assert_eq!(report["renderer"], "wgpu");
        assert_eq!(report["surface_width"], 960);
        assert_eq!(report["surface_height"], 540);
        assert_eq!(report["font_family"], "JetBrainsMono Nerd Font");
        assert_eq!(report["font_size"], 18);
        assert_eq!(report["terminal_padding"], 0.0);
        assert_eq!(report["background_opacity"], 1.0);
        assert_eq!(report["background_image"], false);
        assert_eq!(report["background_image_fit"], "cover");
        assert_eq!(report["transparent_window"], false);
        assert_eq!(report["font_source_count"], 2);
        assert_eq!(report["visible_rows"], 24);
        assert_eq!(report["visible_cols"], 80);
        assert_eq!(report["glyph_runs"], 3);
        assert_eq!(report["glyph_chars"], 42);
        assert_eq!(report["rect_vertices"], 120);
        assert_eq!(report["cursor_visible"], true);
        assert_eq!(report["full_damage"], true);
        assert_eq!(report["damage_regions"], 1);
        assert_eq!(report["vulkan_enabled_by_witty"], false);
        assert_eq!(report["chromium"], false);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(report["native_backend_policy"], "gl");
            assert_eq!(report["opengl_only"], true);
            assert_eq!(report["honors_wgpu_backend_env"], false);
        }
    }

    #[test]
    fn native_window_startup_report_names_selected_last_active_close_policy_only() {
        let report = native_window_startup_report_json(
            PhysicalSize::new(960, 540),
            NativeActiveSessionCloseFallbackPolicy::FallbackLocalSession,
            &RendererFontConfig::default(),
            &RendererVisualConfig::default(),
            0,
        );
        let line = native_window_startup_report_line(
            PhysicalSize::new(960, 540),
            NativeActiveSessionCloseFallbackPolicy::FallbackLocalSession,
            &RendererFontConfig::default(),
            &RendererVisualConfig::default(),
            0,
        );

        assert_eq!(
            report["last_active_close_policy"],
            WindowLastActiveClosePolicy::FallbackLocalSession.as_config_value()
        );
        assert!(line.contains("\"last_active_close_policy\":\"fallback-local-session\""));
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !line.contains(hidden),
                "leaked startup report policy detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_window_startup_report_policy_values_match_config_values() {
        for window_policy in WindowLastActiveClosePolicy::all() {
            let policy = NativeActiveSessionCloseFallbackPolicy::from(*window_policy);
            let report = native_window_startup_report_json(
                PhysicalSize::new(960, 540),
                policy,
                &RendererFontConfig::default(),
                &RendererVisualConfig::default(),
                0,
            );

            assert_eq!(
                report["last_active_close_policy"],
                window_policy.as_config_value()
            );
        }
    }

    #[test]
    fn native_window_startup_report_names_configured_font_metadata() {
        let font_config =
            RendererFontConfig::with_font_size(Some("Symbols Nerd Font Mono".to_owned()), 18)
                .with_terminal_padding(4.0);
        let visual_config = RendererVisualConfig::default().with_background_opacity(0.72);
        let report = native_window_startup_report_json(
            PhysicalSize::new(960, 540),
            NativeActiveSessionCloseFallbackPolicy::Block,
            &font_config,
            &visual_config,
            2,
        );

        assert_eq!(report["font_family"], "Symbols Nerd Font Mono");
        assert_eq!(report["font_size"], 18);
        assert_eq!(report["terminal_padding"], 4.0);
        assert!((report["background_opacity"].as_f64().unwrap() - 0.72).abs() < 0.001);
        assert_eq!(report["background_image_fit"], "cover");
        assert_eq!(report["transparent_window"], true);
        assert_eq!(report["font_source_count"], 2);
    }

    #[test]
    fn native_renderer_startup_error_mentions_backend_policy() {
        let error = anyhow::anyhow!("adapter unavailable");
        let message = native_renderer_startup_error_message(&error);

        assert!(message.contains("failed to initialize wgpu renderer"));
        assert!(message.contains("native_backend_policy="));
        assert!(message.contains("vulkan_enabled_by_witty=false"));
        assert!(message.contains("adapter unavailable"));
        #[cfg(target_os = "linux")]
        {
            assert!(message.contains("native_backend_policy=gl"));
            assert!(message.contains("opengl_only=true"));
        }
    }

    #[test]
    fn native_profile_action_bridge_refreshes_snapshot() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();

        let event = bridge.refresh(&app, &store).unwrap();

        let NativeProfileActionBridgeEvent::SnapshotRefreshed(snapshot) = event else {
            panic!("expected snapshot refresh event");
        };
        assert_eq!(snapshot.picker_requests, 1);
        assert_eq!(snapshot.launch_requests, 1);
        assert_eq!(snapshot.reviews.len(), 2);
        assert_eq!(snapshot.display_rows.len(), 2);
        assert_eq!(snapshot.picker_options.len(), 2);
        assert!(matches!(
            snapshot.reviews[0],
            PendingProfileActionReview::ProfilePicker {
                key: PendingProfileActionKey {
                    kind: PendingProfileActionKind::ProfilePicker,
                    request_index: 0,
                },
                ..
            }
        ));
        assert!(matches!(
            snapshot.reviews[1],
            PendingProfileActionReview::ProfileLaunch {
                key: PendingProfileActionKey {
                    kind: PendingProfileActionKind::ProfileLaunch,
                    request_index: 0,
                },
                ..
            }
        ));
        assert_eq!(
            snapshot.display_rows[0],
            NativeProfileActionDisplayRow {
                key: PendingProfileActionKey::profile_picker(0),
                source_plugin: "profile-bridge".to_owned(),
                title: "Choose SSH profile".to_owned(),
                detail: "2 profiles available, 1 launchable, 1 require credentials".to_owned(),
                reason: Some("choose profile".to_owned()),
                status: NativeProfileActionDisplayStatus::PickProfile,
                confirm_label: Some("Choose".to_owned()),
                new_tab_label: None,
                dismiss_label: "Dismiss".to_owned(),
            }
        );
        assert_eq!(
            snapshot.display_rows[1],
            NativeProfileActionDisplayRow {
                key: PendingProfileActionKey::profile_launch(0),
                source_plugin: "profile-bridge".to_owned(),
                title: "Launch Production".to_owned(),
                detail: "id=prod default=false tags=work".to_owned(),
                reason: Some("open production".to_owned()),
                status: NativeProfileActionDisplayStatus::Launchable,
                confirm_label: Some("Launch".to_owned()),
                new_tab_label: Some("New Tab".to_owned()),
                dismiss_label: "Dismiss".to_owned(),
            }
        );
        assert_eq!(
            snapshot.picker_options[0],
            NativeProfilePickerOptionRow {
                request_key: PendingProfileActionKey::profile_picker(0),
                profile_id: "prod".to_owned(),
                title: "Production".to_owned(),
                detail: "id=prod default=false tags=work".to_owned(),
                status: NativeProfilePickerOptionStatus::Launchable,
                select_label: Some("Select".to_owned()),
                new_tab_label: Some("New Tab".to_owned()),
            }
        );
        assert_eq!(
            snapshot.picker_options[1],
            NativeProfilePickerOptionRow {
                request_key: PendingProfileActionKey::profile_picker(0),
                profile_id: "vaulted".to_owned(),
                title: "Vaulted".to_owned(),
                detail: "id=vaulted default=false tags=-".to_owned(),
                status: NativeProfilePickerOptionStatus::RequiresCredentialResolver,
                select_label: None,
                new_tab_label: None,
            }
        );
        assert_eq!(bridge.snapshot(), &snapshot);
    }

    #[test]
    fn native_profile_action_display_rows_disable_missing_launch_requests() {
        let app = profile_bridge_app();
        let snapshot = native_profile_action_snapshot(&app, &ProfileStoreV1::new()).unwrap();

        let row = &snapshot.display_rows[1];

        assert_eq!(row.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(row.status, NativeProfileActionDisplayStatus::NotFound);
        assert_eq!(row.title, "Launch prod");
        assert_eq!(row.detail, "id=prod default=false tags=-");
        assert_eq!(row.confirm_label, None);
        assert_eq!(row.dismiss_label, "Dismiss");
    }

    #[test]
    fn native_profile_action_display_rows_disable_resolver_required_launch_requests() {
        let app = profile_bridge_app();
        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.credential = witty_transport::SshCredentialRef::VaultSecret {
            secret_id: "vault-prod".to_owned(),
        };
        let store = ProfileStoreV1::with_profiles(vec![profile]);

        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let row = &snapshot.display_rows[1];

        assert_eq!(
            row.status,
            NativeProfileActionDisplayStatus::RequiresCredentialResolver
        );
        assert_eq!(row.title, "Launch Production");
        assert_eq!(row.confirm_label, None);
        assert_eq!(row.reason.as_deref(), Some("open production"));
    }

    #[test]
    fn pending_profile_action_feedback_summarizes_native_snapshot_counts() {
        let snapshot = NativeProfileActionSnapshot {
            reviews: Vec::new(),
            display_rows: Vec::new(),
            start_success: None,
            start_failure: None,
            picker_options: Vec::new(),
            picker_requests: 2,
            launch_requests: 1,
        };

        assert_eq!(
            pending_profile_action_feedback(&snapshot).as_deref(),
            Some("\r\n[profile action pending: picker=2 launch=1]\r\n")
        );
    }

    #[test]
    fn pending_profile_action_feedback_skips_empty_snapshot() {
        assert_eq!(
            pending_profile_action_feedback(&NativeProfileActionSnapshot::default()),
            None
        );
    }

    #[test]
    fn pending_profile_action_feedback_omits_profile_review_details() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();

        let feedback = pending_profile_action_feedback(&snapshot).unwrap();

        assert_eq!(
            feedback,
            "\r\n[profile action pending: picker=1 launch=1]\r\n"
        );
        for hidden in [
            "prod",
            "Production",
            "Vaulted",
            "choose profile",
            "open production",
            "work",
            "vaulted",
            "vault-prod",
            "launchable",
            "resolver",
            "prod.example.com",
            "vaulted.example.com",
        ] {
            assert!(!feedback.contains(hidden), "leaked {hidden:?}");
        }
    }

    #[test]
    fn read_profile_store_snapshot_or_empty_returns_empty_for_missing_file() {
        let root = unique_temp_dir("witty-window-missing-profile-store");
        let store = read_profile_store_snapshot_or_empty(&root.join("profiles.v1.json")).unwrap();

        assert!(store.profiles.is_empty());
        assert_eq!(store.default_profile_id, None);
    }

    #[test]
    fn read_profile_store_snapshot_or_empty_reads_existing_store() {
        let root = unique_temp_dir("witty-window-profile-store");
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("profiles.v1.json");
        let mut store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);
        store.default_profile_id = Some("prod".to_owned());
        std::fs::write(&store_path, store.to_pretty_json().unwrap()).unwrap();

        let loaded = read_profile_store_snapshot_or_empty(&store_path).unwrap();

        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].id, "prod");
        assert_eq!(loaded.default_profile_id.as_deref(), Some("prod"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_profile_action_bridge_dismisses_without_launching() {
        let mut app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();
        bridge.refresh(&app, &store).unwrap();

        let event = bridge
            .dismiss(&mut app, &store, PendingProfileActionKey::profile_picker(0))
            .unwrap();

        let NativeProfileActionBridgeEvent::Dismissed {
            dismissed,
            snapshot,
        } = event
        else {
            panic!("expected dismiss event");
        };
        assert!(matches!(
            dismissed,
            DismissedPendingProfileAction::ProfilePicker {
                key: PendingProfileActionKey {
                    kind: PendingProfileActionKind::ProfilePicker,
                    request_index: 0,
                },
                ..
            }
        ));
        assert_eq!(snapshot.picker_requests, 0);
        assert_eq!(snapshot.launch_requests, 1);
        assert!(app.profile_picker_requests().is_empty());
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[test]
    fn native_profile_action_bridge_confirms_to_pty_config_without_launching() {
        let mut app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();
        bridge.refresh(&app, &store).unwrap();

        let event = bridge
            .confirm(
                &mut app,
                &store,
                PendingProfileActionConfirmation::profile_picker(
                    PendingProfileActionKey::profile_picker(0),
                    "prod",
                ),
                GridSize::new(24, 80),
            )
            .unwrap();

        let NativeProfileActionBridgeEvent::Confirmed { resolved, snapshot } = event else {
            panic!("expected confirmed event");
        };
        let ResolvedPendingProfileActionPtyConfig::ProfilePicker { key, resolved } = resolved
        else {
            panic!("expected picker confirmation");
        };
        assert_eq!(key, PendingProfileActionKey::profile_picker(0));
        assert_eq!(resolved.source_plugin, "profile-bridge");
        assert_eq!(resolved.profile_id, "prod");
        assert_eq!(resolved.reason.as_deref(), Some("choose profile"));
        let mut expected = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
        expected.env("TERM", "xterm-256color");
        expected.args(["-tt", "prod.example.com"]);
        assert_eq!(resolved.config, expected);
        assert_eq!(snapshot.picker_requests, 0);
        assert_eq!(snapshot.launch_requests, 1);
        assert!(app.profile_picker_requests().is_empty());
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[test]
    fn native_profile_action_bridge_confirms_launch_to_pty_config_without_launching() {
        let mut app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();
        bridge.refresh(&app, &store).unwrap();

        let event = bridge
            .confirm(
                &mut app,
                &store,
                PendingProfileActionConfirmation::profile_launch(
                    PendingProfileActionKey::profile_launch(0),
                ),
                GridSize::new(24, 80),
            )
            .unwrap();

        let NativeProfileActionBridgeEvent::Confirmed { resolved, snapshot } = event else {
            panic!("expected confirmed event");
        };
        let ResolvedPendingProfileActionPtyConfig::ProfileLaunch { key, resolved } = resolved
        else {
            panic!("expected launch confirmation");
        };
        assert_eq!(key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(resolved.source_plugin, "profile-bridge");
        assert_eq!(resolved.profile_id, "prod");
        assert_eq!(resolved.reason.as_deref(), Some("open production"));
        let mut expected = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
        expected.env("TERM", "xterm-256color");
        expected.args(["-tt", "prod.example.com"]);
        assert_eq!(resolved.config, expected);
        assert_eq!(snapshot.picker_requests, 1);
        assert_eq!(snapshot.launch_requests, 0);
        assert_eq!(app.profile_picker_requests().len(), 1);
        assert!(app.profile_launch_requests().is_empty());
    }

    #[test]
    fn native_resolved_profile_action_handoff_keeps_config_in_trusted_state() {
        let mut app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();
        bridge.refresh(&app, &store).unwrap();

        let event = bridge
            .confirm(
                &mut app,
                &store,
                PendingProfileActionConfirmation::profile_picker(
                    PendingProfileActionKey::profile_picker(0),
                    "prod",
                ),
                GridSize::new(24, 80),
            )
            .unwrap();

        let NativeProfileActionBridgeEvent::Confirmed { resolved, snapshot } = event else {
            panic!("expected confirmed event");
        };
        let handoff = native_resolved_profile_action_handoff(resolved);

        assert_eq!(handoff.key, PendingProfileActionKey::profile_picker(0));
        assert_eq!(handoff.kind, NativeResolvedProfileActionKind::ProfilePicker);
        assert_eq!(handoff.source_plugin, "profile-bridge");
        assert_eq!(handoff.profile_id, "prod");
        assert_eq!(handoff.reason.as_deref(), Some("choose profile"));
        let mut expected = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
        expected.env("TERM", "xterm-256color");
        expected.args(["-tt", "prod.example.com"]);
        assert_eq!(handoff.config, expected);

        let feedback = pending_profile_action_feedback(&snapshot).unwrap();
        assert_eq!(feedback, "\r\n[profile action pending: launch=1]\r\n");
        for hidden in ["prod", "Production", "prod.example.com", "choose profile"] {
            assert!(!feedback.contains(hidden), "leaked {hidden:?}");
        }
    }

    #[test]
    fn native_resolved_profile_action_handoff_queue_consumes_fifo() {
        let first = test_profile_action_handoff(
            PendingProfileActionKey::profile_picker(0),
            NativeResolvedProfileActionKind::ProfilePicker,
            "prod",
            "choose profile",
            "prod.example.com",
        );
        let second = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "staging",
            "open staging",
            "staging.example.com",
        );
        let mut queue = NativeResolvedProfileActionHandoffQueue::default();

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.take_next(), None);

        queue.push(first.clone());
        queue.push(second.clone());

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.as_slice(), &[first.clone(), second.clone()]);
        assert_eq!(queue.take_next(), Some(first));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.take_next(), Some(second));
        assert!(queue.is_empty());
        assert_eq!(queue.take_next(), None);
    }

    #[test]
    fn native_resolved_profile_action_defer_policy_moves_next_handoff_without_launching() {
        let first = test_profile_action_handoff(
            PendingProfileActionKey::profile_picker(0),
            NativeResolvedProfileActionKind::ProfilePicker,
            "prod",
            "choose profile",
            "prod.example.com",
        );
        let second = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "staging",
            "open staging",
            "staging.example.com",
        );
        let mut handoffs = NativeResolvedProfileActionHandoffQueue::default();
        let mut deferred_starts = NativeResolvedProfileActionHandoffQueue::default();

        assert!(!apply_native_resolved_profile_action_session_policy(
            &mut handoffs,
            &mut deferred_starts,
            NativeResolvedProfileActionSessionPolicy::DeferStart,
        ));

        handoffs.push(first.clone());
        handoffs.push(second.clone());

        assert!(apply_native_resolved_profile_action_session_policy(
            &mut handoffs,
            &mut deferred_starts,
            NativeResolvedProfileActionSessionPolicy::DeferStart,
        ));
        assert_eq!(handoffs.as_slice(), &[second.clone()]);
        assert_eq!(deferred_starts.as_slice(), &[first.clone()]);

        assert!(apply_native_resolved_profile_action_session_policy(
            &mut handoffs,
            &mut deferred_starts,
            NativeResolvedProfileActionSessionPolicy::DeferStart,
        ));
        assert!(handoffs.is_empty());
        assert_eq!(deferred_starts.as_slice(), &[first, second]);
    }

    #[test]
    fn native_profile_action_start_plan_preserves_deferred_config_without_spawning() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let mut deferred_starts = NativeResolvedProfileActionHandoffQueue::default();
        let mut start_plans = NativeProfileActionStartPlanQueue::default();

        assert!(!plan_next_native_profile_action_start(
            &mut deferred_starts,
            &mut start_plans,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        ));

        deferred_starts.push(handoff.clone());

        assert!(plan_next_native_profile_action_start(
            &mut deferred_starts,
            &mut start_plans,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        ));
        assert!(deferred_starts.is_empty());
        assert_eq!(
            start_plans.as_slice(),
            &[NativeProfileActionStartPlan {
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                key: handoff.key,
                kind: handoff.kind,
                source_plugin: handoff.source_plugin,
                profile_id: handoff.profile_id,
                reason: handoff.reason,
                config: handoff.config,
            }]
        );
    }

    #[test]
    fn native_profile_action_default_policy_advances_to_replace_current_plan_without_spawning() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_picker(0),
            NativeResolvedProfileActionKind::ProfilePicker,
            "prod",
            "choose profile",
            "prod.example.com",
        );
        let mut handoffs = NativeResolvedProfileActionHandoffQueue::default();
        let mut deferred_starts = NativeResolvedProfileActionHandoffQueue::default();
        let mut start_plans = NativeProfileActionStartPlanQueue::default();

        handoffs.push(handoff.clone());
        assert!(apply_native_resolved_profile_action_session_policy(
            &mut handoffs,
            &mut deferred_starts,
            NativeResolvedProfileActionSessionPolicy::DeferStart,
        ));
        assert!(plan_next_native_profile_action_start(
            &mut deferred_starts,
            &mut start_plans,
            NativeProfileActionStartMode::default(),
        ));

        assert!(handoffs.is_empty());
        assert!(deferred_starts.is_empty());
        assert_eq!(
            start_plans.as_slice(),
            &[NativeProfileActionStartPlan {
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                key: handoff.key,
                kind: handoff.kind,
                source_plugin: handoff.source_plugin,
                profile_id: handoff.profile_id,
                reason: handoff.reason,
                config: handoff.config,
            }]
        );
    }

    #[test]
    fn native_profile_action_start_execution_replaces_transport_and_resets_session_state() {
        let size = GridSize::new(3, 16);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.register_command(CommandRegistration {
            id: "native.keep".to_owned(),
            title: "Keep".to_owned(),
            source_plugin: "native".to_owned(),
        })
        .unwrap();
        app.write_input(b"old-session").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 7);
        terminal.feed(b"old-visible");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 2),
        }));
        assert_eq!(terminal.selected_text().as_deref(), Some("old"));

        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("old"));
        assert!(terminal_search.is_open());
        assert_eq!(terminal_search.query(), "old");
        assert_eq!(terminal_search.match_count(), 1);

        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(TerminalCurrentDirectory {
            uri: "file://localhost/home/mingxu/old".to_owned(),
            host: Some("localhost".to_owned()),
            path: "/home/mingxu/old".to_owned(),
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: None,
        });
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|dir| dir.path.as_str()),
            Some("/home/mingxu/old")
        );

        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let execution = apply_native_profile_action_start_plan_with_transport(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            plan.clone(),
            MockTransport::new(size),
            size,
        );

        assert_eq!(execution, NativeProfileActionStartExecution { plan });
        assert_eq!(app.commands()[0].id, "native.keep");
        assert!(app.transport().written().is_empty());
        app.write_input(b"new-session").unwrap();
        assert_eq!(app.transport().written(), b"new-session");

        assert_eq!(terminal.max_scrollback_lines(), 7);
        assert_eq!(terminal.selected_text(), None);
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("old-visible")));
        assert!(!terminal_search.is_open());
        assert_eq!(terminal_search.query(), "");
        assert_eq!(terminal_search.match_count(), 0);
        assert_eq!(shell_integration.current_directory(), None);
        assert!(shell_integration.completed_blocks().is_empty());
        assert_eq!(sessions.active().unwrap().id, NativeSessionId(1));
        assert_eq!(sessions.active().unwrap().profile_action.profile_id, "prod");
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_profile_action_start_spawner_consumes_plan_after_transport_ready() {
        let size = GridSize::new(3, 16);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        let mut terminal_search = TerminalSearch::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        let mut start_plans = NativeProfileActionStartPlanQueue::default();
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        start_plans.push(plan.clone());

        let execution = apply_next_native_profile_action_start_plan_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            &mut start_plans,
            |config| Ok(MockTransport::new(config.size)),
            size,
        )
        .unwrap();

        assert_eq!(execution, Some(NativeProfileActionStartExecution { plan }));
        assert!(start_plans.as_slice().is_empty());
        app.write_input(b"new-session").unwrap();
        assert_eq!(app.transport().written(), b"new-session");
        assert_eq!(sessions.active().unwrap().profile_action.profile_id, "prod");
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_profile_action_start_spawner_keeps_plan_when_spawn_fails() {
        let size = GridSize::new(3, 16);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"old-session").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        terminal.feed(b"old-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("old"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(TerminalCurrentDirectory {
            uri: "file://localhost/home/mingxu/old".to_owned(),
            host: Some("localhost".to_owned()),
            path: "/home/mingxu/old".to_owned(),
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: None,
        });
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let mut start_plans = NativeProfileActionStartPlanQueue::default();
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        start_plans.push(plan.clone());

        let err = apply_next_native_profile_action_start_plan_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            &mut start_plans,
            |_config| Err(anyhow::anyhow!("spawn unavailable")),
            size,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "spawn unavailable");
        assert_eq!(start_plans.as_slice(), &[plan]);
        assert_eq!(app.transport().written(), b"old-session");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("old-visible")));
        assert!(terminal_search.is_open());
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|dir| dir.path.as_str()),
            Some("/home/mingxu/old")
        );
        assert!(sessions.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn fallback_local_session_spawner_replaces_transport_and_clears_profile_action_sessions() {
        let size = GridSize::new(3, 18);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.register_command(CommandRegistration {
            id: "native.keep".to_owned(),
            title: "Keep".to_owned(),
            source_plugin: "native".to_owned(),
        })
        .unwrap();
        app.write_input(b"old-session").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 13);
        terminal.feed(b"prod.example.com old-visible");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 3),
        }));
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("prod.example.com"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/ssh/prod.example.com"));

        let mut sessions = NativeSessionRegistry::default();
        let active_id = sessions.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let inactive_id = sessions.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "stage".to_owned(),
            reason: Some("open staging".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            active_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: BasicTerminal::with_scrollback_limit(size, 3),
                terminal_search: TerminalSearch::default(),
                shell_integration: ShellIntegrationState::default(),
            },
        );
        parked_sessions.park_or_replace(
            inactive_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: BasicTerminal::with_scrollback_limit(size, 5),
                terminal_search: TerminalSearch::default(),
                shell_integration: ShellIntegrationState::default(),
            },
        );

        apply_fallback_local_session_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            |spawn_size| {
                assert_eq!(spawn_size, size);
                Ok(MockTransport::new(spawn_size))
            },
            size,
        )
        .unwrap();

        assert_eq!(app.commands()[0].id, "native.keep");
        assert!(app.transport().written().is_empty());
        app.write_input(b"new-local").unwrap();
        assert_eq!(app.transport().written(), b"new-local");
        assert_eq!(app.transport().size(), size);

        assert_eq!(terminal.max_scrollback_lines(), 13);
        assert_eq!(terminal.selected_text(), None);
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("old-visible")));
        assert!(!terminal_search.is_open());
        assert_eq!(terminal_search.query(), "");
        assert_eq!(terminal_search.match_count(), 0);
        assert_eq!(shell_integration.current_directory(), None);
        assert!(sessions.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());

        let visible_text = terminal
            .search_text_rows()
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let trusted_state_debug = format!(
            "{visible_text} {terminal_search:?} {shell_integration:?} {:?}",
            sessions.as_slice()
        );
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !trusted_state_debug.contains(hidden),
                "leaked fallback local session detail {hidden:?}"
            );
        }
    }

    #[test]
    fn fallback_local_session_spawner_preserves_state_when_spawn_fails() {
        let size = GridSize::new(3, 40);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"old-session").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 13);
        terminal.feed(b"prod.example.com old-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("old-visible"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/ssh/prod.example.com"));

        let mut sessions = NativeSessionRegistry::default();
        let active_id = sessions.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            active_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: BasicTerminal::with_scrollback_limit(size, 3),
                terminal_search: TerminalSearch::default(),
                shell_integration: ShellIntegrationState::default(),
            },
        );

        let err = apply_fallback_local_session_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            |_spawn_size| Err(anyhow::anyhow!("fallback pty unavailable")),
            size,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "fallback pty unavailable");
        assert_eq!(app.transport().written(), b"old-session");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("old-visible")));
        assert!(terminal_search.is_open());
        assert_eq!(terminal_search.query(), "old-visible");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/ssh/prod.example.com")
        );
        assert_eq!(sessions.as_slice().len(), 1);
        assert_eq!(sessions.active().unwrap().id, active_id);
        assert_eq!(sessions.active().unwrap().profile_action.profile_id, "prod");
        assert_eq!(parked_sessions.as_slice().len(), 1);
    }

    #[test]
    fn empty_session_local_shell_replaces_exited_runtime_without_parking_stale_buffer() {
        let exited_size = GridSize::new(3, 24);
        let new_size = GridSize::new(5, 40);
        let mut app = TerminalApp::new(MockTransport::new(exited_size), exited_size);
        app.write_input(b"dead-runtime").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(exited_size, 11);
        terminal.feed(b"stale exited terminal text");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("stale"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/stale"));
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        let mut template = LocalPtyConfig::new(exited_size);
        template.program = Some("/bin/zsh".to_owned());
        template.args(["-l"]);
        template.cwd("/work/project");
        template.env("WITTY_SESSION", "daily");
        let config = local_new_tab_config(&template, new_size);
        let mut captured_config = None;

        let session_id = replace_with_tracked_local_session_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            config.clone(),
            |config| {
                captured_config = Some(config.clone());
                Ok(MockTransport::new(config.size))
            },
            new_size,
        )
        .unwrap();

        assert_eq!(captured_config, Some(config));
        assert_eq!(session_id, NativeSessionId(1));
        assert_eq!(sessions.as_slice().len(), 1);
        assert_eq!(sessions.active().unwrap().id, session_id);
        assert_eq!(
            sessions.active().unwrap().profile_action.profile_id,
            "local-1"
        );
        assert_eq!(
            sessions.active().unwrap().profile_action.mode,
            NativeProfileActionStartMode::ReplaceCurrentSession
        );
        assert_eq!(parked_sessions.as_slice().len(), 0);
        app.write_input(b"new-local").unwrap();
        assert_eq!(app.transport().written(), b"new-local");
        assert_eq!(app.transport().size(), new_size);
        assert_eq!(terminal.max_scrollback_lines(), 11);
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("stale exited terminal text")));
        assert!(!terminal_search.is_open());
        assert_eq!(shell_integration.current_directory(), None);
    }

    #[test]
    fn empty_session_local_shell_spawn_failure_preserves_exited_state() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"dead-runtime").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 11);
        terminal.feed(b"stale exited terminal text");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("stale"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/stale"));
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let err = replace_with_tracked_local_session_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            LocalPtyConfig::new(size),
            |_config| Err(anyhow::anyhow!("new local shell unavailable")),
            size,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "new local shell unavailable");
        assert_eq!(app.transport().written(), b"dead-runtime");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("stale exited terminal")));
        assert!(terminal_search.is_open());
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/stale")
        );
        assert!(sessions.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_profile_action_new_tab_start_parks_runtime_without_replacing_active_state() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 11);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));

        let mut sessions = NativeSessionRegistry::default();
        let active_id = sessions.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(1),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(handoff, NativeProfileActionStartMode::NewTab);

        let execution = apply_native_profile_action_start_plan_with_transport(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            plan.clone(),
            MockTransport::new(size),
            size,
        );

        assert_eq!(execution, NativeProfileActionStartExecution { plan });
        assert_eq!(sessions.active().unwrap().id, active_id);
        assert_eq!(sessions.as_slice().len(), 2);
        let new_tab_id = sessions.as_slice()[1].id;
        assert_eq!(new_tab_id, NativeSessionId(2));
        assert_eq!(sessions.as_slice()[1].profile_action.profile_id, "prod");
        assert_eq!(
            sessions.as_slice()[1].profile_action.mode,
            NativeProfileActionStartMode::NewTab
        );

        app.write_input(b"-still-active").unwrap();
        assert_eq!(app.transport().written(), b"active-write-still-active");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("active-visible")));
        assert_eq!(terminal.max_scrollback_lines(), 11);
        assert_eq!(terminal_search.query(), "active");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/active")
        );

        assert_eq!(parked_sessions.as_slice().len(), 1);
        let parked = &parked_sessions.as_slice()[0];
        assert_eq!(parked.id, new_tab_id);
        assert!(parked.runtime.transport.written().is_empty());
        assert_eq!(parked.runtime.terminal.max_scrollback_lines(), 11);
        assert!(parked
            .runtime
            .terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("active-visible")));
        assert!(!parked.runtime.terminal_search.is_open());
        assert_eq!(parked.runtime.shell_integration.current_directory(), None);

        let rows = sessions.tab_rows();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].active);
        assert!(!rows[1].active);
        assert_eq!(rows[1].profile_id, "prod");
        assert_eq!(rows[1].mode, NativeProfileActionStartMode::NewTab);
        let tab_text = native_session_tab_strip_text_with_notice(
            &rows,
            None,
            240,
            NativeSessionTabLabelStyle::IndexProfile,
        );
        assert!(tab_text.contains("*0:active x"));
        assert!(tab_text.contains("1:prod x"));
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "open production",
        ] {
            assert!(
                !tab_text.contains(hidden),
                "leaked new-tab detail {hidden:?}"
            );
        }

        assert!(switch_native_session_runtime(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut parked_sessions,
            active_id,
            new_tab_id,
        ));
        assert!(sessions.set_active(new_tab_id));

        app.write_input(b"new-tab-write").unwrap();
        assert_eq!(app.transport().written(), b"new-tab-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("active-visible")));
        assert!(!terminal_search.is_open());
        assert_eq!(shell_integration.current_directory(), None);
        assert_eq!(parked_sessions.as_slice().len(), 1);
        assert_eq!(parked_sessions.as_slice()[0].id, active_id);
        assert_eq!(
            parked_sessions.as_slice()[0].runtime.transport.written(),
            b"active-write-still-active"
        );
    }

    #[test]
    fn native_profile_action_new_tab_without_active_session_starts_active_runtime() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"dead-runtime").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 11);
        terminal.feed(b"stale exited terminal text");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("stale"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/stale"));
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(1),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(handoff, NativeProfileActionStartMode::NewTab);

        let execution = apply_native_profile_action_start_plan_with_transport(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            plan,
            MockTransport::new(size),
            size,
        );

        assert_eq!(
            execution.plan.mode,
            NativeProfileActionStartMode::ReplaceCurrentSession
        );
        assert_eq!(sessions.as_slice().len(), 1);
        assert_eq!(sessions.active().unwrap().profile_action.profile_id, "prod");
        assert_eq!(
            sessions.active().unwrap().profile_action.mode,
            NativeProfileActionStartMode::ReplaceCurrentSession
        );
        assert!(parked_sessions.as_slice().is_empty());
        app.write_input(b"profile-write").unwrap();
        assert_eq!(app.transport().written(), b"profile-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("stale exited terminal text")));
        assert!(!terminal_search.is_open());
        assert_eq!(shell_integration.current_directory(), None);
    }

    #[test]
    fn local_new_tab_start_tracks_untracked_active_session_and_switches_to_new_runtime() {
        let active_size = GridSize::new(3, 24);
        let new_size = GridSize::new(5, 40);
        let mut app = TerminalApp::new(MockTransport::new(active_size), active_size);
        app.write_input(b"active-write").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(active_size, 11);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        let mut template = LocalPtyConfig::new(active_size);
        template.program = Some("/bin/zsh".to_owned());
        template.args(["-l"]);
        template.cwd("/work/project");
        template.env("WITTY_SESSION", "daily");
        let config = local_new_tab_config(&template, new_size);
        let mut captured_config = None;

        let new_session_id = open_local_new_tab_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            config.clone(),
            |config| {
                captured_config = Some(config.clone());
                Ok(MockTransport::new(config.size))
            },
            new_size,
        )
        .unwrap();

        assert_eq!(captured_config, Some(config));
        assert_eq!(new_session_id, NativeSessionId(2));
        assert_eq!(sessions.as_slice().len(), 2);
        assert_eq!(sessions.active().unwrap().id, new_session_id);
        assert_eq!(sessions.as_slice()[0].profile_action.profile_id, "local-1");
        assert_eq!(
            sessions.as_slice()[0].profile_action.mode,
            NativeProfileActionStartMode::ReplaceCurrentSession
        );
        assert_eq!(sessions.as_slice()[1].profile_action.profile_id, "local-2");
        assert_eq!(
            sessions.as_slice()[1].profile_action.mode,
            NativeProfileActionStartMode::NewTab
        );

        app.write_input(b"new-tab-write").unwrap();
        assert_eq!(app.transport().written(), b"new-tab-write");
        assert_eq!(app.transport().size(), new_size);
        assert_eq!(terminal.max_scrollback_lines(), 11);
        assert!(terminal
            .search_text_rows()
            .iter()
            .all(|row| !row.text.contains("active-visible")));
        assert!(!terminal_search.is_open());
        assert_eq!(shell_integration.current_directory(), None);

        assert_eq!(parked_sessions.as_slice().len(), 1);
        let parked_active = &parked_sessions.as_slice()[0];
        assert_eq!(parked_active.id, NativeSessionId(1));
        assert_eq!(parked_active.runtime.transport.written(), b"active-write");
        assert!(parked_active
            .runtime
            .terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("active-visible")));
        assert_eq!(parked_active.runtime.terminal.max_scrollback_lines(), 11);
        assert_eq!(parked_active.runtime.terminal_search.query(), "active");
        assert_eq!(
            parked_active
                .runtime
                .shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/active")
        );

        let tab_text = native_session_tab_strip_text_with_notice(
            &sessions.tab_rows(),
            None,
            240,
            NativeSessionTabLabelStyle::IndexProfile,
        );
        assert!(tab_text.contains("0:local-1 x"));
        assert!(tab_text.contains("*1:local-2 x"));
    }

    #[test]
    fn local_new_tab_spawn_failure_preserves_untracked_active_state() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 11);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));
        let mut sessions = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let err = open_local_new_tab_with_spawner(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut sessions,
            &mut parked_sessions,
            LocalPtyConfig::new(size),
            |_config| Err(anyhow::anyhow!("new tab pty unavailable")),
            size,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "new tab pty unavailable");
        assert_eq!(app.transport().written(), b"active-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("active-visible")));
        assert_eq!(terminal_search.query(), "active");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/active")
        );
        assert!(sessions.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_profile_action_start_failure_row_keeps_retry_state_trusted_and_redacted() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );

        let failure = native_profile_action_start_failure_row(&plan);
        let summary = profile_start_failure_overlay_row_summary(&failure);

        assert_eq!(failure.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(failure.source_plugin, "profile-bridge");
        assert_eq!(failure.profile_id, "prod");
        assert_eq!(failure.detail, "mode=replace_current_session status=failed");
        assert_eq!(failure.retry_label, "Retry");
        assert_eq!(failure.dismiss_label, "Dismiss");
        assert!(summary.contains("[start failed] Retry prod"));
        assert!(summary.contains("plugin=profile-bridge"));
        assert!(summary.contains("reason=open production"));
        assert!(!summary.contains("prod.example.com"));
        assert!(!summary.contains("ssh -tt"));
    }

    #[test]
    fn native_profile_action_start_success_row_keeps_session_state_trusted_and_redacted() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );

        let success = native_profile_action_start_success_row(&plan);
        let summary = profile_start_success_overlay_row_summary(&success);

        assert_eq!(success.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(success.source_plugin, "profile-bridge");
        assert_eq!(success.profile_id, "prod");
        assert_eq!(
            success.detail,
            "mode=replace_current_session status=started"
        );
        assert_eq!(success.dismiss_label, "Dismiss");
        assert!(summary.contains("[started] Active prod"));
        assert!(summary.contains("plugin=profile-bridge"));
        assert!(summary.contains("reason=open production"));
        assert!(!summary.contains("prod.example.com"));
        assert!(!summary.contains("ssh -tt"));
    }

    #[test]
    fn native_profile_action_new_tab_rows_keep_mode_trusted_and_redacted() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(1),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(handoff, NativeProfileActionStartMode::NewTab);

        let success = native_profile_action_start_success_row(&plan);
        let failure = native_profile_action_start_failure_row(&plan);
        let success_summary = profile_start_success_overlay_row_summary(&success);
        let failure_summary = profile_start_failure_overlay_row_summary(&failure);

        assert_eq!(success.title, "New tab prod");
        assert_eq!(success.detail, "mode=new_tab status=started");
        assert_eq!(failure.detail, "mode=new_tab status=failed");
        assert!(success_summary.contains("[started] New tab prod"));
        assert!(success_summary.contains("mode=new_tab status=started"));
        assert!(failure_summary.contains("mode=new_tab status=failed"));
        for hidden in ["prod.example.com", "ssh", "-tt", "LocalPtyConfig"] {
            assert!(
                !success_summary.contains(hidden),
                "leaked success detail {hidden:?}"
            );
            assert!(
                !failure_summary.contains(hidden),
                "leaked failure detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_profile_action_current_session_keeps_config_out_of_metadata() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let execution = NativeProfileActionStartExecution { plan };

        let session = native_profile_action_current_session(&execution);

        assert_eq!(
            session,
            NativeProfileActionCurrentSession {
                key: PendingProfileActionKey::profile_launch(0),
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            }
        );
        let debug = format!("{session:?}");
        for hidden in ["prod.example.com", "ssh", "-tt", "LocalPtyConfig"] {
            assert!(!debug.contains(hidden), "leaked config detail {hidden:?}");
        }
    }

    #[test]
    fn native_session_tab_row_uses_current_session_without_config() {
        let session = NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        };
        let record = NativeSessionRecord {
            id: NativeSessionId(7),
            profile_action: session,
            launch: None,
        };

        let row = native_session_tab_row(&record, Some(NativeSessionId(7)));

        assert_eq!(
            row,
            NativeSessionTabRow {
                session_id: NativeSessionId(7),
                key: PendingProfileActionKey::profile_launch(0),
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: "prod".to_owned(),
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                active: true,
            }
        );
        let debug = format!("{row:?}");
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "open production",
        ] {
            assert!(!debug.contains(hidden), "leaked tab detail {hidden:?}");
        }
    }

    #[test]
    fn native_session_registry_replaces_active_session_without_config() {
        let first = NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        };
        let second = NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_picker(1),
            kind: NativeResolvedProfileActionKind::ProfilePicker,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "staging".to_owned(),
            reason: Some("choose staging".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        };
        let mut registry = NativeSessionRegistry::default();

        let first_id = registry.replace_current(first);
        let second_id = registry.replace_current(second.clone());

        assert_eq!(first_id, NativeSessionId(1));
        assert_eq!(second_id, first_id);
        assert_eq!(registry.as_slice().len(), 1);
        assert_eq!(registry.active().unwrap().profile_action, second);
        let debug = format!("{registry:?}");
        for hidden in ["prod.example.com", "ssh", "-tt", "LocalPtyConfig"] {
            assert!(!debug.contains(hidden), "leaked registry detail {hidden:?}");
        }
    }

    #[test]
    fn native_session_registry_inserts_inactive_session_without_switching_active() {
        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });

        let inactive_id = registry.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });

        assert_eq!(active_id, NativeSessionId(1));
        assert_eq!(inactive_id, NativeSessionId(2));
        assert_eq!(registry.active().unwrap().id, active_id);
        let rows = registry.tab_rows();
        assert!(rows[0].active);
        assert!(!rows[1].active);
        assert_eq!(rows[1].profile_id, "prod");
        assert_eq!(rows[1].mode, NativeProfileActionStartMode::NewTab);
    }

    #[test]
    fn native_session_registry_tab_rows_mark_active_record() {
        let mut registry = NativeSessionRegistry {
            next_session_id: 3,
            active_session_id: Some(NativeSessionId(2)),
            sessions: vec![
                NativeSessionRecord {
                    id: NativeSessionId(1),
                    profile_action: NativeProfileActionCurrentSession {
                        key: PendingProfileActionKey::profile_launch(0),
                        kind: NativeResolvedProfileActionKind::ProfileLaunch,
                        source_plugin: "profile-bridge".to_owned(),
                        profile_id: "prod".to_owned(),
                        reason: Some("open production".to_owned()),
                        mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                    },
                    launch: None,
                },
                NativeSessionRecord {
                    id: NativeSessionId(2),
                    profile_action: NativeProfileActionCurrentSession {
                        key: PendingProfileActionKey::profile_picker(0),
                        kind: NativeResolvedProfileActionKind::ProfilePicker,
                        source_plugin: "profile-bridge".to_owned(),
                        profile_id: "staging".to_owned(),
                        reason: Some("choose staging".to_owned()),
                        mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                    },
                    launch: None,
                },
            ],
        };

        let rows = registry.tab_rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].session_id, NativeSessionId(1));
        assert_eq!(rows[0].profile_id, "prod");
        assert!(!rows[0].active);
        assert_eq!(rows[1].session_id, NativeSessionId(2));
        assert_eq!(rows[1].profile_id, "staging");
        assert!(rows[1].active);
        let text = native_session_tab_strip_text_with_notice(
            &rows,
            None,
            120,
            NativeSessionTabLabelStyle::IndexProfile,
        );
        assert!(text.contains("0:prod x"));
        assert!(text.contains("*1:staging x"));
        assert!(!text.contains("open production"));
        assert!(!text.contains("choose staging"));
        assert!(!registry.set_active(NativeSessionId(99)));
        assert!(!registry.set_active(NativeSessionId(2)));
        assert!(registry.set_active(NativeSessionId(1)));
        assert_eq!(registry.active().unwrap().id, NativeSessionId(1));
        assert!(registry.set_active(NativeSessionId(2)));

        let replacement = NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "qa".to_owned(),
            reason: Some("open qa".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        };
        let active_id = registry.replace_current(replacement);

        assert_eq!(active_id, NativeSessionId(2));
        assert_eq!(registry.as_slice().len(), 2);
        assert_eq!(registry.as_slice()[1].profile_action.profile_id, "qa");
    }

    #[test]
    fn native_session_runtime_switch_swaps_transport_terminal_search_and_shell_state() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();

        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));

        let mut target_terminal = BasicTerminal::with_scrollback_limit(size, 9);
        target_terminal.feed(b"target-visible");
        let mut target_search = TerminalSearch::default();
        target_search.open(&target_terminal.search_text_rows(), Some("target"));
        let mut target_shell = ShellIntegrationState::default();
        target_shell.apply_current_directory(test_current_directory("/target"));

        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            NativeSessionId(2),
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: target_terminal,
                terminal_search: target_search,
                shell_integration: target_shell,
            },
        );

        assert!(switch_native_session_runtime(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut parked_sessions,
            NativeSessionId(1),
            NativeSessionId(2),
        ));

        app.write_input(b"target-write").unwrap();
        assert_eq!(app.transport().written(), b"target-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("target-visible")));
        assert_eq!(terminal.max_scrollback_lines(), 9);
        assert_eq!(terminal_search.query(), "target");
        assert_eq!(terminal_search.match_count(), 1);
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/target")
        );

        assert_eq!(parked_sessions.as_slice().len(), 1);
        let parked_active = &parked_sessions.as_slice()[0];
        assert_eq!(parked_active.id, NativeSessionId(1));
        assert_eq!(parked_active.runtime.transport.written(), b"active-write");
        assert!(parked_active
            .runtime
            .terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("active-visible")));
        assert_eq!(parked_active.runtime.terminal.max_scrollback_lines(), 5);
        assert_eq!(parked_active.runtime.terminal_search.query(), "active");
        assert_eq!(
            parked_active
                .runtime
                .shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/active")
        );
        assert!(!switch_native_session_runtime(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut parked_sessions,
            NativeSessionId(2),
            NativeSessionId(99),
        ));
    }

    #[test]
    fn native_session_close_parked_runtime_removes_inactive_session_only() {
        let size = GridSize::new(3, 24);
        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let inactive_id = registry.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            inactive_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: BasicTerminal::with_scrollback_limit(size, 5),
                terminal_search: TerminalSearch::default(),
                shell_integration: ShellIntegrationState::default(),
            },
        );

        assert!(close_parked_native_session_runtime(
            &mut registry,
            &mut parked_sessions,
            inactive_id,
        ));

        assert_eq!(registry.active().unwrap().id, active_id);
        assert_eq!(registry.as_slice().len(), 1);
        assert_eq!(registry.as_slice()[0].id, active_id);
        assert!(parked_sessions.as_slice().is_empty());
        let debug = format!("{registry:?}");
        for hidden in ["prod.example.com", "ssh", "-tt", "LocalPtyConfig"] {
            assert!(
                !debug.contains(hidden),
                "leaked closed session detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_close_parked_runtime_rejects_active_and_inconsistent_state() {
        let size = GridSize::new(3, 24);
        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let inactive_without_runtime =
            registry.insert_inactive(NativeProfileActionCurrentSession {
                key: PendingProfileActionKey::profile_launch(1),
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                mode: NativeProfileActionStartMode::NewTab,
            });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        for id in [active_id, NativeSessionId(99)] {
            parked_sessions.park_or_replace(
                id,
                NativeSessionRuntime {
                    transport: MockTransport::new(size),
                    terminal: BasicTerminal::with_scrollback_limit(size, 5),
                    terminal_search: TerminalSearch::default(),
                    shell_integration: ShellIntegrationState::default(),
                },
            );
        }

        assert!(!close_parked_native_session_runtime(
            &mut registry,
            &mut parked_sessions,
            active_id,
        ));
        assert!(!close_parked_native_session_runtime(
            &mut registry,
            &mut parked_sessions,
            inactive_without_runtime,
        ));
        assert!(!close_parked_native_session_runtime(
            &mut registry,
            &mut parked_sessions,
            NativeSessionId(99),
        ));

        assert_eq!(registry.active().unwrap().id, active_id);
        assert_eq!(registry.as_slice().len(), 2);
        assert_eq!(parked_sessions.as_slice().len(), 2);
        assert!(parked_sessions
            .as_slice()
            .iter()
            .any(|record| record.id == active_id));
        assert!(parked_sessions
            .as_slice()
            .iter()
            .any(|record| record.id == NativeSessionId(99)));
    }

    #[test]
    fn native_session_close_active_switches_to_parked_session_and_drops_old_runtime() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));

        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let target_id = registry.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });

        let mut target_terminal = BasicTerminal::with_scrollback_limit(size, 9);
        target_terminal.feed(b"target-visible");
        let mut target_search = TerminalSearch::default();
        target_search.open(&target_terminal.search_text_rows(), Some("target"));
        let mut target_shell = ShellIntegrationState::default();
        target_shell.apply_current_directory(test_current_directory("/target"));
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            target_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: target_terminal,
                terminal_search: target_search,
                shell_integration: target_shell,
            },
        );

        assert!(close_active_native_session_by_switching_to_parked(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut registry,
            &mut parked_sessions,
        ));

        assert_eq!(active_id, NativeSessionId(1));
        assert_eq!(registry.active().unwrap().id, target_id);
        assert_eq!(registry.as_slice().len(), 1);
        assert_eq!(registry.as_slice()[0].id, target_id);
        assert!(parked_sessions.as_slice().is_empty());
        app.write_input(b"target-write").unwrap();
        assert_eq!(app.transport().written(), b"target-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("target-visible")));
        assert_eq!(terminal.max_scrollback_lines(), 9);
        assert_eq!(terminal_search.query(), "target");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/target")
        );
    }

    #[test]
    fn native_session_close_active_rejects_when_no_parked_target_exists() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));

        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let inactive_id = registry.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        assert!(!close_active_native_session_by_switching_to_parked(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut registry,
            &mut parked_sessions,
        ));

        assert_eq!(registry.active().unwrap().id, active_id);
        assert_eq!(registry.as_slice().len(), 2);
        assert_eq!(registry.as_slice()[1].id, inactive_id);
        assert!(parked_sessions.as_slice().is_empty());
        app.write_input(b"-still-active").unwrap();
        assert_eq!(app.transport().written(), b"active-write-still-active");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("active-visible")));
        assert_eq!(terminal_search.query(), "active");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/active")
        );
    }

    #[test]
    fn native_child_exit_of_untracked_last_session_requests_window_close_by_default() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        let mut terminal_search = TerminalSearch::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut registry = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let result = close_exited_active_native_session(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut registry,
            &mut parked_sessions,
            NativeActiveSessionCloseFallbackPolicy::default(),
        );

        assert_eq!(result, NativeSessionCloseResult::RequestWindowClose);
        assert!(registry.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_child_exit_of_untracked_last_session_can_block_for_welcome_screen() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        let mut terminal_search = TerminalSearch::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut registry = NativeSessionRegistry::default();
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();

        let result = close_exited_active_native_session(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut registry,
            &mut parked_sessions,
            NativeActiveSessionCloseFallbackPolicy::Block,
        );

        assert_eq!(result, NativeSessionCloseResult::BlockedLastActive);
        assert!(registry.as_slice().is_empty());
        assert!(parked_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_child_exit_of_tracked_active_session_switches_to_parked_session() {
        let size = GridSize::new(3, 24);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.write_input(b"active-write").unwrap();
        let mut terminal = BasicTerminal::with_scrollback_limit(size, 5);
        terminal.feed(b"active-visible");
        let mut terminal_search = TerminalSearch::default();
        terminal_search.open(&terminal.search_text_rows(), Some("active"));
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_current_directory(test_current_directory("/active"));

        let mut registry = NativeSessionRegistry::default();
        let active_id = registry.replace_current(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "active".to_owned(),
            reason: Some("existing active session".to_owned()),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
        });
        let target_id = registry.insert_inactive(NativeProfileActionCurrentSession {
            key: PendingProfileActionKey::profile_launch(1),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("open production".to_owned()),
            mode: NativeProfileActionStartMode::NewTab,
        });

        let mut target_terminal = BasicTerminal::with_scrollback_limit(size, 9);
        target_terminal.feed(b"target-visible");
        let mut target_search = TerminalSearch::default();
        target_search.open(&target_terminal.search_text_rows(), Some("target"));
        let mut target_shell = ShellIntegrationState::default();
        target_shell.apply_current_directory(test_current_directory("/target"));
        let mut parked_sessions = NativeSessionRuntimeRegistry::default();
        parked_sessions.park_or_replace(
            target_id,
            NativeSessionRuntime {
                transport: MockTransport::new(size),
                terminal: target_terminal,
                terminal_search: target_search,
                shell_integration: target_shell,
            },
        );

        let result = close_exited_active_native_session(
            &mut app,
            &mut terminal,
            &mut terminal_search,
            &mut shell_integration,
            &mut registry,
            &mut parked_sessions,
            NativeActiveSessionCloseFallbackPolicy::default(),
        );

        assert_eq!(result, NativeSessionCloseResult::Closed);
        assert_eq!(active_id, NativeSessionId(1));
        assert_eq!(registry.active().unwrap().id, target_id);
        assert_eq!(registry.as_slice().len(), 1);
        assert!(parked_sessions.as_slice().is_empty());
        app.write_input(b"target-write").unwrap();
        assert_eq!(app.transport().written(), b"target-write");
        assert!(terminal
            .search_text_rows()
            .iter()
            .any(|row| row.text.contains("target-visible")));
        assert_eq!(terminal_search.query(), "target");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|directory| directory.path.as_str()),
            Some("/target")
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_pty_child_exit_poll_requests_window_close_by_default() {
        let mut app = TerminalWindowApp::new(
            None,
            Vec::new(),
            WindowSmokeOptions::default(),
            None,
            Some("/bin/sh".to_owned()),
            vec!["-lc".to_owned(), "exit 7".to_owned()],
            None,
            Vec::new(),
            MouseSelectionOverridePolicy::default(),
            Osc52ClipboardPolicy::Disabled,
            1000,
            None,
            None,
            None,
            None,
            None,
            None,
            CursorShape::Block,
            true,
            NativeSessionTabPosition::Bottom,
            NativeSessionTabLabelStyle::Index,
            NativeSessionTabDisplayPolicy::default(),
            Vec::new(),
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut changed = false;

        while Instant::now() < deadline && !app.window_close_requested {
            changed |= app.poll_transport();
            if app.exited && app.window_close_requested {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(changed, "poll_transport never observed the PTY child exit");
        assert!(app.exited);
        assert!(app.window_close_requested);
        assert!(!app.fallback_local_session_requested);
        let visible_text = app
            .terminal
            .search_text_rows()
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible_text.contains("[process exited: 7]"));
    }

    #[cfg(unix)]
    #[test]
    fn local_pty_child_exit_poll_can_keep_window_open_with_block_policy() {
        let mut smoke = WindowSmokeOptions::default();
        smoke.last_active_close_policy = WindowLastActiveClosePolicy::Block;
        let mut app = TerminalWindowApp::new(
            None,
            Vec::new(),
            smoke,
            None,
            Some("/bin/sh".to_owned()),
            vec!["-lc".to_owned(), "exit 0".to_owned()],
            None,
            Vec::new(),
            MouseSelectionOverridePolicy::default(),
            Osc52ClipboardPolicy::Disabled,
            1000,
            None,
            None,
            None,
            None,
            None,
            None,
            CursorShape::Block,
            true,
            NativeSessionTabPosition::Bottom,
            NativeSessionTabLabelStyle::Index,
            NativeSessionTabDisplayPolicy::default(),
            Vec::new(),
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut changed = false;

        while Instant::now() < deadline && !app.exited {
            changed |= app.poll_transport();
            if app.exited {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(changed, "poll_transport never observed the PTY child exit");
        assert!(app.exited);
        assert!(!app.window_close_requested);
        assert!(!app.fallback_local_session_requested);
        assert!(app.profile_action_sessions.as_slice().is_empty());
    }

    #[test]
    fn native_session_tab_display_policy_defaults_to_hidden() {
        let policy = NativeSessionTabDisplayPolicy::default();

        assert!(!policy.should_show(0));
        assert!(!policy.should_show(1));
        assert!(!policy.should_show(2));
        assert!(NativeSessionTabDisplayPolicy {
            show_single: true,
            show_multiple: false,
        }
        .should_show(1));
        assert!(NativeSessionTabDisplayPolicy {
            show_single: false,
            show_multiple: true,
        }
        .should_show(2));
    }

    #[test]
    fn native_session_switcher_opens_next_and_wraps_selection() {
        let rows = test_session_tab_rows();
        let mut switcher = NativeSessionSwitcher::default();

        assert!(switcher.open(&rows, 1));
        assert_eq!(switcher.selected_session_id, Some(NativeSessionId(2)));
        assert!(switcher.move_selection(&rows, 1));
        assert_eq!(switcher.selected_session_id, Some(NativeSessionId(3)));
        assert!(switcher.move_selection(&rows, 1));
        assert_eq!(switcher.selected_session_id, Some(NativeSessionId(1)));
        assert!(switcher.move_selection(&rows, -1));
        assert_eq!(switcher.selected_session_id, Some(NativeSessionId(3)));
    }

    #[test]
    fn session_switcher_overlay_hides_covered_terminal_frame_and_marks_selection() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(12, 80);
        let rows = test_session_tab_rows();
        let mut switcher = NativeSessionSwitcher::default();
        assert!(switcher.open(&rows, 1));
        let mut frame = FramePlan::default();
        let panel = session_switcher_panel(grid_size, rows.len()).unwrap();
        frame.cursor = Some(RectBatchItem {
            origin: cell_origin(CellPoint::new(panel.start.row, panel.start.col), metrics),
            size: metrics.cell,
            color: Rgba::rgb(255, 255, 255),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1),
                metrics,
            ),
            text: "stale terminal text".to_owned(),
            color: Rgba::rgb(255, 255, 255),
            style_flags: CellFlags::default(),
        });

        apply_session_switcher_overlay(
            &mut frame,
            &switcher,
            &rows,
            NativeSessionTabLabelStyle::Index,
            metrics,
            grid_size,
        );

        assert!(frame.cursor.is_none());
        assert!(!frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("stale terminal text")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Switch Session")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("> 1  local-2  session")));
    }

    #[test]
    fn native_session_tab_strip_hit_test_maps_visible_tab_text_only() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(4, 120);
        let position = NativeSessionTabPosition::Top;
        let label_style = NativeSessionTabLabelStyle::Index;
        let rows = vec![
            NativeSessionTabRow {
                session_id: NativeSessionId(1),
                key: PendingProfileActionKey::profile_launch(0),
                kind: NativeResolvedProfileActionKind::ProfileLaunch,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: "prod".to_owned(),
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                active: false,
            },
            NativeSessionTabRow {
                session_id: NativeSessionId(2),
                key: PendingProfileActionKey::profile_picker(0),
                kind: NativeResolvedProfileActionKind::ProfilePicker,
                source_plugin: "profile-bridge".to_owned(),
                profile_id: "staging".to_owned(),
                mode: NativeProfileActionStartMode::ReplaceCurrentSession,
                active: true,
            },
        ];
        let first_label_width =
            text_cell_width(&native_session_tab_label(&rows[0], 0, label_style));
        let first_width = text_cell_width(&native_session_tab_summary(&rows[0], 0, label_style));
        let first_close_start = first_label_width.saturating_add(1);
        let second_start = first_width.saturating_add(2);

        let first_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(0, 1), metrics),
            metrics,
            grid_size,
        );
        let first_close_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(0, 1 + first_close_start), metrics),
            metrics,
            grid_size,
        );
        let separator_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(0, 1 + first_width), metrics),
            metrics,
            grid_size,
        );
        let second_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(0, 1 + second_start), metrics),
            metrics,
            grid_size,
        );
        let terminal_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(1, 1), metrics),
            metrics,
            grid_size,
        );

        assert_eq!(
            first_hit,
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(1),
                row_index: 0,
                target: NativeSessionTabStripTarget::Select,
            })
        );
        assert_eq!(
            first_close_hit,
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(1),
                row_index: 0,
                target: NativeSessionTabStripTarget::Close,
            })
        );
        assert_eq!(separator_hit, None);
        assert_eq!(
            second_hit,
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(2),
                row_index: 1,
                target: NativeSessionTabStripTarget::Select,
            })
        );
        assert_eq!(terminal_hit, None);
    }

    #[test]
    fn native_session_tab_strip_hit_test_ignores_truncated_close_affordance() {
        let metrics = CellMetrics::default();
        let position = NativeSessionTabPosition::Top;
        let label_style = NativeSessionTabLabelStyle::Index;
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };
        let close_start =
            text_cell_width(&native_session_tab_label(&row, 0, label_style)).saturating_add(1);
        let rows = vec![row];
        let grid_size = GridSize::new(4, close_start.saturating_add(3));

        let truncated_close_hit = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            position,
            label_style,
            physical_position_for_cell(CellPoint::new(0, 1 + close_start), metrics),
            metrics,
            grid_size,
        );

        assert_eq!(truncated_close_hit, None);
    }

    #[test]
    fn native_session_tab_strip_hover_highlights_host_owned_span_only() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(6, 80);
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };
        let mut frame = FramePlan::default();

        apply_native_session_tab_strip_overlay(
            &mut frame,
            &[row],
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(1),
                row_index: 0,
                target: NativeSessionTabStripTarget::Select,
            }),
            None,
            NativeSessionTabPosition::Top,
            NativeSessionTabLabelStyle::Index,
            metrics,
            grid_size,
        );

        assert_eq!(frame.overlay_backgrounds.len(), 2);
        assert_eq!(
            frame.overlay_backgrounds[1].origin,
            cell_origin(CellPoint::new(0, 1), metrics)
        );
        assert_eq!(
            frame.overlay_backgrounds[1].color,
            native_session_tab_hover_color(NativeSessionTabStripTarget::Select)
        );
        assert!(frame.glyphs.iter().any(|glyph| glyph.text.contains("*0 x")));
        for hidden in ["prod.example.com", "ssh -tt", "LocalPtyConfig"] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked tab hover detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_tab_strip_close_hover_uses_close_target_color_only() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(6, 120);
        let label_style = NativeSessionTabLabelStyle::Index;
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };
        let close_start =
            text_cell_width(&native_session_tab_label(&row, 0, label_style)).saturating_add(1);
        let close_width = text_cell_width(native_session_tab_close_label());
        let mut frame = FramePlan::default();

        apply_native_session_tab_strip_overlay(
            &mut frame,
            &[row],
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(1),
                row_index: 0,
                target: NativeSessionTabStripTarget::Close,
            }),
            None,
            NativeSessionTabPosition::Top,
            label_style,
            metrics,
            grid_size,
        );

        assert_eq!(frame.overlay_backgrounds.len(), 2);
        assert_eq!(
            frame.overlay_backgrounds[1].origin,
            cell_origin(CellPoint::new(0, 1 + close_start), metrics)
        );
        assert_eq!(
            frame.overlay_backgrounds[1].size.width,
            f32::from(close_width) * metrics.cell.width
        );
        assert_eq!(
            frame.overlay_backgrounds[1].color,
            native_session_tab_hover_color(NativeSessionTabStripTarget::Close)
        );
        assert_ne!(
            frame.overlay_backgrounds[1].color,
            native_session_tab_hover_color(NativeSessionTabStripTarget::Select)
        );
        for hidden in ["prod.example.com", "ssh -tt", "LocalPtyConfig"] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked close hover detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_tab_strip_renders_trusted_read_model_only() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(6, 120);
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };
        let mut frame = FramePlan {
            glyphs: vec![
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(0, 0), metrics),
                    text: "terminal row zero".to_owned(),
                    color: Rgba::rgb(255, 255, 255),
                    style_flags: CellFlags::default(),
                },
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(1, 0), metrics),
                    text: "terminal row one".to_owned(),
                    color: Rgba::rgb(255, 255, 255),
                    style_flags: CellFlags::default(),
                },
            ],
            cursor: Some(RectBatchItem {
                origin: cell_origin(CellPoint::new(0, 1), metrics),
                size: metrics.cell,
                color: Rgba::rgb(255, 255, 255),
            }),
            ..FramePlan::default()
        };

        apply_native_session_tab_strip_overlay(
            &mut frame,
            &[row],
            None,
            None,
            NativeSessionTabPosition::Top,
            NativeSessionTabLabelStyle::IndexProfile,
            metrics,
            grid_size,
        );

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("*0:prod x")));
        assert!(!frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("terminal row zero")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("terminal row one")));
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.overlay_backgrounds.len(), 1);
        for hidden in [
            "prod.example.com",
            "ssh -tt",
            "LocalPtyConfig",
            "open production",
        ] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked tab detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_tab_strip_notice_stays_host_owned_and_redacted() {
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };

        let text = native_session_tab_strip_text_with_notice(
            &[row],
            Some(NativeSessionTabStripNotice::LastActiveCloseBlocked),
            160,
            NativeSessionTabLabelStyle::Index,
        );

        assert!(text.contains("[close blocked: last active]"));
        assert!(text.contains('x'));
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
        ] {
            assert!(
                !text.contains(hidden),
                "leaked tab notice detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_tab_strip_notice_is_visible_and_not_actionable_when_width_is_tight() {
        let metrics = CellMetrics::default();
        let notice = NativeSessionTabStripNotice::LastActiveCloseBlocked;
        let notice_text = native_session_tab_strip_notice_text(notice);
        let width = text_cell_width(notice_text)
            .saturating_add(text_cell_width("  "))
            .saturating_add(12);
        let grid_size = GridSize::new(4, width.saturating_add(2));
        let row = NativeSessionTabRow {
            session_id: NativeSessionId(1),
            key: PendingProfileActionKey::profile_launch(0),
            kind: NativeResolvedProfileActionKind::ProfileLaunch,
            source_plugin: "profile-bridge".to_owned(),
            profile_id: "prod-with-a-very-long-profile-id".to_owned(),
            mode: NativeProfileActionStartMode::ReplaceCurrentSession,
            active: true,
        };
        let rows = vec![row];
        let action_width = native_session_tab_strip_action_width(&rows, Some(notice), width);
        let notice_start_col = action_width.saturating_add(text_cell_width("  "));

        let text = native_session_tab_strip_text_with_notice(
            &rows,
            Some(notice),
            width,
            NativeSessionTabLabelStyle::IndexProfile,
        );
        let notice_hit = native_session_tab_strip_hit_for_position(
            &rows,
            Some(notice),
            NativeSessionTabPosition::Top,
            NativeSessionTabLabelStyle::IndexProfile,
            physical_position_for_cell(CellPoint::new(0, 1 + notice_start_col), metrics),
            metrics,
            grid_size,
        );
        let same_position_without_notice = native_session_tab_strip_hit_for_position(
            &rows,
            None,
            NativeSessionTabPosition::Top,
            NativeSessionTabLabelStyle::IndexProfile,
            physical_position_for_cell(CellPoint::new(0, 1 + notice_start_col), metrics),
            metrics,
            grid_size,
        );

        assert!(text.contains(notice_text));
        assert!(text_cell_width(&text) <= width);
        assert_eq!(notice_hit, None);
        assert_eq!(
            same_position_without_notice,
            Some(NativeSessionTabStripHit {
                session_id: NativeSessionId(1),
                row_index: 0,
                target: NativeSessionTabStripTarget::Select,
            })
        );
        for hidden in ["prod.example.com", "ssh", "-tt", "LocalPtyConfig"] {
            assert!(
                !text.contains(hidden),
                "leaked priority notice detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_session_tab_notice_lifecycle_follows_close_result() {
        for current in [
            None,
            Some(NativeSessionTabStripNotice::LastActiveCloseBlocked),
        ] {
            for result in NativeSessionCloseResult::all() {
                let expected = match *result {
                    NativeSessionCloseResult::Closed
                    | NativeSessionCloseResult::RequestWindowClose
                    | NativeSessionCloseResult::RequestFallbackLocalSession => None,
                    NativeSessionCloseResult::BlockedLastActive => {
                        Some(NativeSessionTabStripNotice::LastActiveCloseBlocked)
                    }
                    NativeSessionCloseResult::Ignored => current,
                };

                assert_eq!(
                    native_session_tab_notice_after_close_result(current, *result),
                    expected
                );
            }
        }
    }

    #[test]
    fn native_session_close_event_requests_are_result_specific() {
        for result in NativeSessionCloseResult::all() {
            let requests = native_session_close_event_requests(*result);

            assert_eq!(
                requests.window_close,
                matches!(*result, NativeSessionCloseResult::RequestWindowClose)
            );
            assert_eq!(
                requests.fallback_local_session,
                matches!(
                    *result,
                    NativeSessionCloseResult::RequestFallbackLocalSession
                )
            );
            assert_eq!(
                requests.any(),
                matches!(
                    *result,
                    NativeSessionCloseResult::RequestWindowClose
                        | NativeSessionCloseResult::RequestFallbackLocalSession
                )
            );
        }

        let debug = format!(
            "{:?} {:?}",
            native_session_close_event_requests(NativeSessionCloseResult::RequestWindowClose),
            native_session_close_event_requests(
                NativeSessionCloseResult::RequestFallbackLocalSession
            )
        );
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !debug.contains(hidden),
                "leaked close event request detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_event_request_flags_are_one_shot() {
        let mut requested = false;

        assert!(!take_native_event_request_flag(&mut requested));
        assert!(!requested);

        requested = true;
        assert!(take_native_event_request_flag(&mut requested));
        assert!(!requested);
        assert!(!take_native_event_request_flag(&mut requested));
    }

    #[test]
    fn native_session_close_event_requests_apply_without_clearing_existing_flags() {
        let mut window_close_requested = false;
        let mut fallback_local_session_requested = false;

        NativeSessionCloseEventRequests::default().apply_to(
            &mut window_close_requested,
            &mut fallback_local_session_requested,
        );
        assert!(!window_close_requested);
        assert!(!fallback_local_session_requested);

        native_session_close_event_requests(NativeSessionCloseResult::RequestWindowClose).apply_to(
            &mut window_close_requested,
            &mut fallback_local_session_requested,
        );
        assert!(window_close_requested);
        assert!(!fallback_local_session_requested);

        native_session_close_event_requests(NativeSessionCloseResult::RequestFallbackLocalSession)
            .apply_to(
                &mut window_close_requested,
                &mut fallback_local_session_requested,
            );
        assert!(window_close_requested);
        assert!(fallback_local_session_requested);

        NativeSessionCloseEventRequests::default().apply_to(
            &mut window_close_requested,
            &mut fallback_local_session_requested,
        );
        assert!(window_close_requested);
        assert!(fallback_local_session_requested);
    }

    #[test]
    fn native_active_session_close_fallback_policy_closes_by_default() {
        let policy = NativeActiveSessionCloseFallbackPolicy::default();
        let action = native_active_session_close_fallback_action_without_switch_target(policy);

        assert_eq!(
            action,
            NativeActiveSessionCloseFallbackAction::RequestWindowClose
        );

        assert_eq!(
            native_session_close_result_for_fallback_action(action),
            NativeSessionCloseResult::RequestWindowClose
        );

        let debug = format!("{policy:?} {action:?}");
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !debug.contains(hidden),
                "leaked close fallback policy detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_active_session_close_fallback_policy_blocks_when_configured() {
        let policy = NativeActiveSessionCloseFallbackPolicy::Block;
        let action = native_active_session_close_fallback_action_without_switch_target(policy);

        assert_eq!(
            action,
            NativeActiveSessionCloseFallbackAction::BlockLastActive
        );

        assert_eq!(
            native_session_close_result_for_fallback_action(action),
            NativeSessionCloseResult::BlockedLastActive
        );
        assert_eq!(
            native_session_close_result_for_last_active_policy(policy),
            NativeSessionCloseResult::BlockedLastActive
        );
    }

    #[test]
    fn native_active_session_close_fallback_policy_config_values_follow_window_policy() {
        for window_policy in WindowLastActiveClosePolicy::all() {
            let native_policy = NativeActiveSessionCloseFallbackPolicy::from(*window_policy);

            assert_eq!(
                native_policy.as_config_value(),
                window_policy.as_config_value()
            );
        }
    }

    #[test]
    fn native_active_session_close_fallback_policy_all_matches_window_policy_all() {
        let native_values: Vec<_> = NativeActiveSessionCloseFallbackPolicy::all()
            .iter()
            .map(|policy| policy.as_config_value())
            .collect();
        let window_values: Vec<_> = WindowLastActiveClosePolicy::all()
            .iter()
            .map(|policy| policy.as_config_value())
            .collect();

        assert_eq!(native_values, window_values);
    }

    #[test]
    fn native_active_session_close_fallback_policy_can_request_window_close() {
        let policy = NativeActiveSessionCloseFallbackPolicy::CloseWindow;
        let action = native_active_session_close_fallback_action_without_switch_target(policy);

        assert_eq!(
            action,
            NativeActiveSessionCloseFallbackAction::RequestWindowClose
        );
        assert_eq!(
            native_session_close_result_for_fallback_action(action),
            NativeSessionCloseResult::RequestWindowClose
        );
        assert_eq!(
            native_session_tab_notice_after_close_result(
                Some(NativeSessionTabStripNotice::LastActiveCloseBlocked),
                native_session_close_result_for_fallback_action(action),
            ),
            None
        );

        let debug = format!("{policy:?} {action:?}");
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !debug.contains(hidden),
                "leaked close-window fallback policy detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_active_session_close_fallback_policy_can_request_fallback_local_session() {
        let policy = NativeActiveSessionCloseFallbackPolicy::FallbackLocalSession;
        let action = native_active_session_close_fallback_action_without_switch_target(policy);

        assert_eq!(
            action,
            NativeActiveSessionCloseFallbackAction::RequestFallbackLocalSession
        );
        assert_eq!(
            native_session_close_result_for_fallback_action(action),
            NativeSessionCloseResult::RequestFallbackLocalSession
        );
        assert_eq!(
            native_session_tab_notice_after_close_result(
                Some(NativeSessionTabStripNotice::LastActiveCloseBlocked),
                native_session_close_result_for_fallback_action(action),
            ),
            None
        );

        let debug = format!("{policy:?} {action:?}");
        for hidden in [
            "prod.example.com",
            "ssh",
            "-tt",
            "LocalPtyConfig",
            "credentials",
            "launch result",
            "tab inventory",
        ] {
            assert!(
                !debug.contains(hidden),
                "leaked fallback-local-session policy detail {hidden:?}"
            );
        }
    }

    #[test]
    fn native_profile_action_bridge_preserves_start_failure_across_refresh() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let failure = native_profile_action_start_failure_row(&plan);
        let mut bridge = NativeProfileActionBridge::default();

        bridge.set_start_failure(Some(failure.clone()));
        let event = bridge.refresh(&app, &store).unwrap();

        let NativeProfileActionBridgeEvent::SnapshotRefreshed(snapshot) = event else {
            panic!("expected snapshot refresh");
        };
        assert_eq!(snapshot.start_failure, Some(failure.clone()));
        assert_eq!(bridge.snapshot().start_failure, Some(failure));
        assert_eq!(snapshot.picker_requests, 1);
        assert_eq!(snapshot.launch_requests, 1);
    }

    #[test]
    fn native_profile_action_bridge_preserves_start_success_across_refresh_and_clears_failure() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let success = native_profile_action_start_success_row(&plan);
        let failure = native_profile_action_start_failure_row(&plan);
        let mut bridge = NativeProfileActionBridge::default();

        bridge.set_start_failure(Some(failure));
        bridge.set_start_success(Some(success.clone()));
        let event = bridge.refresh(&app, &store).unwrap();

        let NativeProfileActionBridgeEvent::SnapshotRefreshed(snapshot) = event else {
            panic!("expected snapshot refresh");
        };
        assert_eq!(snapshot.start_success, Some(success.clone()));
        assert_eq!(snapshot.start_failure, None);
        assert_eq!(bridge.snapshot().start_success, Some(success));
        assert_eq!(bridge.snapshot().start_failure, None);
    }

    #[test]
    fn native_profile_action_bridge_keeps_snapshot_when_confirm_fails() {
        let mut app = profile_bridge_app();
        let store = profile_bridge_store();
        let mut bridge = NativeProfileActionBridge::default();
        bridge.refresh(&app, &store).unwrap();
        let before = bridge.snapshot().clone();

        assert!(bridge
            .confirm(
                &mut app,
                &store,
                PendingProfileActionConfirmation::profile_launch(
                    PendingProfileActionKey::profile_picker(0),
                ),
                GridSize::new(24, 80),
            )
            .is_err());

        assert_eq!(bridge.snapshot(), &before);
        assert_eq!(app.profile_picker_requests().len(), 1);
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    fn frame_with_cursor() -> FramePlan {
        FramePlan {
            cursor: Some(RectBatchItem {
                origin: PixelPoint { x: 0.0, y: 0.0 },
                size: CellMetrics::default().cell,
                color: Rgba::WHITE,
            }),
            ..FramePlan::default()
        }
    }

    #[test]
    fn cursor_blink_state_hides_and_restores_due_cursor_phase() {
        let now = Instant::now();
        let cursor = CursorState::default();
        let mut blink = CursorBlinkState::default();
        let mut visible_frame = frame_with_cursor();

        blink.apply_to_frame(&mut visible_frame, cursor, TextInputTarget::Terminal, now);

        assert!(visible_frame.cursor.is_some());
        assert_eq!(blink.next_deadline(), Some(now + CURSOR_BLINK_INTERVAL));

        assert!(blink.toggle_if_due(
            cursor,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL
        ));
        let mut hidden_frame = frame_with_cursor();
        blink.apply_to_frame(
            &mut hidden_frame,
            cursor,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL,
        );

        assert!(hidden_frame.cursor.is_none());

        assert!(blink.toggle_if_due(
            cursor,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL + CURSOR_BLINK_INTERVAL
        ));
        let mut restored_frame = frame_with_cursor();
        blink.apply_to_frame(
            &mut restored_frame,
            cursor,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL + CURSOR_BLINK_INTERVAL,
        );

        assert!(restored_frame.cursor.is_some());
    }

    #[test]
    fn cursor_blink_state_resets_when_cursor_identity_changes() {
        let now = Instant::now();
        let cursor = CursorState::default();
        let mut blink = CursorBlinkState::default();

        blink.apply_to_frame(
            &mut frame_with_cursor(),
            cursor,
            TextInputTarget::Terminal,
            now,
        );
        assert!(blink.toggle_if_due(
            cursor,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL
        ));

        let moved = CursorState {
            position: CellPoint::new(1, 2),
            ..cursor
        };
        let mut frame = frame_with_cursor();
        blink.apply_to_frame(
            &mut frame,
            moved,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL,
        );

        assert!(frame.cursor.is_some());
        assert_eq!(
            blink.next_deadline(),
            Some(now + CURSOR_BLINK_INTERVAL + CURSOR_BLINK_INTERVAL)
        );
    }

    #[test]
    fn cursor_blink_state_disables_for_steady_hidden_or_non_terminal_cursor() {
        let now = Instant::now();
        let mut blink = CursorBlinkState::default();
        let blinking = CursorState::default();
        blink.apply_to_frame(
            &mut frame_with_cursor(),
            blinking,
            TextInputTarget::Terminal,
            now,
        );
        assert!(blink.next_deadline().is_some());

        let steady = CursorState {
            blink: false,
            ..blinking
        };
        let mut steady_frame = frame_with_cursor();
        blink.apply_to_frame(
            &mut steady_frame,
            steady,
            TextInputTarget::Terminal,
            now + CURSOR_BLINK_INTERVAL,
        );
        assert!(steady_frame.cursor.is_some());
        assert!(blink.next_deadline().is_none());

        blink.apply_to_frame(
            &mut frame_with_cursor(),
            blinking,
            TextInputTarget::CommandPalette,
            now,
        );
        assert!(blink.next_deadline().is_none());

        let mut no_cursor_frame = FramePlan::default();
        blink.apply_to_frame(
            &mut no_cursor_frame,
            blinking,
            TextInputTarget::Terminal,
            now,
        );
        assert!(blink.next_deadline().is_none());

        let hidden = CursorState {
            visible: false,
            ..blinking
        };
        blink.apply_to_frame(
            &mut frame_with_cursor(),
            hidden,
            TextInputTarget::Terminal,
            now,
        );
        assert!(blink.next_deadline().is_none());
    }

    #[test]
    fn text_blink_state_schedules_and_toggles_visible_phase() {
        let now = Instant::now();
        let mut snapshot = RenderSnapshot::from_plain_lines(&["blink"]);
        snapshot.rows[0].cells[0].style.flags.blink = true;
        let mut blink = TextBlinkState::default();

        assert!(blink.apply_to_snapshot(&snapshot, now));
        assert!(blink.visible_phase());
        assert_eq!(blink.next_deadline(), Some(now + TEXT_BLINK_INTERVAL));

        assert!(!blink.toggle_if_due(now + TEXT_BLINK_INTERVAL / 2));
        assert!(blink.visible_phase());

        assert!(blink.toggle_if_due(now + TEXT_BLINK_INTERVAL));
        assert!(!blink.visible_phase());
        assert_eq!(
            blink.next_deadline(),
            Some(now + TEXT_BLINK_INTERVAL + TEXT_BLINK_INTERVAL)
        );

        assert!(!blink.apply_to_snapshot(&snapshot, now + TEXT_BLINK_INTERVAL));
        assert!(!blink.visible_phase());
    }

    #[test]
    fn text_blink_state_disables_without_visible_blink_cells() {
        let now = Instant::now();
        let mut snapshot = RenderSnapshot::from_plain_lines(&["blink"]);
        snapshot.rows[0].cells[0].style.flags.blink = true;
        let mut blink = TextBlinkState::default();
        blink.apply_to_snapshot(&snapshot, now);
        assert!(blink.next_deadline().is_some());

        snapshot.rows[0].cells[0].style.flags.conceal = true;

        assert!(blink.apply_to_snapshot(&snapshot, now + TEXT_BLINK_INTERVAL));
        assert!(blink.visible_phase());
        assert!(blink.next_deadline().is_none());
        assert!(!snapshot_contains_blinking_text(&snapshot));
    }

    #[test]
    fn earliest_deadline_chooses_soonest_available_timer() {
        let now = Instant::now();

        assert_eq!(earliest_deadline(None, None), None);
        assert_eq!(earliest_deadline(Some(now), None), Some(now));
        assert_eq!(
            earliest_deadline(Some(now + TEXT_BLINK_INTERVAL), Some(now)),
            Some(now)
        );
    }

    #[test]
    fn synchronized_output_deadline_starts_with_timeout() {
        let now = Instant::now();

        assert_eq!(
            synchronized_output_deadline_after_poll(None, now),
            Some(now + SYNCHRONIZED_OUTPUT_TIMEOUT)
        );
    }

    #[test]
    fn synchronized_output_deadline_preserves_existing_deadline() {
        let now = Instant::now();
        let existing = now + Duration::from_millis(20);

        assert_eq!(
            synchronized_output_deadline_after_poll(Some(existing), now),
            Some(existing)
        );
    }

    #[test]
    fn key_encoder_handles_text_and_named_keys() {
        assert_eq!(
            encode_key_input(
                &Key::Character("x".into()),
                Some("x"),
                false,
                TerminalInputModes::default()
            ),
            Some(b"x".to_vec())
        );
        assert_eq!(
            encode_key_input(
                &Key::Named(NamedKey::Enter),
                None,
                false,
                TerminalInputModes::default()
            ),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_key_input(
                &Key::Named(NamedKey::ArrowUp),
                None,
                false,
                TerminalInputModes::default()
            ),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn key_encoder_respects_keyboard_action_mode() {
        let locked_modes = TerminalInputModes {
            keyboard_locked: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_key_input(&Key::Character("x".into()), Some("x"), false, locked_modes),
            None
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::Enter), None, false, locked_modes),
            None
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::ArrowUp), None, false, locked_modes),
            None
        );
    }

    #[test]
    fn key_encoder_uses_backarrow_key_mode() {
        let backarrow_modes = TerminalInputModes {
            backarrow_sends_backspace: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_key_input(
                &Key::Named(NamedKey::Backspace),
                None,
                false,
                TerminalInputModes::default()
            ),
            Some(b"\x7f".to_vec())
        );
        assert_eq!(
            encode_key_input(
                &Key::Named(NamedKey::Backspace),
                None,
                false,
                backarrow_modes
            ),
            Some(b"\x08".to_vec())
        );
    }

    #[test]
    fn key_encoder_uses_application_cursor_key_sequences() {
        let modes = TerminalInputModes {
            application_cursor_keys: true,
            application_keypad: false,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::ArrowUp), None, false, modes),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::ArrowDown), None, false, modes),
            Some(b"\x1bOB".to_vec())
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::ArrowRight), None, false, modes),
            Some(b"\x1bOC".to_vec())
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::ArrowLeft), None, false, modes),
            Some(b"\x1bOD".to_vec())
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::Home), None, false, modes),
            Some(b"\x1bOH".to_vec())
        );
        assert_eq!(
            encode_key_input(&Key::Named(NamedKey::End), None, false, modes),
            Some(b"\x1bOF".to_vec())
        );
    }

    #[test]
    fn key_encoder_handles_xterm_navigation_and_function_keys() {
        let cases = [
            (Key::Named(NamedKey::Home), b"\x1b[H".as_slice()),
            (Key::Named(NamedKey::End), b"\x1b[F".as_slice()),
            (Key::Named(NamedKey::Insert), b"\x1b[2~".as_slice()),
            (Key::Named(NamedKey::Delete), b"\x1b[3~".as_slice()),
            (Key::Named(NamedKey::PageUp), b"\x1b[5~".as_slice()),
            (Key::Named(NamedKey::PageDown), b"\x1b[6~".as_slice()),
            (Key::Named(NamedKey::F1), b"\x1bOP".as_slice()),
            (Key::Named(NamedKey::F2), b"\x1bOQ".as_slice()),
            (Key::Named(NamedKey::F3), b"\x1bOR".as_slice()),
            (Key::Named(NamedKey::F4), b"\x1bOS".as_slice()),
            (Key::Named(NamedKey::F5), b"\x1b[15~".as_slice()),
            (Key::Named(NamedKey::F6), b"\x1b[17~".as_slice()),
            (Key::Named(NamedKey::F7), b"\x1b[18~".as_slice()),
            (Key::Named(NamedKey::F8), b"\x1b[19~".as_slice()),
            (Key::Named(NamedKey::F9), b"\x1b[20~".as_slice()),
            (Key::Named(NamedKey::F10), b"\x1b[21~".as_slice()),
            (Key::Named(NamedKey::F11), b"\x1b[23~".as_slice()),
            (Key::Named(NamedKey::F12), b"\x1b[24~".as_slice()),
        ];

        for (key, expected) in cases {
            assert_eq!(
                encode_key_input(&key, None, false, TerminalInputModes::default()),
                Some(expected.to_vec())
            );
        }
    }

    #[test]
    fn key_encoder_parameterizes_modified_navigation_and_function_keys() {
        let shift = TerminalKeyModifiers {
            shift: true,
            ..TerminalKeyModifiers::default()
        };
        let alt = TerminalKeyModifiers {
            alt: true,
            ..TerminalKeyModifiers::default()
        };
        let control = TerminalKeyModifiers {
            control: true,
            ..TerminalKeyModifiers::default()
        };
        let shift_control = TerminalKeyModifiers {
            shift: true,
            control: true,
            ..TerminalKeyModifiers::default()
        };
        let alt_control = TerminalKeyModifiers {
            alt: true,
            control: true,
            ..TerminalKeyModifiers::default()
        };
        let all = TerminalKeyModifiers {
            shift: true,
            alt: true,
            control: true,
            ..TerminalKeyModifiers::default()
        };
        let app_cursor_modes = TerminalInputModes {
            application_cursor_keys: true,
            application_keypad: false,
            ..TerminalInputModes::default()
        };

        let cases = [
            (
                Key::Named(NamedKey::ArrowUp),
                shift,
                b"\x1b[1;2A".as_slice(),
            ),
            (
                Key::Named(NamedKey::ArrowLeft),
                control,
                b"\x1b[1;5D".as_slice(),
            ),
            (Key::Named(NamedKey::Home), alt, b"\x1b[1;3H".as_slice()),
            (
                Key::Named(NamedKey::End),
                shift_control,
                b"\x1b[1;6F".as_slice(),
            ),
            (Key::Named(NamedKey::Insert), shift, b"\x1b[2;2~".as_slice()),
            (
                Key::Named(NamedKey::Delete),
                control,
                b"\x1b[3;5~".as_slice(),
            ),
            (
                Key::Named(NamedKey::PageUp),
                alt_control,
                b"\x1b[5;7~".as_slice(),
            ),
            (Key::Named(NamedKey::PageDown), all, b"\x1b[6;8~".as_slice()),
            (Key::Named(NamedKey::F1), shift, b"\x1b[1;2P".as_slice()),
            (Key::Named(NamedKey::F5), control, b"\x1b[15;5~".as_slice()),
        ];

        for (key, modifiers, expected) in cases {
            assert_eq!(
                encode_key_input_with_modifiers(&key, None, modifiers, app_cursor_modes),
                Some(expected.to_vec())
            );
        }

        let meta_shift = TerminalKeyModifiers {
            shift: true,
            meta: true,
            ..TerminalKeyModifiers::default()
        };
        assert_eq!(
            encode_key_input_with_modifiers(
                &Key::Named(NamedKey::ArrowUp),
                None,
                meta_shift,
                TerminalInputModes::default(),
            ),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn key_encoder_separates_top_row_and_keypad_digit_modes() {
        let keypad_modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };
        let top_row_one = Key::Character("1".into());
        let keypad_one = TerminalKeyInput {
            logical_key: &top_row_one,
            text: Some("1"),
            modifiers: TerminalKeyModifiers::default(),
            keypad_key: Some(KeypadKey::Digit(1)),
        };

        assert_eq!(
            encode_key_input(&top_row_one, Some("1"), false, keypad_modes),
            Some(b"1".to_vec())
        );
        assert_eq!(
            encode_terminal_key_input(keypad_one, TerminalInputModes::default()),
            Some(b"1".to_vec())
        );
        assert_eq!(
            encode_terminal_key_input(keypad_one, keypad_modes),
            Some(b"\x1bOq".to_vec())
        );
    }

    #[test]
    fn key_encoder_uses_application_keypad_sequences() {
        let modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };
        let cases = [
            (KeypadKey::Digit(0), "0", b"\x1bOp".as_slice()),
            (KeypadKey::Digit(9), "9", b"\x1bOy".as_slice()),
            (KeypadKey::Decimal, ".", b"\x1bOn".as_slice()),
            (KeypadKey::Comma, ",", b"\x1bOl".as_slice()),
            (KeypadKey::Add, "+", b"\x1bOk".as_slice()),
            (KeypadKey::Subtract, "-", b"\x1bOm".as_slice()),
            (KeypadKey::Multiply, "*", b"\x1bOj".as_slice()),
            (KeypadKey::Divide, "/", b"\x1bOo".as_slice()),
        ];

        for (keypad_key, text, expected) in cases {
            let logical_key = Key::Character(text.into());
            assert_eq!(
                encode_terminal_key_input(
                    TerminalKeyInput {
                        logical_key: &logical_key,
                        text: Some(text),
                        modifiers: TerminalKeyModifiers::default(),
                        keypad_key: Some(keypad_key),
                    },
                    modes,
                ),
                Some(expected.to_vec())
            );
        }

        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            encode_terminal_key_input(
                TerminalKeyInput {
                    logical_key: &enter,
                    text: None,
                    modifiers: TerminalKeyModifiers::default(),
                    keypad_key: Some(KeypadKey::Enter),
                },
                modes,
            ),
            Some(b"\x1bOM".to_vec())
        );
    }

    #[test]
    fn key_encoder_keeps_main_enter_and_unsupported_keypad_fallbacks() {
        let modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };
        let enter = Key::Named(NamedKey::Enter);
        let equal = Key::Character("=".into());

        assert_eq!(
            encode_key_input(&enter, None, false, modes),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_terminal_key_input(
                TerminalKeyInput {
                    logical_key: &equal,
                    text: Some("="),
                    modifiers: TerminalKeyModifiers::default(),
                    keypad_key: Some(KeypadKey::Equal),
                },
                modes,
            ),
            Some(b"=".to_vec())
        );
        assert_eq!(
            encode_terminal_key_input(
                TerminalKeyInput {
                    logical_key: &Key::Character("1".into()),
                    text: Some("1"),
                    modifiers: TerminalKeyModifiers {
                        control: true,
                        ..TerminalKeyModifiers::default()
                    },
                    keypad_key: Some(KeypadKey::Digit(1)),
                },
                modes,
            ),
            None
        );
    }

    #[test]
    fn native_keypad_mapper_uses_physical_code_or_numpad_location() {
        let digit_one = Key::Character("1".into());
        let enter = Key::Named(NamedKey::Enter);

        assert_eq!(
            keypad_key_from_winit_key_code(KeyCode::Numpad1),
            Some(KeypadKey::Digit(1))
        );
        assert_eq!(
            keypad_key_from_winit_key_code(KeyCode::NumpadEnter),
            Some(KeypadKey::Enter)
        );
        assert_eq!(
            keypad_key_from_winit_location(&digit_one, Some("1"), KeyLocation::Standard),
            None
        );
        assert_eq!(
            keypad_key_from_winit_location(&digit_one, Some("1"), KeyLocation::Numpad),
            Some(KeypadKey::Digit(1))
        );
        assert_eq!(
            keypad_key_from_winit_location(&enter, None, KeyLocation::Numpad),
            Some(KeypadKey::Enter)
        );
    }

    #[test]
    fn key_encoder_handles_control_characters() {
        assert_eq!(
            encode_key_input(
                &Key::Character("c".into()),
                Some("c"),
                true,
                TerminalInputModes::default()
            ),
            Some(vec![3])
        );
        assert_eq!(
            encode_key_input(
                &Key::Character("[".into()),
                Some("["),
                true,
                TerminalInputModes::default()
            ),
            Some(vec![0x1b])
        );
    }

    #[test]
    fn pointer_position_maps_to_clamped_cell() {
        let metrics = CellMetrics::default();
        let size = GridSize::new(24, 80);

        assert_eq!(
            cell_point_for_position(PhysicalPosition::new(8.0, 8.0), metrics, size),
            CellPoint::new(0, 0)
        );
        assert_eq!(
            cell_point_for_position(PhysicalPosition::new(17.0, 26.0), metrics, size),
            CellPoint::new(1, 1)
        );
        assert_eq!(
            cell_point_for_position(PhysicalPosition::new(-10.0, 9999.0), metrics, size),
            CellPoint::new(23, 0)
        );
    }

    #[test]
    fn hyperlink_position_hit_test_uses_snapshot_metadata() {
        let metrics = CellMetrics::default();
        let size = GridSize::new(1, 8);
        let mut snapshot = RenderSnapshot::from_plain_lines(&["a界b"]);
        snapshot.rows[0].cells[1].hyperlink = Some(5);
        snapshot.hyperlinks = vec![TerminalHyperlink {
            id: 5,
            uri: "https://example.com".to_owned(),
            osc8_id: None,
        }];

        let position = PhysicalPosition::new(
            f64::from(metrics.padding.x + metrics.cell.width * 2.0),
            f64::from(metrics.padding.y),
        );

        assert_eq!(
            hyperlink_for_position(&snapshot, position, metrics, size)
                .map(|link| link.uri.as_str()),
            Some("https://example.com")
        );
    }

    #[test]
    fn hyperlink_activation_click_requires_platform_modifier() {
        let plain = ModifiersState::empty();
        let control = ModifiersState::CONTROL;
        let super_key = ModifiersState::SUPER;

        assert!(!is_hyperlink_activation_click(
            ElementState::Released,
            MouseButton::Left,
            control
        ));
        assert!(!is_hyperlink_activation_click(
            ElementState::Pressed,
            MouseButton::Right,
            control
        ));
        assert!(!is_hyperlink_activation_click(
            ElementState::Pressed,
            MouseButton::Left,
            plain
        ));

        #[cfg(target_os = "macos")]
        {
            assert!(is_hyperlink_activation_click(
                ElementState::Pressed,
                MouseButton::Left,
                super_key
            ));
            assert!(!is_hyperlink_activation_click(
                ElementState::Pressed,
                MouseButton::Left,
                control
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert!(is_hyperlink_activation_click(
                ElementState::Pressed,
                MouseButton::Left,
                control
            ));
            assert!(!is_hyperlink_activation_click(
                ElementState::Pressed,
                MouseButton::Left,
                super_key
            ));
        }
    }

    #[test]
    fn ordered_cell_range_normalizes_drag_direction() {
        assert_eq!(
            ordered_cell_range(CellPoint::new(2, 4), CellPoint::new(1, 7)),
            CellRange {
                start: CellPoint::new(1, 7),
                end: CellPoint::new(2, 4),
            }
        );
        assert_eq!(
            ordered_cell_range(CellPoint::new(1, 2), CellPoint::new(1, 5)),
            CellRange {
                start: CellPoint::new(1, 2),
                end: CellPoint::new(1, 5),
            }
        );
    }

    #[test]
    fn left_press_single_click_collapses_selection_and_anchors_drag() {
        let terminal = BasicTerminal::new(GridSize::new(3, 12));
        let point = CellPoint::new(1, 4);

        let action = selection_for_left_press(&terminal, None, point, Instant::now());

        assert_eq!(action.range, collapsed_range(point));
        assert_eq!(action.anchor, Some(point));
        assert!(!action.completed);
    }

    #[test]
    fn left_press_double_click_expands_word_and_disables_drag_anchor() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 20));
        let first_point = CellPoint::new(0, 8);
        let now = Instant::now();

        terminal.feed(b"cat src/main.rs");
        let first = selection_for_left_press(&terminal, None, first_point, now);
        let second = selection_for_left_press(
            &terminal,
            Some(first.click),
            CellPoint::new(0, 9),
            now + Duration::from_millis(100),
        );

        assert_eq!(
            second.range,
            CellRange {
                start: CellPoint::new(0, 4),
                end: CellPoint::new(0, 14),
            }
        );
        assert_eq!(second.anchor, None);
        assert!(second.completed);
    }

    #[test]
    fn selection_release_publish_requires_non_collapsed_drag_selection() {
        let anchor = CellPoint::new(1, 4);

        assert!(!selection_release_should_publish(
            Some(anchor),
            Some(collapsed_range(anchor))
        ));
        assert!(selection_release_should_publish(
            Some(anchor),
            Some(CellRange {
                start: anchor,
                end: CellPoint::new(1, 6),
            })
        ));
        assert!(!selection_release_should_publish(
            None,
            Some(CellRange {
                start: anchor,
                end: CellPoint::new(1, 6),
            })
        ));
        assert!(!selection_release_should_publish(Some(anchor), None));
    }

    #[test]
    fn mouse_override_policy_uses_shift_for_local_selection_gestures() {
        let shift = ModifiersState::SHIFT;

        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                shift,
                MouseLocalOverrideEvent::Button {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                },
                None,
            ),
            MouseLocalOverrideAction::Selection
        );
        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                shift,
                MouseLocalOverrideEvent::Button {
                    state: ElementState::Pressed,
                    button: MouseButton::Middle,
                },
                None,
            ),
            MouseLocalOverrideAction::PrimaryPaste
        );
        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                shift,
                MouseLocalOverrideEvent::Wheel,
                None,
            ),
            MouseLocalOverrideAction::Scrollback
        );
        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                shift,
                MouseLocalOverrideEvent::Button {
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                },
                None,
            ),
            MouseLocalOverrideAction::None
        );
    }

    #[test]
    fn mouse_override_policy_keeps_raw_xterm_mode_when_disabled() {
        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::Disabled,
                ModifiersState::SHIFT,
                MouseLocalOverrideEvent::Button {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                },
                None,
            ),
            MouseLocalOverrideAction::None
        );
    }

    #[test]
    fn mouse_override_policy_keeps_active_drag_local_after_shift_release() {
        let anchor = Some(CellPoint::new(1, 2));

        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                ModifiersState::empty(),
                MouseLocalOverrideEvent::Motion,
                anchor,
            ),
            MouseLocalOverrideAction::Selection
        );
        assert_eq!(
            mouse_local_override_action(
                true,
                MouseSelectionOverridePolicy::ShiftSelect,
                ModifiersState::empty(),
                MouseLocalOverrideEvent::Button {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                },
                anchor,
            ),
            MouseLocalOverrideAction::Selection
        );
    }

    #[test]
    fn left_double_click_requires_interval_and_cell_tolerance() {
        let now = Instant::now();
        let previous = ClickStamp {
            point: CellPoint::new(2, 3),
            at: now,
        };

        assert!(is_left_double_click(
            Some(previous),
            CellPoint::new(2, 4),
            now + Duration::from_millis(500)
        ));
        assert!(!is_left_double_click(
            Some(previous),
            CellPoint::new(2, 4),
            now + Duration::from_millis(501)
        ));
        assert!(!is_left_double_click(
            Some(previous),
            CellPoint::new(0, 3),
            now + Duration::from_millis(100)
        ));
        assert!(!is_left_double_click(
            Some(previous),
            CellPoint::new(2, 5),
            now + Duration::from_millis(100)
        ));
    }

    #[test]
    fn scroll_delta_maps_to_terminal_lines() {
        assert_eq!(
            scroll_lines_for_delta(
                MouseScrollDelta::LineDelta(0.0, 3.2),
                CellMetrics::default()
            ),
            3
        );
        assert_eq!(
            scroll_lines_for_delta(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 36.0)),
                CellMetrics::default()
            ),
            2
        );
        assert_eq!(
            scroll_lines_for_delta(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -1.0)),
                CellMetrics::default()
            ),
            -1
        );
    }

    fn native_mouse_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::Sgr,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    #[test]
    fn native_mouse_reporter_ignores_disabled_reporting() {
        let mut reporter = NativeMouseReportState::default();
        let modes = TerminalMouseModes::default();

        assert_eq!(
            reporter.button(
                ElementState::Pressed,
                MouseButtonCode::Left,
                CellPoint::new(0, 0),
                None,
                MouseModifiers::NONE,
                modes,
            ),
            None
        );
        assert_eq!(
            reporter.motion(CellPoint::new(0, 1), None, MouseModifiers::NONE, modes),
            None
        );
        assert_eq!(
            reporter.wheel(
                MouseButtonCode::WheelUp,
                CellPoint::new(0, 1),
                None,
                MouseModifiers::NONE,
                modes,
            ),
            None
        );
        assert_eq!(reporter.pressed_button, None);
        assert_eq!(reporter.last_reported_cell, None);
    }

    #[test]
    fn native_mouse_reporter_encodes_press_release_and_wheel() {
        let mut reporter = NativeMouseReportState::default();
        let modes = native_mouse_modes(MouseTrackingMode::Normal);
        let cell = CellPoint::new(0, 0);

        assert_eq!(
            reporter.button(
                ElementState::Pressed,
                MouseButtonCode::Left,
                cell,
                None,
                MouseModifiers::NONE,
                modes,
            ),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(reporter.pressed_button, Some(MouseButtonCode::Left));
        assert_eq!(
            reporter.button(
                ElementState::Released,
                MouseButtonCode::Left,
                cell,
                None,
                MouseModifiers::NONE,
                modes,
            ),
            Some(b"\x1b[<0;1;1m".to_vec())
        );
        assert_eq!(reporter.pressed_button, None);

        assert_eq!(
            reporter.wheel(
                MouseButtonCode::WheelUp,
                CellPoint::new(1, 2),
                None,
                MouseModifiers {
                    shift: true,
                    ..MouseModifiers::NONE
                },
                modes,
            ),
            Some(b"\x1b[<68;3;2M".to_vec())
        );
    }

    #[test]
    fn native_button_event_mouse_reports_drag_only_with_pressed_button_and_cell_change() {
        let mut reporter = NativeMouseReportState::default();
        let modes = native_mouse_modes(MouseTrackingMode::ButtonEvent);
        let origin = CellPoint::new(0, 0);

        assert_eq!(
            reporter.motion(origin, None, MouseModifiers::NONE, modes),
            None
        );
        assert_eq!(
            reporter.button(
                ElementState::Pressed,
                MouseButtonCode::Left,
                origin,
                None,
                MouseModifiers::NONE,
                modes,
            ),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            reporter.motion(origin, None, MouseModifiers::NONE, modes),
            None
        );
        assert_eq!(
            reporter.motion(CellPoint::new(0, 1), None, MouseModifiers::NONE, modes),
            Some(b"\x1b[<32;2;1M".to_vec())
        );
    }

    #[test]
    fn native_any_event_mouse_reports_motion_without_pressed_button() {
        let mut reporter = NativeMouseReportState::default();
        let modes = native_mouse_modes(MouseTrackingMode::AnyEvent);

        assert_eq!(
            reporter.motion(CellPoint::new(2, 3), None, MouseModifiers::NONE, modes),
            Some(b"\x1b[<35;4;3M".to_vec())
        );
    }

    #[test]
    fn native_mouse_reporter_passes_pixel_positions_to_sgr_pixel_encoder() {
        let mut reporter = NativeMouseReportState::default();
        let modes = TerminalMouseModes {
            tracking: MouseTrackingMode::Normal,
            encoding: MouseEncodingMode::SgrPixels,
            focus_events: false,
            alternate_scroll: false,
        };

        assert_eq!(
            reporter.button(
                ElementState::Pressed,
                MouseButtonCode::Left,
                CellPoint::new(9, 9),
                Some(PixelMousePosition::new(20, 40)),
                MouseModifiers::NONE,
                modes,
            ),
            Some(b"\x1b[<0;21;41M".to_vec())
        );
    }

    #[test]
    fn command_shortcuts_pick_builtin_and_first_external_command() {
        let mut commands = vec![
            CommandRegistration {
                id: "witty.about".to_owned(),
                title: "About".to_owned(),
                source_plugin: "builtin".to_owned(),
            },
            CommandRegistration {
                id: "fixture.echo".to_owned(),
                title: "Fixture Echo".to_owned(),
                source_plugin: "fixture".to_owned(),
            },
        ];
        commands.splice(1..1, search_command_registrations());

        assert_eq!(
            command_shortcut_for_key(&Key::Named(NamedKey::F1), &commands),
            Some("witty.about".to_owned())
        );
        assert_eq!(
            command_shortcut_for_key(&Key::Named(NamedKey::F2), &commands),
            Some("fixture.echo".to_owned())
        );
    }

    #[test]
    fn diagnostics_shortcut_uses_f3() {
        assert!(is_frame_diagnostics_shortcut(&Key::Named(NamedKey::F3)));
        assert!(!is_frame_diagnostics_shortcut(&Key::Named(NamedKey::F2)));
    }

    #[test]
    fn fullscreen_shortcut_uses_unmodified_f11() {
        assert!(is_toggle_fullscreen_shortcut(
            &Key::Named(NamedKey::F11),
            Modifiers::from(ModifiersState::empty())
        ));
        assert!(!is_toggle_fullscreen_shortcut(
            &Key::Named(NamedKey::F11),
            Modifiers::from(ModifiersState::SHIFT)
        ));
        assert!(!is_toggle_fullscreen_shortcut(
            &Key::Named(NamedKey::F10),
            Modifiers::from(ModifiersState::empty())
        ));
    }

    #[test]
    fn fullscreen_size_degradation_allows_small_compositor_drift() {
        assert!(!fullscreen_inner_size_is_degraded(
            PhysicalSize::new(1916, 1076),
            PhysicalSize::new(1920, 1080)
        ));
        assert!(fullscreen_inner_size_is_degraded(
            PhysicalSize::new(1280, 720),
            PhysicalSize::new(1920, 1080)
        ));
        assert!(fullscreen_inner_size_is_degraded(
            PhysicalSize::new(1920, 900),
            PhysicalSize::new(1920, 1080)
        ));
    }

    #[test]
    fn fullscreen_effectiveness_requires_flag_and_non_degraded_size() {
        assert!(!fullscreen_is_effective_for_size(
            false,
            PhysicalSize::new(1920, 1080),
            Some(PhysicalSize::new(1920, 1080))
        ));
        assert!(fullscreen_is_effective_for_size(
            true,
            PhysicalSize::new(960, 540),
            None
        ));
        assert!(fullscreen_is_effective_for_size(
            true,
            PhysicalSize::new(1920, 1080),
            Some(PhysicalSize::new(1920, 1080))
        ));
        assert!(!fullscreen_is_effective_for_size(
            true,
            PhysicalSize::new(960, 540),
            Some(PhysicalSize::new(1920, 1080))
        ));
    }

    #[test]
    fn runtime_font_size_shortcuts_use_control_plus_minus_and_zero() {
        let control = Modifiers::from(ModifiersState::CONTROL);
        let control_shift = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);
        let control_alt = Modifiers::from(ModifiersState::CONTROL | ModifiersState::ALT);

        assert_eq!(
            runtime_font_size_shortcut_action_for_parts(
                &Key::Character("=".into()),
                PhysicalKey::Code(KeyCode::Equal),
                control,
            ),
            Some(RuntimeFontSizeAction::Increase)
        );
        assert_eq!(
            runtime_font_size_shortcut_action_for_parts(
                &Key::Character("+".into()),
                PhysicalKey::Code(KeyCode::Equal),
                control_shift,
            ),
            Some(RuntimeFontSizeAction::Increase)
        );
        assert_eq!(
            runtime_font_size_shortcut_action_for_parts(
                &Key::Character("-".into()),
                PhysicalKey::Code(KeyCode::Minus),
                control,
            ),
            Some(RuntimeFontSizeAction::Decrease)
        );
        assert_eq!(
            runtime_font_size_shortcut_action_for_parts(
                &Key::Character("0".into()),
                PhysicalKey::Code(KeyCode::Digit0),
                control,
            ),
            Some(RuntimeFontSizeAction::Reset)
        );
        assert_eq!(
            runtime_font_size_shortcut_action_for_parts(
                &Key::Character("+".into()),
                PhysicalKey::Code(KeyCode::NumpadAdd),
                control_alt,
            ),
            None
        );
    }

    #[test]
    fn runtime_font_size_actions_clamp_reset_and_preserve_family() {
        assert_eq!(
            runtime_font_size_after_action(14, RuntimeFontSizeAction::Increase),
            15
        );
        assert_eq!(
            runtime_font_size_after_action(14, RuntimeFontSizeAction::Decrease),
            13
        );
        assert_eq!(
            runtime_font_size_after_action(MAX_TERMINAL_FONT_SIZE, RuntimeFontSizeAction::Increase),
            MAX_TERMINAL_FONT_SIZE
        );
        assert_eq!(
            runtime_font_size_after_action(MIN_TERMINAL_FONT_SIZE, RuntimeFontSizeAction::Decrease),
            MIN_TERMINAL_FONT_SIZE
        );
        assert_eq!(
            runtime_font_size_after_action(22, RuntimeFontSizeAction::Reset),
            DEFAULT_TERMINAL_FONT_SIZE
        );

        let config =
            RendererFontConfig::with_font_size(Some("JetBrainsMono Nerd Font".to_owned()), 18)
                .with_terminal_padding(8.0);
        let next = runtime_font_config_after_action(&config, RuntimeFontSizeAction::Increase);

        assert_eq!(next.family(), Some("JetBrainsMono Nerd Font"));
        assert_eq!(next.font_size(), 19);
        assert_eq!(next.terminal_padding(), 8.0);
    }

    #[test]
    fn search_shortcut_uses_control_shift_f() {
        let modifiers = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(is_search_shortcut(&Key::Character("f".into()), modifiers));
        assert!(is_search_shortcut(&Key::Character("F".into()), modifiers));
        assert!(!is_search_shortcut(
            &Key::Character("f".into()),
            Modifiers::from(ModifiersState::CONTROL)
        ));
    }

    #[test]
    fn new_local_tab_shortcut_uses_control_shift_t() {
        let modifiers = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(is_new_local_tab_shortcut(
            &Key::Character("t".into()),
            modifiers
        ));
        assert!(is_new_local_tab_shortcut(
            &Key::Character("T".into()),
            modifiers
        ));
        assert!(!is_new_local_tab_shortcut(
            &Key::Character("t".into()),
            Modifiers::from(ModifiersState::CONTROL)
        ));
    }

    #[test]
    fn session_switcher_shortcut_uses_control_tab() {
        let control = Modifiers::from(ModifiersState::CONTROL);
        let control_shift = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);
        let control_alt = Modifiers::from(ModifiersState::CONTROL | ModifiersState::ALT);

        assert!(is_session_switcher_shortcut(
            &Key::Named(NamedKey::Tab),
            control
        ));
        assert!(is_session_switcher_shortcut(
            &Key::Named(NamedKey::Tab),
            control_shift
        ));
        assert!(!is_session_switcher_shortcut(
            &Key::Named(NamedKey::Tab),
            control_alt
        ));
        assert!(!is_session_switcher_shortcut(
            &Key::Named(NamedKey::Enter),
            control
        ));
    }

    #[test]
    fn copy_selection_shortcut_uses_control_shift_c() {
        let modifiers = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(is_copy_selection_shortcut(
            &Key::Character("c".into()),
            modifiers
        ));
        assert!(is_copy_selection_shortcut(
            &Key::Character("C".into()),
            modifiers
        ));
        assert!(!is_copy_selection_shortcut(
            &Key::Character("c".into()),
            Modifiers::from(ModifiersState::CONTROL)
        ));
    }

    #[test]
    fn paste_clipboard_shortcut_uses_control_shift_v() {
        let modifiers = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert!(is_paste_clipboard_shortcut(
            &Key::Character("v".into()),
            modifiers
        ));
        assert!(is_paste_clipboard_shortcut(
            &Key::Character("V".into()),
            modifiers
        ));
        assert!(!is_paste_clipboard_shortcut(
            &Key::Character("v".into()),
            Modifiers::from(ModifiersState::CONTROL)
        ));
    }

    #[test]
    fn search_key_action_consumes_find_bar_keys_without_terminal_input() {
        let none = Modifiers::from(ModifiersState::empty());
        let shift = Modifiers::from(ModifiersState::SHIFT);
        let alt = Modifiers::from(ModifiersState::ALT);
        let control_shift = Modifiers::from(ModifiersState::CONTROL | ModifiersState::SHIFT);

        assert_eq!(
            search_key_action(&Key::Named(NamedKey::Escape), None, none),
            SearchKeyAction::Close
        );
        assert_eq!(
            search_key_action(&Key::Named(NamedKey::Enter), None, none),
            SearchKeyAction::Next
        );
        assert_eq!(
            search_key_action(&Key::Named(NamedKey::Enter), None, shift),
            SearchKeyAction::Previous
        );
        assert_eq!(
            search_key_action(&Key::Named(NamedKey::Backspace), None, none),
            SearchKeyAction::Backspace
        );
        assert_eq!(
            search_key_action(&Key::Named(NamedKey::ArrowUp), None, none),
            SearchKeyAction::HistoryPrevious
        );
        assert_eq!(
            search_key_action(&Key::Named(NamedKey::ArrowDown), None, none),
            SearchKeyAction::HistoryNext
        );
        assert_eq!(
            search_key_action(&Key::Character("x".into()), Some("x"), none),
            SearchKeyAction::InputText("x".to_owned())
        );
        assert_eq!(
            search_key_action(&Key::Character("c".into()), Some("c"), alt),
            SearchKeyAction::ToggleCaseSensitive
        );
        assert_eq!(
            search_key_action(&Key::Character("r".into()), Some("r"), alt),
            SearchKeyAction::ToggleRegex
        );
        assert_eq!(
            search_key_action(&Key::Character("w".into()), Some("w"), alt),
            SearchKeyAction::ToggleWholeWord
        );
        assert_eq!(
            search_key_action(&Key::Character("n".into()), Some("n"), alt),
            SearchKeyAction::ToggleNormalizeNfc
        );
        assert_eq!(
            search_key_action(&Key::Character("c".into()), Some("c"), control_shift),
            SearchKeyAction::None
        );
    }

    #[test]
    fn search_commands_apply_locally_to_find_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 16));
        let mut search = TerminalSearch::default();
        terminal.feed(b"alpha\r\nbeta\r\nalpha");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 4),
        }));

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_OPEN_COMMAND_ID
        ));
        assert!(search.is_open());
        assert_eq!(search.query(), "alpha");
        assert_eq!(search.match_count(), 2);
        assert_eq!(search.active_index(), Some(0));

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_NEXT_COMMAND_ID
        ));
        assert_eq!(search.active_index(), Some(1));

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_PREVIOUS_COMMAND_ID
        ));
        assert_eq!(search.active_index(), Some(0));

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_CLOSE_COMMAND_ID
        ));
        assert!(!search.is_open());
        assert_eq!(search.query(), "");

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_NEXT_COMMAND_ID
        ));
        assert!(search.is_open());
        assert_eq!(search.query(), "alpha");
        assert_eq!(search.active_index(), Some(0));

        assert!(apply_search_command(
            &mut terminal,
            &mut search,
            SEARCH_CLOSE_COMMAND_ID
        ));
        assert!(!search.is_open());
        assert!(!apply_search_command(
            &mut terminal,
            &mut search,
            "plugin.echo"
        ));
    }

    #[test]
    fn copy_selection_writes_selected_text_to_clipboard() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 4),
        }));

        let copied = copy_selection_to_clipboard(&terminal, &mut clipboard).unwrap();

        assert!(copied);
        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Clipboard, "ello".to_owned())]
        );
    }

    #[test]
    fn copy_selection_to_clipboard_and_clear_clears_selection_after_success() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 4),
        }));

        let copied =
            copy_selection_to_clipboard_and_clear(&mut terminal, &mut clipboard).unwrap();

        assert!(copied);
        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Clipboard, "ello".to_owned())]
        );
        assert_eq!(terminal.selected_text(), None);
    }

    #[test]
    fn copy_selection_to_clipboard_and_clear_clears_empty_selection() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 2),
        }));

        let copied =
            copy_selection_to_clipboard_and_clear(&mut terminal, &mut clipboard).unwrap();

        assert!(!copied);
        assert!(clipboard.writes.is_empty());
        assert_eq!(terminal.selected_text(), None);
    }

    #[test]
    fn copy_selection_to_clipboard_and_clear_preserves_selection_on_write_error() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard {
            clipboard_write_error: Some("clipboard offline".to_owned()),
            ..RecordingClipboard::default()
        };

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 4),
        }));

        let err =
            copy_selection_to_clipboard_and_clear(&mut terminal, &mut clipboard).unwrap_err();

        assert!(err.to_string().contains("clipboard offline"));
        assert!(clipboard.writes.is_empty());
        assert_eq!(terminal.selected_text().as_deref(), Some("ello"));
    }

    #[test]
    fn copy_selection_to_target_can_write_primary_selection() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 4),
        }));

        let copied =
            copy_selection_to_target(&terminal, &mut clipboard, ClipboardSelection::Primary)
                .unwrap();

        assert!(copied);
        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Primary, "ello".to_owned())]
        );
    }

    #[test]
    fn copy_selection_to_primary_uses_primary_target() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 4),
        }));

        let copied = copy_selection_to_primary(&terminal, &mut clipboard).unwrap();

        assert!(copied);
        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Primary, "ello".to_owned())]
        );
    }

    #[test]
    fn selection_copy_regression_smoke_validates_multiline_copy_payload() {
        assert_eq!(selection_copy_regression_smoke().unwrap(), "bc\nde");
    }

    #[test]
    fn primary_selection_boundary_smoke_uses_primary_target_only() {
        assert_eq!(primary_selection_boundary_smoke().unwrap(), "middle");
    }

    #[test]
    fn primary_selection_gui_smoke_roundtrips_double_click_to_middle_click_paste() {
        let smoke = primary_selection_gui_smoke().unwrap();

        assert_eq!(smoke.copied, "middle");
        assert_eq!(smoke.pasted, b"\x1b[200~middle\x1b[201~");
    }

    #[test]
    fn copy_selection_skips_empty_selection_payload() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let mut clipboard = RecordingClipboard::default();

        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 2),
        }));

        let copied = copy_selection_to_clipboard(&terminal, &mut clipboard).unwrap();

        assert!(!copied);
        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn clipboard_paste_text_reads_non_empty_clipboard() {
        let mut clipboard = RecordingClipboard {
            clipboard_read: "cargo test\n".to_owned(),
            ..RecordingClipboard::default()
        };

        assert_eq!(
            clipboard_paste_text(&mut clipboard).unwrap().as_deref(),
            Some("cargo test\n")
        );
    }

    #[test]
    fn clipboard_paste_text_skips_empty_clipboard() {
        let mut clipboard = RecordingClipboard::default();

        assert_eq!(clipboard_paste_text(&mut clipboard).unwrap(), None);
    }

    #[test]
    fn paste_clipboard_to_input_writes_clipboard_bytes() {
        let mut clipboard = RecordingClipboard {
            clipboard_read: "echo ok\n".to_owned(),
            ..RecordingClipboard::default()
        };
        let mut input = Vec::new();

        let pasted = paste_clipboard_to_input(&mut clipboard, false, |bytes| {
            input.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();

        assert!(pasted);
        assert_eq!(input, b"echo ok\n");
    }

    #[test]
    fn paste_clipboard_to_input_saves_image_and_writes_png_path_when_text_is_unavailable() {
        let mut clipboard = RecordingClipboard {
            clipboard_text_error: Some("clipboard does not contain text".to_owned()),
            clipboard_image: Some(ClipboardImage {
                width: 1,
                height: 1,
                rgba: vec![0xff, 0x00, 0x00, 0xff],
            }),
            ..RecordingClipboard::default()
        };
        let mut input = Vec::new();

        let pasted = paste_clipboard_to_input(&mut clipboard, false, |bytes| {
            input.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();

        assert!(pasted);
        let path = std::str::from_utf8(&input).unwrap();
        assert!(path.contains("witty-clipboard-image-"), "{path}");
        assert!(path.ends_with(".png"), "{path}");
        let png = std::fs::read(path).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn paste_clipboard_to_input_wraps_bracketed_paste_payload() {
        let mut clipboard = RecordingClipboard {
            clipboard_read: "echo ok\n".to_owned(),
            ..RecordingClipboard::default()
        };
        let mut input = Vec::new();

        let pasted = paste_clipboard_to_input(&mut clipboard, true, |bytes| {
            input.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();

        assert!(pasted);
        assert_eq!(input, b"\x1b[200~echo ok\n\x1b[201~");
    }

    #[test]
    fn paste_selection_to_input_reads_primary_selection() {
        let mut clipboard = RecordingClipboard {
            primary_read: "middle paste\n".to_owned(),
            ..RecordingClipboard::default()
        };
        let mut input = Vec::new();

        let pasted = paste_selection_to_input(
            &mut clipboard,
            ClipboardSelection::Primary,
            false,
            |bytes| {
                input.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();

        assert!(pasted);
        assert_eq!(input, b"middle paste\n");
    }

    #[test]
    fn paste_selection_to_input_wraps_primary_bracketed_paste_payload() {
        let mut clipboard = RecordingClipboard {
            primary_read: "middle paste\n".to_owned(),
            ..RecordingClipboard::default()
        };
        let mut input = Vec::new();

        let pasted =
            paste_selection_to_input(&mut clipboard, ClipboardSelection::Primary, true, |bytes| {
                input.extend_from_slice(bytes);
                Ok(())
            })
            .unwrap();

        assert!(pasted);
        assert_eq!(input, b"\x1b[200~middle paste\n\x1b[201~");
    }

    #[test]
    fn native_ime_preedit_updates_state_without_writing_input() {
        let mut composition = ImeComposition::default();

        let result = apply_native_ime_event(
            &mut composition,
            Ime::Preedit("pinyin".to_owned(), Some((2, 4))),
        );

        assert_eq!(
            result,
            NativeImeEventResult {
                changed: true,
                committed_text: None,
            }
        );
        assert!(composition.is_enabled());
        assert!(composition.is_active());
        assert_eq!(composition.preedit(), "pinyin");
        assert_eq!(composition.caret(), Some((2, 4)));
    }

    #[test]
    fn native_ime_commit_clears_preedit_and_returns_commit_text() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("ni".to_owned(), Some((2, 2)));

        let result = apply_native_ime_event(&mut composition, Ime::Commit("你".to_owned()));

        assert_eq!(
            result,
            NativeImeEventResult {
                changed: true,
                committed_text: Some("你".to_owned()),
            }
        );
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
        assert_eq!(composition.preedit(), "");
    }

    #[test]
    fn native_ime_empty_commit_clears_preedit_without_commit_text() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("kana".to_owned(), Some((4, 4)));

        let result = apply_native_ime_event(&mut composition, Ime::Commit(String::new()));

        assert_eq!(
            result,
            NativeImeEventResult {
                changed: true,
                committed_text: None,
            }
        );
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
    }

    #[test]
    fn native_ime_commit_is_routed_to_search_without_terminal_input() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("zhong".to_owned(), Some((5, 5)));
        let mut terminal = BasicTerminal::new(GridSize::new(3, 24));
        terminal.feed("alpha 中 beta".as_bytes());
        let mut search = TerminalSearch::default();
        search.open(&terminal.search_text_rows(), None);
        let terminal_input = Vec::<u8>::new();

        let result = apply_native_ime_event(&mut composition, Ime::Commit("中".to_owned()));
        if let Some(text) = result.committed_text.as_deref() {
            search.input_text(&terminal.search_text_rows(), text);
        }

        assert!(result.changed);
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
        assert_eq!(search.query(), "中");
        assert_eq!(search.match_count(), 1);
        assert!(terminal_input.is_empty());
    }

    #[test]
    fn native_ime_commit_is_routed_to_command_palette_without_terminal_input() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("se".to_owned(), Some((2, 2)));
        let mut palette = CommandPalette::default();
        let commands = vec![CommandRegistration {
            id: "witty.search.open".to_owned(),
            title: "Search: Open".to_owned(),
            source_plugin: "builtin".to_owned(),
        }];
        palette.open(&commands);
        let terminal_input = Vec::<u8>::new();

        let result = apply_native_ime_event(&mut composition, Ime::Commit("搜".to_owned()));
        if let Some(text) = result.committed_text.as_deref() {
            palette.input_text(text);
        }

        assert!(result.changed);
        assert_eq!(palette.query(), "搜");
        assert_eq!(palette.filtered_count(), 0);
        assert!(terminal_input.is_empty());
    }

    #[test]
    fn native_ime_disabled_clears_enabled_and_preedit_state() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("compose".to_owned(), Some((7, 7)));

        let result = apply_native_ime_event(&mut composition, Ime::Disabled);

        assert_eq!(
            result,
            NativeImeEventResult {
                changed: true,
                committed_text: None,
            }
        );
        assert!(!composition.is_enabled());
        assert!(!composition.is_active());
    }

    #[test]
    fn selection_paste_text_can_read_primary_selection() {
        let mut clipboard = RecordingClipboard {
            primary_read: "middle paste\n".to_owned(),
            ..RecordingClipboard::default()
        };

        assert_eq!(
            selection_paste_text(&mut clipboard, ClipboardSelection::Primary)
                .unwrap()
                .as_deref(),
            Some("middle paste\n")
        );
    }

    #[test]
    fn osc52_host_actions_disabled_policy_does_not_write_clipboard() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();

        apply_terminal_host_actions(
            vec![clipboard_write_action(
                TerminalClipboardSelection::Clipboard,
                "secret",
            )],
            Osc52ClipboardPolicy::Disabled,
            &mut clipboard,
            &mut shell_integration,
            reply_sink(),
        )
        .unwrap();

        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn osc52_host_actions_confirm_policy_rejects_without_writing() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();

        let err = apply_terminal_host_actions(
            vec![clipboard_write_action(
                TerminalClipboardSelection::Clipboard,
                "secret",
            )],
            Osc52ClipboardPolicy::Confirm,
            &mut clipboard,
            &mut shell_integration,
            reply_sink(),
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("OSC 52 clipboard confirmation is not implemented"));
        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn osc52_host_actions_allow_policy_writes_clipboard() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();

        apply_terminal_host_actions(
            vec![clipboard_write_action(
                TerminalClipboardSelection::Clipboard,
                "allowed text",
            )],
            Osc52ClipboardPolicy::Allow,
            &mut clipboard,
            &mut shell_integration,
            reply_sink(),
        )
        .unwrap();

        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Clipboard, "allowed text".to_owned())]
        );
    }

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    #[test]
    fn osc52_host_actions_allow_policy_writes_primary_selection_on_linux_like_unix() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();

        apply_terminal_host_actions(
            vec![clipboard_write_action(
                TerminalClipboardSelection::Primary,
                "primary text",
            )],
            Osc52ClipboardPolicy::Allow,
            &mut clipboard,
            &mut shell_integration,
            reply_sink(),
        )
        .unwrap();

        assert_eq!(
            clipboard.writes,
            vec![(ClipboardSelection::Primary, "primary text".to_owned())]
        );
    }

    #[test]
    fn terminal_reply_host_actions_write_bytes_to_transport_sink() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();

        apply_terminal_host_actions(
            vec![
                terminal_reply_action(b"\x1b[0n"),
                terminal_reply_action(b"\x1b[3;5R"),
            ],
            Osc52ClipboardPolicy::Disabled,
            &mut clipboard,
            &mut shell_integration,
            |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(replies, b"\x1b[0n\x1b[3;5R");
        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn bell_host_actions_are_ignored_until_policy_exists() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();

        apply_terminal_host_actions(
            vec![bell_action()],
            Osc52ClipboardPolicy::Allow,
            &mut clipboard,
            &mut shell_integration,
            |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();

        assert!(clipboard.writes.is_empty());
        assert!(replies.is_empty());
        assert_eq!(shell_integration.completed_len(), 0);
    }

    #[test]
    fn shell_integration_host_actions_update_block_state_without_transport_or_clipboard() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();

        apply_terminal_host_actions(
            vec![
                shell_integration_action(TerminalShellIntegrationMarker::PromptStart, 0, 0, None),
                shell_integration_action(TerminalShellIntegrationMarker::CommandStart, 0, 2, None),
                shell_integration_action(TerminalShellIntegrationMarker::OutputStart, 0, 8, None),
                shell_integration_action(
                    TerminalShellIntegrationMarker::CommandFinished,
                    1,
                    3,
                    Some(0),
                ),
            ],
            Osc52ClipboardPolicy::Disabled,
            &mut clipboard,
            &mut shell_integration,
            |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(shell_integration.completed_len(), 1);
        assert!(shell_integration.pending_block().is_none());
        assert!(clipboard.writes.is_empty());
        assert!(replies.is_empty());
    }

    #[test]
    fn current_directory_host_actions_update_shell_integration_state_only() {
        let mut clipboard = RecordingClipboard::default();
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();

        apply_terminal_host_actions(
            vec![current_directory_action("/home/mingxu/project")],
            Osc52ClipboardPolicy::Allow,
            &mut clipboard,
            &mut shell_integration,
            |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            shell_integration
                .current_directory()
                .map(|dir| dir.path.as_str()),
            Some("/home/mingxu/project")
        );
        assert!(clipboard.writes.is_empty());
        assert!(replies.is_empty());
    }

    #[test]
    fn native_command_block_smoke_selects_latest_and_draws_overlay() {
        let smoke = native_command_block_smoke().unwrap();

        assert_eq!(
            smoke,
            NativeCommandBlockSmoke {
                completed_blocks: 1,
                selected_id: Some(0),
                command_copy: "echo native".to_owned(),
                output_copy: "ok".to_owned(),
                overlay_rects: 6,
                frame_backgrounds: 10,
                folded_hidden_rows: 2,
                folded_second_compact_row: Some(1),
                folded_second_glyph_row: Some(1),
                folded_gutter_selected_id: Some(1),
            }
        );
    }

    #[test]
    fn native_command_block_gutter_position_selects_hit_block() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(1, 4),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
                col: 4,
            }),
            exit_code: Some(0),
        });
        let visible_row_anchors = [
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 3,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
            },
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 4,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
            },
        ];
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };

        assert_eq!(
            select_command_block_gutter_hit_for_position(
                &mut shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                PhysicalPosition::new(2.0, 65.0),
                metrics,
                GridSize::new(6, 8),
            ),
            Some(0)
        );
        assert_eq!(shell_integration.selected_block_id(), Some(0));
        assert_eq!(
            select_command_block_gutter_hit_for_position(
                &mut shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                PhysicalPosition::new(10.0, 65.0),
                metrics,
                GridSize::new(6, 8),
            ),
            None
        );
        assert_eq!(shell_integration.selected_block_id(), Some(0));
    }

    #[test]
    fn native_command_block_gutter_position_remaps_folded_compact_rows() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(2, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 32,
                },
                col: 0,
            }),
            exit_code: Some(0),
        });
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(3, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(TerminalShellIntegrationEvent {
            marker: TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(3, 2),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
                col: 2,
            }),
            exit_code: Some(0),
        });
        assert!(shell_integration.set_completed_block_folded(0, true));

        let visible_row_anchors = [
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 0,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
            },
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 1,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
            },
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 2,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 32,
                },
            },
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 3,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
            },
        ];
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };

        assert_eq!(
            select_command_block_gutter_hit_for_position(
                &mut shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                PhysicalPosition::new(2.0, 25.0),
                metrics,
                GridSize::new(5, 8),
            ),
            Some(1)
        );
        assert_eq!(shell_integration.selected_block_id(), Some(1));
    }

    #[test]
    fn paste_payload_leaves_plain_paste_unwrapped() {
        assert_eq!(paste_payload("echo ok\n", false), b"echo ok\n");
    }

    #[test]
    fn palette_panel_centers_with_bounded_visible_items() {
        assert_eq!(
            palette_panel(GridSize::new(24, 80), 20),
            Some(PalettePanel {
                start: CellPoint::new(2, 2),
                cols: 76,
                rows: 9,
                item_rows: 8,
            })
        );
    }

    #[test]
    fn profile_action_panel_uses_top_right_bounded_rows() {
        assert_eq!(
            profile_action_panel(GridSize::new(24, 120), 10),
            Some(PalettePanel {
                start: CellPoint::new(1, 47),
                cols: 72,
                rows: 7,
                item_rows: 5,
            })
        );
        assert_eq!(profile_action_panel(GridSize::new(1, 80), 1), None);
    }

    #[test]
    fn profile_action_overlay_renders_trusted_rows_without_terminal_feedback() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let metrics = CellMetrics::default();
        let panel = profile_action_panel(GridSize::new(24, 80), snapshot.display_rows.len())
            .expect("profile action panel");
        let mut frame = FramePlan {
            glyphs: vec![
                GlyphBatchItem {
                    origin: cell_origin(panel.start, metrics),
                    text: "covered".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(20, 0), metrics),
                    text: "visible".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
            ],
            cursor: Some(RectBatchItem {
                origin: cell_origin(panel.start, metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }),
            selection: vec![RectBatchItem {
                origin: cell_origin(panel.start, metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            ..FramePlan::default()
        };

        apply_profile_action_overlay(&mut frame, &snapshot, None, metrics, GridSize::new(24, 80));

        assert!(frame.cursor.is_none());
        assert!(frame.selection.is_empty());
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "visible"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "covered"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "Profile Actions"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[pick] Choose SSH profile")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[ready] Launch Production")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[profile] Production")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[credentials] Vaulted")));
        for hidden in ["prod.example.com", "vaulted.example.com", "vault-prod"] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked trusted profile secret {hidden:?}"
            );
        }
    }

    #[test]
    fn profile_action_overlay_highlights_hovered_visible_row() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel = profile_action_panel(grid_size, snapshot.display_rows.len())
            .expect("profile action panel");
        let row_index = 1;
        let hover = NativeProfileActionOverlayHit {
            key: snapshot.display_rows[row_index].key,
            row_index,
            target: NativeProfileActionOverlayTarget::Confirm,
        };
        let mut frame = FramePlan::default();

        apply_profile_action_overlay(&mut frame, &snapshot, Some(hover), metrics, grid_size);

        let hover_origin = cell_origin(
            CellPoint::new(panel.start.row + row_index as u16 + 1, panel.start.col),
            metrics,
        );
        assert!(frame.overlay_backgrounds.iter().any(|rect| {
            rect.origin == hover_origin
                && rect.size.width == f32::from(panel.cols) * metrics.cell.width
                && rect.size.height == metrics.cell.height
                && rect.color
                    == profile_action_hover_color(NativeProfileActionOverlayTarget::Confirm)
        }));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[ready] Launch Production")));
    }

    #[test]
    fn profile_action_overlay_does_not_highlight_hidden_rows() {
        let app = profile_bridge_app();
        let mut snapshot =
            native_profile_action_snapshot(&app, &profile_bridge_store()).expect("snapshot");
        let template = snapshot.display_rows[0].clone();
        snapshot.display_rows = vec![template; 10];
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 120);
        let panel =
            profile_action_panel(grid_size, snapshot.display_rows.len()).expect("profile panel");
        let hover = NativeProfileActionOverlayHit {
            key: snapshot.display_rows[panel.item_rows].key,
            row_index: panel.item_rows,
            target: NativeProfileActionOverlayTarget::Row,
        };
        let mut frame = FramePlan::default();

        apply_profile_action_overlay(&mut frame, &snapshot, Some(hover), metrics, grid_size);

        assert!(!frame
            .backgrounds
            .iter()
            .any(|rect| rect.color
                == profile_action_hover_color(NativeProfileActionOverlayTarget::Row)));
        assert!(frame.glyphs.iter().any(|glyph| glyph.text.contains("more")));
    }

    #[test]
    fn profile_action_overlay_hit_test_maps_rows_and_buttons() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel =
            profile_action_panel(grid_size, snapshot.display_rows.len()).expect("profile panel");
        let width = panel.cols.saturating_sub(2);
        let confirm_start = width.saturating_sub(text_cell_width("[Launch] [New Tab] [Dismiss]"));
        let new_tab_start = confirm_start
            .saturating_add(text_cell_width("[Launch]"))
            .saturating_add(1);
        let dismiss_start = confirm_start
            .saturating_add(text_cell_width("[Launch]"))
            .saturating_add(1)
            .saturating_add(text_cell_width("[New Tab]"))
            .saturating_add(1);

        let row_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 2, panel.start.col + 1),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(row_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(row_hit.row_index, 1);
        assert_eq!(row_hit.target, NativeProfileActionOverlayTarget::Row);

        let confirm_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 2, panel.start.col + 1 + confirm_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(confirm_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(
            confirm_hit.target,
            NativeProfileActionOverlayTarget::Confirm
        );

        let new_tab_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 2, panel.start.col + 1 + new_tab_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(new_tab_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(
            new_tab_hit.target,
            NativeProfileActionOverlayTarget::ConfirmNewTab
        );

        let dismiss_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 2, panel.start.col + 1 + dismiss_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(dismiss_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(
            dismiss_hit.target,
            NativeProfileActionOverlayTarget::Dismiss
        );
    }

    #[test]
    fn profile_action_overlay_hit_test_captures_picker_option_rows() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel =
            profile_action_panel(grid_size, profile_action_overlay_body_row_count(&snapshot))
                .expect("profile panel");
        let option_row_index = snapshot.display_rows.len();
        let width = panel.cols.saturating_sub(2);
        let select_start = width.saturating_sub(text_cell_width("[Select] [New Tab]"));
        let new_tab_start = select_start
            .saturating_add(text_cell_width("[Select]"))
            .saturating_add(1);

        let hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(
                    panel.start.row + option_row_index as u16 + 1,
                    panel.start.col + 1,
                ),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();

        assert_eq!(hit.key, PendingProfileActionKey::profile_picker(0));
        assert_eq!(hit.row_index, option_row_index);
        assert_eq!(hit.target, NativeProfileActionOverlayTarget::Row);

        let select_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(
                    panel.start.row + option_row_index as u16 + 1,
                    panel.start.col + 1 + select_start,
                ),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(select_hit.key, PendingProfileActionKey::profile_picker(0));
        assert_eq!(select_hit.row_index, option_row_index);
        assert_eq!(select_hit.target, NativeProfileActionOverlayTarget::Confirm);

        let new_tab_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(
                    panel.start.row + option_row_index as u16 + 1,
                    panel.start.col + 1 + new_tab_start,
                ),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(new_tab_hit.key, PendingProfileActionKey::profile_picker(0));
        assert_eq!(new_tab_hit.row_index, option_row_index);
        assert_eq!(
            new_tab_hit.target,
            NativeProfileActionOverlayTarget::ConfirmNewTab
        );

        let credential_option_row_index = option_row_index + 1;
        let credential_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(
                    panel.start.row + credential_option_row_index as u16 + 1,
                    panel.start.col + 1 + select_start,
                ),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(
            credential_hit.key,
            PendingProfileActionKey::profile_picker(0)
        );
        assert_eq!(credential_hit.row_index, credential_option_row_index);
        assert_eq!(credential_hit.target, NativeProfileActionOverlayTarget::Row);
    }

    #[test]
    fn profile_action_overlay_renders_start_success_without_terminal_feedback() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let snapshot = NativeProfileActionSnapshot {
            start_success: Some(native_profile_action_start_success_row(&plan)),
            ..NativeProfileActionSnapshot::default()
        };
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let mut frame = FramePlan::default();

        apply_profile_action_overlay(&mut frame, &snapshot, None, metrics, grid_size);

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[started] Active prod")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[Dismiss]")));
        assert!(!frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[Retry]")));
        for hidden in ["prod.example.com", "ssh -tt"] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked start success detail {hidden:?}"
            );
        }
    }

    #[test]
    fn profile_action_overlay_hit_test_maps_start_success_dismiss_only() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let snapshot = NativeProfileActionSnapshot {
            start_success: Some(native_profile_action_start_success_row(&plan)),
            ..NativeProfileActionSnapshot::default()
        };
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel =
            profile_action_panel(grid_size, profile_action_overlay_body_row_count(&snapshot))
                .expect("profile panel");
        let width = panel.cols.saturating_sub(2);
        let dismiss_start = width.saturating_sub(text_cell_width("[Dismiss]"));

        let row_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(row_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(row_hit.row_index, 0);
        assert_eq!(row_hit.target, NativeProfileActionOverlayTarget::Row);
        assert!(profile_action_overlay_start_success_for_hit(&snapshot, row_hit).is_some());

        let dismiss_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1 + dismiss_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(dismiss_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(dismiss_hit.row_index, 0);
        assert_eq!(
            dismiss_hit.target,
            NativeProfileActionOverlayTarget::Dismiss
        );
        assert!(profile_action_overlay_start_success_for_hit(&snapshot, dismiss_hit).is_some());
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, dismiss_hit),
            None
        );
    }

    #[test]
    fn profile_action_overlay_renders_start_failure_retry_without_terminal_feedback() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let snapshot = NativeProfileActionSnapshot {
            start_failure: Some(native_profile_action_start_failure_row(&plan)),
            ..NativeProfileActionSnapshot::default()
        };
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let mut frame = FramePlan::default();

        apply_profile_action_overlay(&mut frame, &snapshot, None, metrics, grid_size);

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[start failed] Retry prod")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[Retry] [Dismiss]")));
        for hidden in ["prod.example.com", "ssh -tt"] {
            assert!(
                !frame.glyphs.iter().any(|glyph| glyph.text.contains(hidden)),
                "leaked start failure detail {hidden:?}"
            );
        }
    }

    #[test]
    fn profile_action_overlay_hit_test_maps_start_failure_retry_and_dismiss() {
        let handoff = test_profile_action_handoff(
            PendingProfileActionKey::profile_launch(0),
            NativeResolvedProfileActionKind::ProfileLaunch,
            "prod",
            "open production",
            "prod.example.com",
        );
        let plan = native_profile_action_start_plan(
            handoff,
            NativeProfileActionStartMode::ReplaceCurrentSession,
        );
        let snapshot = NativeProfileActionSnapshot {
            start_failure: Some(native_profile_action_start_failure_row(&plan)),
            ..NativeProfileActionSnapshot::default()
        };
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel =
            profile_action_panel(grid_size, profile_action_overlay_body_row_count(&snapshot))
                .expect("profile panel");
        let width = panel.cols.saturating_sub(2);
        let retry_start = width.saturating_sub(text_cell_width("[Retry] [Dismiss]"));
        let dismiss_start = retry_start
            .saturating_add(text_cell_width("[Retry]"))
            .saturating_add(1);

        let row_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(row_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(row_hit.row_index, 0);
        assert_eq!(row_hit.target, NativeProfileActionOverlayTarget::Row);

        let retry_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1 + retry_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(retry_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(retry_hit.row_index, 0);
        assert_eq!(retry_hit.target, NativeProfileActionOverlayTarget::Confirm);
        assert!(profile_action_overlay_start_failure_for_hit(&snapshot, retry_hit).is_some());
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, retry_hit),
            None
        );

        let dismiss_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 1, panel.start.col + 1 + dismiss_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        assert_eq!(dismiss_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(dismiss_hit.row_index, 0);
        assert_eq!(
            dismiss_hit.target,
            NativeProfileActionOverlayTarget::Dismiss
        );
        assert!(profile_action_overlay_start_failure_for_hit(&snapshot, dismiss_hit).is_some());
    }

    #[test]
    fn profile_action_overlay_confirmation_maps_launch_and_picker_select_only() {
        let app = profile_bridge_app();
        let store = profile_bridge_store();
        let snapshot = native_profile_action_snapshot(&app, &store).unwrap();
        let launch_confirm = NativeProfileActionOverlayHit {
            key: PendingProfileActionKey::profile_launch(0),
            row_index: 1,
            target: NativeProfileActionOverlayTarget::Confirm,
        };
        let picker_option_select = NativeProfileActionOverlayHit {
            key: PendingProfileActionKey::profile_picker(0),
            row_index: snapshot.display_rows.len(),
            target: NativeProfileActionOverlayTarget::Confirm,
        };
        let launch_new_tab = NativeProfileActionOverlayHit {
            target: NativeProfileActionOverlayTarget::ConfirmNewTab,
            ..launch_confirm
        };
        let picker_option_new_tab = NativeProfileActionOverlayHit {
            target: NativeProfileActionOverlayTarget::ConfirmNewTab,
            ..picker_option_select
        };

        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, launch_confirm),
            Some(PendingProfileActionConfirmation::profile_launch(
                PendingProfileActionKey::profile_launch(0)
            ))
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, picker_option_select),
            Some(PendingProfileActionConfirmation::profile_picker(
                PendingProfileActionKey::profile_picker(0),
                "prod"
            ))
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, launch_new_tab),
            Some(PendingProfileActionConfirmation::profile_launch(
                PendingProfileActionKey::profile_launch(0)
            ))
        );
        assert_eq!(
            native_profile_action_start_mode_for_overlay_target(launch_confirm.target),
            Some(NativeProfileActionStartMode::ReplaceCurrentSession)
        );
        assert_eq!(
            native_profile_action_start_mode_for_overlay_target(launch_new_tab.target),
            Some(NativeProfileActionStartMode::NewTab)
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&snapshot, picker_option_new_tab),
            Some(PendingProfileActionConfirmation::profile_picker(
                PendingProfileActionKey::profile_picker(0),
                "prod"
            ))
        );
        assert_eq!(
            native_profile_action_start_mode_for_overlay_target(picker_option_select.target),
            Some(NativeProfileActionStartMode::ReplaceCurrentSession)
        );
        assert_eq!(
            native_profile_action_start_mode_for_overlay_target(picker_option_new_tab.target),
            Some(NativeProfileActionStartMode::NewTab)
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(
                &snapshot,
                NativeProfileActionOverlayHit {
                    target: NativeProfileActionOverlayTarget::Row,
                    ..launch_confirm
                }
            ),
            None
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(
                &snapshot,
                NativeProfileActionOverlayHit {
                    key: PendingProfileActionKey::profile_picker(0),
                    row_index: 0,
                    target: NativeProfileActionOverlayTarget::Confirm,
                }
            ),
            None
        );
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(
                &snapshot,
                NativeProfileActionOverlayHit {
                    key: PendingProfileActionKey::profile_picker(0),
                    row_index: snapshot.display_rows.len() + 1,
                    target: NativeProfileActionOverlayTarget::Confirm,
                }
            ),
            None
        );

        let blocked_snapshot =
            native_profile_action_snapshot(&app, &ProfileStoreV1::new()).unwrap();
        assert_eq!(
            profile_action_overlay_confirmation_for_hit(&blocked_snapshot, launch_confirm),
            None
        );
    }

    #[test]
    fn profile_action_overlay_hit_test_ignores_title_and_hidden_summary_rows() {
        let app = profile_bridge_app();
        let mut snapshot =
            native_profile_action_snapshot(&app, &profile_bridge_store()).expect("snapshot");
        let template = snapshot.display_rows[0].clone();
        snapshot.display_rows = vec![template; 10];
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 120);
        let panel =
            profile_action_panel(grid_size, snapshot.display_rows.len()).expect("profile panel");

        assert_eq!(
            profile_action_overlay_hit_for_position(
                &snapshot,
                physical_position_for_cell(panel.start, metrics),
                metrics,
                grid_size,
            ),
            None
        );
        assert_eq!(
            profile_action_overlay_hit_for_position(
                &snapshot,
                physical_position_for_cell(
                    CellPoint::new(panel.start.row + panel.rows - 1, panel.start.col + 1),
                    metrics,
                ),
                metrics,
                grid_size,
            ),
            None
        );
    }

    #[test]
    fn profile_action_overlay_hit_test_uses_dismiss_only_for_blocked_rows() {
        let app = profile_bridge_app();
        let snapshot = native_profile_action_snapshot(&app, &ProfileStoreV1::new()).unwrap();
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(24, 80);
        let panel =
            profile_action_panel(grid_size, snapshot.display_rows.len()).expect("profile panel");
        let width = panel.cols.saturating_sub(2);
        let dismiss_start = width.saturating_sub(text_cell_width("[Dismiss]"));

        let dismiss_hit = profile_action_overlay_hit_for_position(
            &snapshot,
            physical_position_for_cell(
                CellPoint::new(panel.start.row + 2, panel.start.col + 1 + dismiss_start),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();

        assert_eq!(dismiss_hit.key, PendingProfileActionKey::profile_launch(0));
        assert_eq!(
            dismiss_hit.target,
            NativeProfileActionOverlayTarget::Dismiss
        );
        assert_eq!(
            profile_action_overlay_target_for_text_col(&snapshot.display_rows[1], 0, width),
            NativeProfileActionOverlayTarget::Row
        );
    }

    #[test]
    fn empty_session_welcome_overlay_hides_stale_terminal_frame_and_renders_actions() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(12, 80);
        let mut frame = FramePlan {
            glyphs: vec![GlyphBatchItem {
                origin: cell_origin(CellPoint::new(5, 4), metrics),
                text: "[process exited] stale terminal text".to_owned(),
                color: Rgba::WHITE,
                style_flags: CellFlags::default(),
            }],
            cursor: Some(RectBatchItem {
                origin: cell_origin(CellPoint::new(5, 4), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }),
            selection: vec![RectBatchItem {
                origin: cell_origin(CellPoint::new(5, 4), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            search_highlights: vec![RectBatchItem {
                origin: cell_origin(CellPoint::new(5, 4), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            hyperlink_underlines: vec![RectBatchItem {
                origin: cell_origin(CellPoint::new(5, 4), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            ..FramePlan::default()
        };

        apply_empty_session_welcome_overlay(&mut frame, true, None, None, metrics, grid_size);

        assert!(frame.cursor.is_none());
        assert!(frame.selection.is_empty());
        assert!(frame.search_highlights.is_empty());
        assert!(frame.hyperlink_underlines.is_empty());
        assert!(!frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("stale terminal text")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("No active shell")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[New Session]")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[Command Palette]")));
    }

    #[test]
    fn empty_session_welcome_hit_test_maps_visible_buttons_only() {
        let metrics = CellMetrics::default();
        let grid_size = GridSize::new(12, 80);
        let panel = empty_session_welcome_panel(grid_size).unwrap();
        let button_row = panel.start.row + empty_session_welcome_button_row(panel);
        let spans = empty_session_welcome_button_spans(panel.cols.saturating_sub(2));
        let local_span = spans
            .iter()
            .find(|span| span.target == NativeEmptySessionWelcomeTarget::NewLocalShell)
            .unwrap();
        let palette_span = spans
            .iter()
            .find(|span| span.target == NativeEmptySessionWelcomeTarget::CommandPalette)
            .unwrap();

        let local_hit = empty_session_welcome_hit_for_position(
            physical_position_for_cell(
                CellPoint::new(button_row, panel.start.col + 1 + local_span.start_col),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        let palette_hit = empty_session_welcome_hit_for_position(
            physical_position_for_cell(
                CellPoint::new(button_row, panel.start.col + 1 + palette_span.start_col),
                metrics,
            ),
            metrics,
            grid_size,
        )
        .unwrap();
        let title_hit = empty_session_welcome_hit_for_position(
            physical_position_for_cell(
                CellPoint::new(panel.start.row, panel.start.col + 1),
                metrics,
            ),
            metrics,
            grid_size,
        );
        let outside_hit = empty_session_welcome_hit_for_position(
            physical_position_for_cell(CellPoint::new(button_row, 0), metrics),
            metrics,
            grid_size,
        );

        assert_eq!(
            local_hit.target,
            NativeEmptySessionWelcomeTarget::NewLocalShell
        );
        assert_eq!(
            palette_hit.target,
            NativeEmptySessionWelcomeTarget::CommandPalette
        );
        assert_eq!(title_hit, None);
        assert_eq!(outside_hit, None);
    }

    #[test]
    fn empty_session_welcome_overlay_shows_sanitized_start_failure_notice() {
        let metrics = CellMetrics::default();
        let mut frame = FramePlan::default();

        apply_empty_session_welcome_overlay(
            &mut frame,
            true,
            Some("New session failed:\npty unavailable"),
            None,
            metrics,
            GridSize::new(12, 80),
        );

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("New session failed: pty unavailable")));
    }

    #[test]
    fn native_window_commands_include_new_session() {
        let commands = native_window_command_registrations();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id, WITTY_NEW_LOCAL_SHELL_COMMAND_ID);
        assert_eq!(commands[0].title, "New Session");
        assert_eq!(commands[0].source_plugin, "builtin");
    }

    #[test]
    fn palette_overlay_hides_terminal_cursor_and_panel_glyphs() {
        let metrics = CellMetrics::default();
        let mut palette = CommandPalette::default();
        palette.open(&[CommandRegistration {
            id: "witty.about".to_owned(),
            title: "About Witty".to_owned(),
            source_plugin: "builtin".to_owned(),
        }]);
        let mut frame = FramePlan {
            glyphs: vec![
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(2, 2), metrics),
                    text: "covered".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(20, 0), metrics),
                    text: "visible".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
            ],
            cursor: Some(RectBatchItem {
                origin: cell_origin(CellPoint::new(2, 2), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }),
            ..FramePlan::default()
        };

        let commands = vec![CommandRegistration {
            id: "witty.about".to_owned(),
            title: "About Witty".to_owned(),
            source_plugin: "builtin".to_owned(),
        }];

        apply_command_palette_overlay(
            &mut frame,
            &palette,
            None,
            &commands,
            metrics,
            GridSize::new(24, 80),
        );

        assert!(frame.cursor.is_none());
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "visible"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "covered"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Command Palette")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("About Witty")));
        assert!(frame.glyphs.iter().any(|glyph| glyph.text.contains("F1")));
    }

    #[test]
    fn command_palette_overlay_renders_preedit_inline_and_positions_ime_cursor() {
        let metrics = CellMetrics::default();
        let mut palette = CommandPalette::default();
        let commands = vec![CommandRegistration {
            id: "witty.search.open".to_owned(),
            title: "Search: Open".to_owned(),
            source_plugin: "builtin".to_owned(),
        }];
        palette.open(&commands);
        palette.input_text("se");
        let mut composition = ImeComposition::default();
        composition.set_preedit("搜", Some(("搜".len(), "搜".len())));
        let mut frame = FramePlan::default();

        apply_command_palette_overlay(
            &mut frame,
            &palette,
            Some(&composition),
            &commands,
            metrics,
            GridSize::new(24, 80),
        );

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Command Palette  se搜")));
        assert_eq!(
            command_palette_ime_cursor_cell(&palette, &composition, GridSize::new(24, 80)),
            Some(CellPoint::new(2, 2 + 1 + 17 + 2 + 2))
        );
        assert_eq!(palette.query(), "se");
    }

    #[test]
    fn terminal_ime_cursor_tracks_preedit_caret_and_clamps_to_grid() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("a你b", Some(("a你".len(), "a你".len())));

        assert_eq!(
            terminal_ime_cursor_cell(CellPoint::new(1, 4), &composition, GridSize::new(3, 12)),
            CellPoint::new(1, 7)
        );

        assert_eq!(
            terminal_ime_cursor_cell(CellPoint::new(9, 10), &composition, GridSize::new(3, 12)),
            CellPoint::new(2, 11)
        );
    }

    #[test]
    fn search_bar_overlay_hides_covered_terminal_content_and_reports_count() {
        let metrics = CellMetrics::default();
        let mut terminal = BasicTerminal::new(GridSize::new(3, 36));
        terminal.feed(b"find");
        let mut search = TerminalSearch::default();
        search.open(&terminal.search_text_rows(), Some("find"));

        let mut frame = FramePlan {
            glyphs: vec![
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(0, 0), metrics),
                    text: "visible".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
                GlyphBatchItem {
                    origin: cell_origin(CellPoint::new(2, 0), metrics),
                    text: "covered".to_owned(),
                    color: Rgba::WHITE,
                    style_flags: CellFlags::default(),
                },
            ],
            cursor: Some(RectBatchItem {
                origin: cell_origin(CellPoint::new(2, 4), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }),
            search_highlights: vec![RectBatchItem {
                origin: cell_origin(CellPoint::new(2, 0), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            selection: vec![RectBatchItem {
                origin: cell_origin(CellPoint::new(2, 1), metrics),
                size: metrics.cell,
                color: Rgba::WHITE,
            }],
            ..FramePlan::default()
        };

        apply_search_bar_overlay(&mut frame, &search, None, metrics, GridSize::new(3, 36));

        assert!(frame.cursor.is_none());
        assert!(frame.search_highlights.is_empty());
        assert!(frame.selection.is_empty());
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "visible"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "covered"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Find: find")));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("[aa lit part raw]")));
        assert!(frame.glyphs.iter().any(|glyph| glyph.text.contains("1/1")));
    }

    #[test]
    fn search_bar_overlay_renders_preedit_inline_and_positions_ime_cursor() {
        let metrics = CellMetrics::default();
        let mut terminal = BasicTerminal::new(GridSize::new(3, 40));
        terminal.feed("find 中".as_bytes());
        let mut search = TerminalSearch::default();
        search.open(&terminal.search_text_rows(), Some("find "));
        let mut composition = ImeComposition::default();
        composition.set_preedit("中", Some(("中".len(), "中".len())));
        let mut frame = FramePlan::default();

        apply_search_bar_overlay(
            &mut frame,
            &search,
            Some(&composition),
            metrics,
            GridSize::new(3, 40),
        );

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Find: find 中")));
        assert_eq!(
            search_ime_cursor_cell(&search, &composition, GridSize::new(3, 40)),
            CellPoint::new(2, 1 + 6 + 5 + 2)
        );
        assert_eq!(search.query(), "find ");
    }

    #[test]
    fn search_count_label_reports_zero_and_invalid_regex_states() {
        let rows = vec![witty_core::SearchTextRow {
            id: witty_core::SearchRowId::screen(0),
            visible_row: Some(0),
            text: "alpha beta".to_owned(),
            columns: Vec::new(),
        }];
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        assert_eq!(search_count_label(&search), "0/0");

        search.input_text(&rows, "missing");
        assert_eq!(search_count_label(&search), "No results");

        search.set_query(&rows, "[");
        search.toggle_regex(&rows);
        assert!(search_count_label(&search).contains("invalid regex"));
        assert!(search_options_label(&search).contains(".*"));
        assert!(search_options_label(&search).contains("raw"));
    }

    #[test]
    fn native_search_smoke_validates_highlights_and_find_bar() {
        let smoke = native_search_smoke().unwrap();

        assert_eq!(smoke.query, "alpha");
        assert_eq!(smoke.match_count, 2);
        assert_eq!(smoke.active_index, Some(0));
        assert_eq!(smoke.visible_highlights, 1);
        assert!(smoke.active_visible);
        assert_eq!(smoke.status, "1/2");
    }

    #[test]
    fn palette_item_text_adds_known_shortcut_hints() {
        let commands = vec![
            CommandRegistration {
                id: "witty.about".to_owned(),
                title: "About".to_owned(),
                source_plugin: "builtin".to_owned(),
            },
            CommandRegistration {
                id: "fixture.echo".to_owned(),
                title: "Fixture Echo".to_owned(),
                source_plugin: "fixture".to_owned(),
            },
            CommandRegistration {
                id: "other.echo".to_owned(),
                title: "Other Echo".to_owned(),
                source_plugin: "other".to_owned(),
            },
        ];

        assert!(palette_item_text(&commands[0], true, &commands, 80).contains("F1"));
        assert!(palette_item_text(&commands[1], false, &commands, 80).contains("F2"));
        assert!(!palette_item_text(&commands[2], false, &commands, 80).contains("F2"));
    }

    #[test]
    fn diagnostics_overlay_adds_frame_stats_text() {
        let metrics = CellMetrics::default();
        let stats = FrameStats {
            background_runs: 3,
            glyph_runs: 2,
            glyph_chars: 10,
            glyph_prepare_batches: 2,
            max_glyph_run_chars: 6,
            selection_rects: 1,
            rect_vertices: 12,
            rect_vertex_capacity: 16,
            full_damage: false,
            damage_regions: 2,
            reused_rows: 7,
            rebuilt_rows: 1,
            ..FrameStats::default()
        };
        let mut frame = FramePlan::default();

        apply_frame_diagnostics_overlay(&mut frame, stats, metrics, GridSize::new(24, 80));

        assert!(!frame.overlay_backgrounds.is_empty());
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "damage rows regions=2"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "rows reused=7 rebuilt=1"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "runs bg=3 glyph=2 chars=10 batches=2 max=6"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "rectv=12 cap=16 sel=1 deco=0"));
    }

    #[test]
    fn palette_title_truncates_to_available_cells() {
        assert_eq!(palette_title("abcdef", 8), "Comma...");
    }
}
