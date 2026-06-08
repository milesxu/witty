//! Minimal application shell for the M0 prototype.

mod command_palette;
mod ime;
mod plugin_host;
mod search;
mod shell_integration;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use anyhow::{bail, Result};
use witty_core::{GridSize, RenderSnapshot};
use witty_plugin_api::{
    CommandInvocation, CommandInvocationContext, CommandRegistration, PluginAction, PluginEvent,
};
#[cfg(not(target_arch = "wasm32"))]
use witty_plugin_wasm::WasmPluginRuntime;
use witty_render_wgpu::{CellMetrics, FramePlan, RetainedFramePlanner};
use witty_transport::{ProfileStoreSummary, ProfileStoreV1, TerminalTransport, TransportEvent};

pub use command_palette::{CommandPalette, CommandPaletteItem};
pub use ime::{apply_ime_preedit_overlay, ime_preedit_overlay, ImeComposition, ImePreeditOverlay};
pub use plugin_host::{
    plugin_profile_store_summary_from_redacted, resolve_profile_launch_request,
    resolve_profile_picker_selection, review_profile_launch_requests,
    review_profile_picker_requests, BuiltInPlugin, PendingProfileLaunchRequest,
    PendingProfileLaunchRequestReview, PendingProfileLaunchRequestStatus,
    PendingProfilePickerRequest, PendingProfilePickerRequestReview, PluginHost,
    PluginRuntimeFailure, PluginRuntimeFailureKind, ResolvedProfileLaunchRequest,
    ResolvedProfilePickerSelection,
};
#[cfg(not(target_arch = "wasm32"))]
pub use plugin_host::{
    resolve_profile_launch_pty_config, resolve_profile_picker_pty_config,
    ResolvedProfileLaunchPtyConfig, ResolvedProfilePickerPtyConfig,
};
pub use search::{
    search_command_registrations, TerminalSearch, SEARCH_CLOSE_COMMAND_ID, SEARCH_NEXT_COMMAND_ID,
    SEARCH_OPEN_COMMAND_ID, SEARCH_PREVIOUS_COMMAND_ID,
};
pub use shell_integration::{
    apply_command_block_action_menu_overlay, apply_command_block_command,
    apply_command_block_folded_frame_remap_with_anchors,
    apply_command_block_folded_row_mask_with_anchors,
    apply_command_block_gutter_hover_overlay_with_anchors,
    apply_command_block_gutter_overlay_with_anchors, apply_command_block_selection_overlay,
    apply_command_block_selection_overlay_with_anchors,
    apply_command_block_status_label_overlay_with_anchors, command_block_command_registrations,
    command_block_copy_target,
    command_block_folded_terminal_row_for_compact_visual_row_with_anchors,
    command_block_folded_visual_pixel_to_terminal_pixel_with_anchors,
    command_block_gutter_hit_test_with_anchors, command_block_status_label,
    selected_command_block_copy_text, CommandBlockActionMenu, CommandBlockActionMenuItem,
    CommandBlockActionMenuVisibleItem, CommandBlockCopyTarget, PendingCommandBlock,
    ShellIntegrationState, TerminalCommandBlock, TerminalCommandBlockAnchorRowSpan,
    TerminalCommandBlockFoldedCompactVisualRow, TerminalCommandBlockFoldedFrameRemapStats,
    TerminalCommandBlockFoldedHiddenRowSpan, TerminalCommandBlockGutterHit,
    TerminalCommandBlockRowSpan, TerminalCommandBlockTextRange, TerminalCommandBlockTextRanges,
    COMMAND_BLOCK_ACTION_MENU_BACKGROUND, COMMAND_BLOCK_ACTION_MENU_COMMAND_ID,
    COMMAND_BLOCK_ACTION_MENU_MUTED_TEXT, COMMAND_BLOCK_ACTION_MENU_SELECTED,
    COMMAND_BLOCK_ACTION_MENU_TEXT, COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
    COMMAND_BLOCK_COPY_COMMAND_ID, COMMAND_BLOCK_COPY_OUTPUT_ID,
    COMMAND_BLOCK_FOLDED_ROW_MASK_BACKGROUND, COMMAND_BLOCK_GUTTER_FAILURE,
    COMMAND_BLOCK_GUTTER_SUCCESS, COMMAND_BLOCK_GUTTER_UNKNOWN,
    COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID, COMMAND_BLOCK_SELECT_NEXT_COMMAND_ID,
    COMMAND_BLOCK_SELECT_PREVIOUS_COMMAND_ID, COMMAND_BLOCK_STATUS_LABEL_BACKGROUND,
    COMMAND_BLOCK_STATUS_LABEL_FAILURE_TEXT, COMMAND_BLOCK_STATUS_LABEL_SUCCESS_TEXT,
    COMMAND_BLOCK_STATUS_LABEL_UNKNOWN_TEXT, COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
    HOVERED_COMMAND_BLOCK_BACKGROUND, HOVERED_COMMAND_BLOCK_GUTTER,
    SELECTED_COMMAND_BLOCK_BACKGROUND, SELECTED_COMMAND_BLOCK_GUTTER,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingProfileActionKind {
    ProfilePicker,
    ProfileLaunch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingProfileActionKey {
    pub kind: PendingProfileActionKind,
    pub request_index: usize,
}

impl PendingProfileActionKey {
    pub const fn profile_picker(request_index: usize) -> Self {
        Self {
            kind: PendingProfileActionKind::ProfilePicker,
            request_index,
        }
    }

    pub const fn profile_launch(request_index: usize) -> Self {
        Self {
            kind: PendingProfileActionKind::ProfileLaunch,
            request_index,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingProfileActionReview {
    ProfilePicker {
        key: PendingProfileActionKey,
        request: PendingProfilePickerRequestReview,
    },
    ProfileLaunch {
        key: PendingProfileActionKey,
        request: PendingProfileLaunchRequestReview,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DismissedPendingProfileAction {
    ProfilePicker {
        key: PendingProfileActionKey,
        request: PendingProfilePickerRequest,
    },
    ProfileLaunch {
        key: PendingProfileActionKey,
        request: PendingProfileLaunchRequest,
    },
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingProfileActionConfirmation {
    ProfilePicker {
        key: PendingProfileActionKey,
        profile_id: String,
    },
    ProfileLaunch {
        key: PendingProfileActionKey,
    },
}

#[cfg(not(target_arch = "wasm32"))]
impl PendingProfileActionConfirmation {
    pub fn profile_picker(key: PendingProfileActionKey, profile_id: impl Into<String>) -> Self {
        Self::ProfilePicker {
            key,
            profile_id: profile_id.into(),
        }
    }

    pub const fn profile_launch(key: PendingProfileActionKey) -> Self {
        Self::ProfileLaunch { key }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedPendingProfileActionPtyConfig {
    ProfilePicker {
        key: PendingProfileActionKey,
        resolved: ResolvedProfilePickerPtyConfig,
    },
    ProfileLaunch {
        key: PendingProfileActionKey,
        resolved: ResolvedProfileLaunchPtyConfig,
    },
}

pub struct TerminalApp<T> {
    transport: T,
    planner: RetainedFramePlanner,
    snapshot: RenderSnapshot,
    commands: Vec<CommandRegistration>,
    plugin_host: PluginHost,
}

impl<T> TerminalApp<T>
where
    T: TerminalTransport,
{
    pub fn new(transport: T, size: GridSize) -> Self {
        Self {
            transport,
            planner: RetainedFramePlanner::new(CellMetrics::default()),
            snapshot: RenderSnapshot::empty(size),
            commands: Vec::new(),
            plugin_host: PluginHost::default(),
        }
    }

    pub fn register_command(&mut self, command: CommandRegistration) -> Result<()> {
        self.push_command(command)
    }

    pub fn commands(&self) -> &[CommandRegistration] {
        &self.commands
    }

    pub fn disabled_plugin_ids(&self) -> &[String] {
        self.plugin_host.disabled_plugin_ids()
    }

    pub fn plugin_runtime_failures(&self) -> &[PluginRuntimeFailure] {
        self.plugin_host.runtime_failures()
    }

    pub fn review_pending_profile_actions(
        &self,
        store: &ProfileStoreV1,
    ) -> Result<Vec<PendingProfileActionReview>> {
        let picker_reviews = self.review_pending_profile_picker_requests(store)?;
        let launch_reviews = self.review_pending_profile_launch_requests(store)?;
        let mut reviews = Vec::with_capacity(picker_reviews.len() + launch_reviews.len());

        reviews.extend(
            picker_reviews
                .into_iter()
                .enumerate()
                .map(
                    |(request_index, request)| PendingProfileActionReview::ProfilePicker {
                        key: PendingProfileActionKey::profile_picker(request_index),
                        request,
                    },
                ),
        );
        reviews.extend(
            launch_reviews
                .into_iter()
                .enumerate()
                .map(
                    |(request_index, request)| PendingProfileActionReview::ProfileLaunch {
                        key: PendingProfileActionKey::profile_launch(request_index),
                        request,
                    },
                ),
        );
        Ok(reviews)
    }

    pub fn dismiss_pending_profile_action(
        &mut self,
        key: PendingProfileActionKey,
    ) -> Result<DismissedPendingProfileAction> {
        match key.kind {
            PendingProfileActionKind::ProfilePicker => {
                let request = self.dismiss_pending_profile_picker_request(key.request_index)?;
                Ok(DismissedPendingProfileAction::ProfilePicker { key, request })
            }
            PendingProfileActionKind::ProfileLaunch => {
                let request = self.dismiss_pending_profile_launch_request(key.request_index)?;
                Ok(DismissedPendingProfileAction::ProfileLaunch { key, request })
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_resolved_pending_profile_action_pty_config(
        &mut self,
        store: &ProfileStoreV1,
        confirmation: PendingProfileActionConfirmation,
        size: GridSize,
    ) -> Result<ResolvedPendingProfileActionPtyConfig> {
        match confirmation {
            PendingProfileActionConfirmation::ProfilePicker { key, profile_id } => {
                if key.kind != PendingProfileActionKind::ProfilePicker {
                    bail!("pending profile action key is not a profile picker request");
                }
                let resolved = self.take_resolved_profile_picker_pty_config(
                    store,
                    key.request_index,
                    &profile_id,
                    size,
                )?;
                Ok(ResolvedPendingProfileActionPtyConfig::ProfilePicker { key, resolved })
            }
            PendingProfileActionConfirmation::ProfileLaunch { key } => {
                if key.kind != PendingProfileActionKind::ProfileLaunch {
                    bail!("pending profile action key is not a profile launch request");
                }
                let resolved =
                    self.take_resolved_profile_launch_pty_config(store, key.request_index, size)?;
                Ok(ResolvedPendingProfileActionPtyConfig::ProfileLaunch { key, resolved })
            }
        }
    }

    pub fn profile_picker_requests(&self) -> &[PendingProfilePickerRequest] {
        self.plugin_host.profile_picker_requests()
    }

    pub fn take_profile_picker_requests(&mut self) -> Vec<PendingProfilePickerRequest> {
        self.plugin_host.take_profile_picker_requests()
    }

    pub fn dismiss_pending_profile_picker_request(
        &mut self,
        request_index: usize,
    ) -> Result<PendingProfilePickerRequest> {
        self.plugin_host.take_profile_picker_request(request_index)
    }

    pub fn review_pending_profile_picker_requests(
        &self,
        store: &ProfileStoreV1,
    ) -> Result<Vec<PendingProfilePickerRequestReview>> {
        review_profile_picker_requests(store, self.profile_picker_requests())
    }

    pub fn resolve_pending_profile_picker_selection(
        &self,
        store: &ProfileStoreV1,
        request_index: usize,
        profile_id: &str,
    ) -> Result<ResolvedProfilePickerSelection> {
        let Some(request) = self.profile_picker_requests().get(request_index) else {
            bail!("profile picker request index {request_index} out of range");
        };
        resolve_profile_picker_selection(store, request, profile_id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_pending_profile_picker_pty_config(
        &self,
        store: &ProfileStoreV1,
        request_index: usize,
        profile_id: &str,
        size: GridSize,
    ) -> Result<ResolvedProfilePickerPtyConfig> {
        let Some(request) = self.profile_picker_requests().get(request_index) else {
            bail!("profile picker request index {request_index} out of range");
        };
        resolve_profile_picker_pty_config(store, request, profile_id, size)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_resolved_profile_picker_pty_config(
        &mut self,
        store: &ProfileStoreV1,
        request_index: usize,
        profile_id: &str,
        size: GridSize,
    ) -> Result<ResolvedProfilePickerPtyConfig> {
        let config =
            self.resolve_pending_profile_picker_pty_config(store, request_index, profile_id, size)?;
        self.plugin_host
            .take_profile_picker_request(request_index)?;
        Ok(config)
    }

    pub fn profile_launch_requests(&self) -> &[PendingProfileLaunchRequest] {
        self.plugin_host.profile_launch_requests()
    }

    pub fn take_profile_launch_requests(&mut self) -> Vec<PendingProfileLaunchRequest> {
        self.plugin_host.take_profile_launch_requests()
    }

    pub fn dismiss_pending_profile_launch_request(
        &mut self,
        request_index: usize,
    ) -> Result<PendingProfileLaunchRequest> {
        self.plugin_host.take_profile_launch_request(request_index)
    }

    pub fn review_pending_profile_launch_requests(
        &self,
        store: &ProfileStoreV1,
    ) -> Result<Vec<PendingProfileLaunchRequestReview>> {
        review_profile_launch_requests(store, self.profile_launch_requests())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_pending_profile_launch_pty_config(
        &self,
        store: &ProfileStoreV1,
        request_index: usize,
        size: GridSize,
    ) -> Result<ResolvedProfileLaunchPtyConfig> {
        let Some(request) = self.profile_launch_requests().get(request_index) else {
            bail!("profile launch request index {request_index} out of range");
        };
        resolve_profile_launch_pty_config(store, request, size)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_pending_profile_launch_pty_configs(
        &self,
        store: &ProfileStoreV1,
        size: GridSize,
    ) -> Result<Vec<ResolvedProfileLaunchPtyConfig>> {
        self.profile_launch_requests()
            .iter()
            .map(|request| resolve_profile_launch_pty_config(store, request, size))
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_resolved_profile_launch_pty_configs(
        &mut self,
        store: &ProfileStoreV1,
        size: GridSize,
    ) -> Result<Vec<ResolvedProfileLaunchPtyConfig>> {
        let configs = self.resolve_pending_profile_launch_pty_configs(store, size)?;
        self.take_profile_launch_requests();
        Ok(configs)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_resolved_profile_launch_pty_config(
        &mut self,
        store: &ProfileStoreV1,
        request_index: usize,
        size: GridSize,
    ) -> Result<ResolvedProfileLaunchPtyConfig> {
        let config = self.resolve_pending_profile_launch_pty_config(store, request_index, size)?;
        self.plugin_host
            .take_profile_launch_request(request_index)?;
        Ok(config)
    }

    pub fn install_builtin_plugin<P>(&mut self, plugin: P) -> Result<()>
    where
        P: BuiltInPlugin + 'static,
    {
        for command in plugin.commands() {
            self.ensure_command_available(&command.id)?;
        }

        let commands = self.plugin_host.install_builtin(plugin)?;
        for command in commands {
            self.push_command(command)?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn install_wasm_plugin_from_file(
        &mut self,
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let prepared = PluginHost::prepare_wasm_component_from_file(runtime, path)?;
        for command in prepared.commands() {
            self.ensure_command_available(&command.id)?;
        }

        let commands = self.plugin_host.install_wasm(prepared)?;
        for command in commands {
            self.push_command(command)?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn install_wasm_plugin_from_file_with_profile_store_summary(
        &mut self,
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
        summary: &ProfileStoreSummary,
    ) -> Result<()> {
        let prepared = PluginHost::prepare_wasm_component_from_file_with_profile_store_summary(
            runtime, path, summary,
        )?;
        for command in prepared.commands() {
            self.ensure_command_available(&command.id)?;
        }

        let commands = self.plugin_host.install_wasm(prepared)?;
        for command in commands {
            self.push_command(command)?;
        }
        Ok(())
    }

    pub fn dispatch_plugin_event(&mut self, event: PluginEvent) -> Result<Vec<PluginAction>> {
        let actions = self
            .plugin_host
            .dispatch_event_with_reserved_commands(&event, &self.commands)?;
        self.apply_plugin_actions(&actions)?;
        Ok(actions)
    }

    pub fn invoke_command(
        &mut self,
        command_id: impl Into<String>,
        args: serde_json::Value,
    ) -> Result<Vec<PluginAction>> {
        self.invoke_command_with_context(command_id, args, CommandInvocationContext::default())
    }

    pub fn invoke_command_with_context(
        &mut self,
        command_id: impl Into<String>,
        args: serde_json::Value,
        context: CommandInvocationContext,
    ) -> Result<Vec<PluginAction>> {
        let command_id = command_id.into();
        if !self.commands.iter().any(|command| command.id == command_id) {
            bail!("unknown command id {command_id}");
        }

        self.dispatch_plugin_event(PluginEvent::CommandInvoked(CommandInvocation {
            command_id,
            args,
            context,
        }))
    }

    pub fn write_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.transport.write(bytes)
    }

    pub fn poll_transport(&mut self) -> Result<Option<TransportEvent>> {
        self.transport.poll_event()
    }

    pub fn resize_transport(&mut self, size: GridSize) -> Result<()> {
        self.transport.resize(size)
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn replace_transport(&mut self, transport: T) -> T {
        std::mem::replace(&mut self.transport, transport)
    }

    pub fn set_cell_metrics(&mut self, metrics: CellMetrics) {
        self.planner = RetainedFramePlanner::new(metrics);
    }

    pub fn set_blink_visible(&mut self, visible: bool) {
        self.planner.set_blink_visible(visible);
    }

    pub fn set_snapshot(&mut self, snapshot: RenderSnapshot) {
        self.snapshot = snapshot;
    }

    pub fn title(&self) -> Option<&str> {
        self.snapshot.title.as_deref()
    }

    pub fn frame_plan(&mut self) -> FramePlan {
        self.planner.plan(&self.snapshot)
    }

    fn apply_plugin_actions(&mut self, actions: &[PluginAction]) -> Result<()> {
        for action in actions {
            match action {
                PluginAction::RegisterCommand(command) => self.push_command(command.clone())?,
                PluginAction::WriteTerminal { bytes } => self.transport.write(bytes)?,
                PluginAction::ShowMessage { .. }
                | PluginAction::RenderOverlay(_)
                | PluginAction::RequestProfilePicker(_)
                | PluginAction::RequestProfileLaunch(_) => {}
            }
        }
        Ok(())
    }

    fn push_command(&mut self, command: CommandRegistration) -> Result<()> {
        self.ensure_command_available(&command.id)?;
        self.commands.push(command);
        Ok(())
    }

    fn ensure_command_available(&self, command_id: &str) -> Result<()> {
        if self.commands.iter().any(|command| command.id == command_id) {
            bail!("duplicate command id {command_id}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use witty_plugin_api::{
        NetworkPermission, PluginCurrentDirectory, PluginManifest, PluginPermissions,
        PluginProfileLaunchRequest, PluginProfilePickerRequest, PluginRuntime,
        TerminalReadPermission, TerminalWritePermission, VaultPermission,
    };
    use witty_transport::{
        LocalPtyConfig, MockTransport, ProfileStoreSummary, ProfileStoreV1, ProfileSummary,
        SshProfile, SshProfileLaunchability,
    };

    #[test]
    fn app_builds_frame_plan() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.set_snapshot(RenderSnapshot::from_plain_lines(&["witty"]));

        let frame = app.frame_plan();
        assert_eq!(frame.glyphs.len(), 1);
        assert_eq!(frame.glyphs[0].text, "witty");
    }

    #[test]
    fn app_applies_renderer_blink_phase() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        let mut snapshot = RenderSnapshot::from_plain_lines(&["blink"]);
        for cell in &mut snapshot.rows[0].cells {
            cell.style.flags.blink = true;
        }
        app.set_snapshot(snapshot);

        assert_eq!(app.frame_plan().glyphs.len(), 1);

        app.set_blink_visible(false);

        assert!(app.frame_plan().glyphs.is_empty());

        app.set_blink_visible(true);

        assert_eq!(app.frame_plan().glyphs.len(), 1);
    }

    #[test]
    fn app_exposes_snapshot_title() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        let mut snapshot = RenderSnapshot::from_plain_lines(&["witty"]);
        snapshot.title = Some("build logs".to_owned());

        app.set_snapshot(snapshot);

        assert_eq!(app.title(), Some("build logs"));
    }

    #[test]
    fn app_rejects_duplicate_command_ids() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.register_command(command("app.echo", "app")).unwrap();

        assert!(app.register_command(command("app.echo", "app")).is_err());
    }

    #[test]
    fn builtin_plugin_commands_join_app_registry() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.install_builtin_plugin(EchoPlugin::new("echo", TerminalWritePermission::Deny))
            .unwrap();

        assert_eq!(app.commands()[0].id, "echo.run");
    }

    #[test]
    fn invoke_command_dispatches_plugin_actions_and_writes_transport() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.install_builtin_plugin(EchoPlugin::new(
            "echo",
            TerminalWritePermission::AllowSession,
        ))
        .unwrap();
        let actions = app
            .invoke_command("echo.run", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"echo from plugin\n".to_vec()
            }]
        );
        assert_eq!(app.transport().written(), b"echo from plugin\n");
    }

    #[test]
    fn replace_transport_preserves_app_state_and_routes_future_writes() {
        let size = GridSize::new(24, 80);
        let mut app = TerminalApp::new(MockTransport::new(size), size);
        app.register_command(command("app.keep", "app")).unwrap();
        app.write_input(b"old-session").unwrap();

        let old_transport = app.replace_transport(MockTransport::new(size));

        assert_eq!(old_transport.written(), b"old-session");
        assert_eq!(app.commands()[0].id, "app.keep");
        assert!(app.transport().written().is_empty());

        app.write_input(b"new-session").unwrap();

        assert_eq!(app.transport().written(), b"new-session");
    }

    #[test]
    fn invoke_command_with_context_dispatches_current_directory() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.install_builtin_plugin(ContextEchoPlugin).unwrap();
        let actions = app
            .invoke_command_with_context(
                "context.run",
                serde_json::Value::Null,
                CommandInvocationContext {
                    current_directory: Some(PluginCurrentDirectory {
                        uri: "file://localhost/home/mingxu/context".to_owned(),
                        host: Some("localhost".to_owned()),
                        path: "/home/mingxu/context".to_owned(),
                    }),
                    ..CommandInvocationContext::default()
                },
            )
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"pwd /home/mingxu/context\n".to_vec()
            }]
        );
        assert_eq!(app.transport().written(), b"pwd /home/mingxu/context\n");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wasm_plugin_commands_join_app_registry_and_write_transport() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();

        app.install_wasm_plugin_from_file(&runtime, component_path)
            .unwrap();

        assert_eq!(
            app.commands(),
            &[
                CommandRegistration {
                    id: "fixture.echo".to_owned(),
                    title: "Fixture Echo".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                CommandRegistration {
                    id: "fixture.host-info".to_owned(),
                    title: "Fixture Host Info".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                CommandRegistration {
                    id: "fixture.profile-summary".to_owned(),
                    title: "Fixture Profile Summary".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                CommandRegistration {
                    id: "fixture.profile-picker".to_owned(),
                    title: "Fixture Profile Picker".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                CommandRegistration {
                    id: "fixture.profile-launch".to_owned(),
                    title: "Fixture Profile Launch".to_owned(),
                    source_plugin: "fixture".to_owned(),
                }
            ]
        );

        let actions = app
            .invoke_command("fixture.echo", serde_json::json!({"value": "ok"}))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: br#"echo fixture {"value":"ok"}
"#
                .to_vec()
            }]
        );
        assert_eq!(
            app.transport().written(),
            br#"echo fixture {"value":"ok"}
"#
        );

        app.invoke_command("fixture.host-info", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.transport().written(),
            format!(
                "echo fixture {{\"value\":\"ok\"}}\nhost Witty {} {}\n",
                env!("CARGO_PKG_VERSION"),
                witty_plugin_api::PLUGIN_ABI_VERSION
            )
            .as_bytes()
        );

        app.invoke_command("fixture.profile-summary", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.transport().written(),
            format!(
                "echo fixture {{\"value\":\"ok\"}}\nhost Witty {} {}\nprofiles none\n",
                env!("CARGO_PKG_VERSION"),
                witty_plugin_api::PLUGIN_ABI_VERSION
            )
            .as_bytes()
        );

        let profile_picker_actions = app
            .invoke_command("fixture.profile-picker", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            profile_picker_actions,
            vec![PluginAction::RequestProfilePicker(
                PluginProfilePickerRequest {
                    reason: Some("fixture profile command".to_owned()),
                }
            )]
        );
        assert_eq!(
            app.profile_picker_requests(),
            &[PendingProfilePickerRequest {
                source_plugin: "fixture".to_owned(),
                reason: Some("fixture profile command".to_owned()),
            }]
        );
        assert_eq!(
            app.take_profile_picker_requests(),
            vec![PendingProfilePickerRequest {
                source_plugin: "fixture".to_owned(),
                reason: Some("fixture profile command".to_owned()),
            }]
        );
        assert!(app.profile_picker_requests().is_empty());

        let profile_launch_actions = app
            .invoke_command("fixture.profile-launch", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            profile_launch_actions,
            vec![PluginAction::RequestProfileLaunch(
                PluginProfileLaunchRequest {
                    profile_id: "prod".to_owned(),
                    reason: Some("fixture launch command".to_owned()),
                }
            )]
        );
        assert_eq!(
            app.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "fixture".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("fixture launch command".to_owned()),
            }]
        );
        assert_eq!(
            app.take_profile_launch_requests(),
            vec![PendingProfileLaunchRequest {
                source_plugin: "fixture".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("fixture launch command".to_owned()),
            }]
        );
        assert!(app.profile_launch_requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wasm_plugin_profile_summary_injected_into_app_install_path() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();

        app.install_wasm_plugin_from_file_with_profile_store_summary(
            &runtime,
            component_path,
            &profile_store_summary_fixture(),
        )
        .unwrap();

        app.invoke_command("fixture.profile-summary", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.transport().written(),
            b"profiles 2 default=true launchable=1 resolver=1\n"
        );
    }

    #[test]
    fn app_reviews_pending_profile_picker_requests() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfilePickerPlugin).unwrap();

        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();

        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.tag("work");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![profile])
        };

        assert_eq!(
            app.review_pending_profile_picker_requests(&store).unwrap(),
            vec![PendingProfilePickerRequestReview {
                source_plugin: "profiles".to_owned(),
                reason: Some("choose profile".to_owned()),
                summary: store.redacted_summary().unwrap(),
            }]
        );
        assert_eq!(app.profile_picker_requests().len(), 1);
    }

    #[test]
    fn app_reviews_pending_profile_actions_with_queue_keys() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileActionsPlugin).unwrap();

        app.invoke_command("profile-actions.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profile-actions.launch", serde_json::Value::Null)
            .unwrap();

        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.tag("work");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![profile])
        };

        assert_eq!(
            app.review_pending_profile_actions(&store).unwrap(),
            vec![
                PendingProfileActionReview::ProfilePicker {
                    key: PendingProfileActionKey::profile_picker(0),
                    request: PendingProfilePickerRequestReview {
                        source_plugin: "profile-actions".to_owned(),
                        reason: Some("choose profile".to_owned()),
                        summary: store.redacted_summary().unwrap(),
                    },
                },
                PendingProfileActionReview::ProfileLaunch {
                    key: PendingProfileActionKey::profile_launch(0),
                    request: PendingProfileLaunchRequestReview {
                        source_plugin: "profile-actions".to_owned(),
                        profile_id: "prod".to_owned(),
                        reason: Some("open production".to_owned()),
                        status: PendingProfileLaunchRequestStatus::Launchable,
                        profile_name: Some("Production".to_owned()),
                        tags: vec!["work".to_owned()],
                        is_default: true,
                    },
                },
            ]
        );
        assert_eq!(app.profile_picker_requests().len(), 1);
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[test]
    fn app_dismisses_pending_profile_action_by_key() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileActionsPlugin).unwrap();

        app.invoke_command("profile-actions.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profile-actions.launch", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.dismiss_pending_profile_action(PendingProfileActionKey::profile_picker(0))
                .unwrap(),
            DismissedPendingProfileAction::ProfilePicker {
                key: PendingProfileActionKey::profile_picker(0),
                request: PendingProfilePickerRequest {
                    source_plugin: "profile-actions".to_owned(),
                    reason: Some("choose profile".to_owned()),
                },
            }
        );
        assert_eq!(
            app.dismiss_pending_profile_action(PendingProfileActionKey::profile_launch(0))
                .unwrap(),
            DismissedPendingProfileAction::ProfileLaunch {
                key: PendingProfileActionKey::profile_launch(0),
                request: PendingProfileLaunchRequest {
                    source_plugin: "profile-actions".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("open production".to_owned()),
                },
            }
        );

        assert!(app
            .dismiss_pending_profile_action(PendingProfileActionKey::profile_picker(0))
            .is_err());
        assert!(app.profile_picker_requests().is_empty());
        assert!(app.profile_launch_requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_takes_resolved_pending_profile_action_pty_config_by_key() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileActionsPlugin).unwrap();

        app.invoke_command("profile-actions.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profile-actions.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert_eq!(
            app.take_resolved_pending_profile_action_pty_config(
                &store,
                PendingProfileActionConfirmation::profile_picker(
                    PendingProfileActionKey::profile_picker(0),
                    "prod",
                ),
                GridSize::new(24, 80),
            )
            .unwrap(),
            ResolvedPendingProfileActionPtyConfig::ProfilePicker {
                key: PendingProfileActionKey::profile_picker(0),
                resolved: ResolvedProfilePickerPtyConfig {
                    source_plugin: "profile-actions".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("choose profile".to_owned()),
                    config: {
                        let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                        config.env("TERM", "xterm-256color");
                        config.args(["-tt", "prod.example.com"]);
                        config
                    },
                },
            }
        );
        assert!(app.profile_picker_requests().is_empty());
        assert_eq!(app.profile_launch_requests().len(), 1);

        assert_eq!(
            app.take_resolved_pending_profile_action_pty_config(
                &store,
                PendingProfileActionConfirmation::profile_launch(
                    PendingProfileActionKey::profile_launch(0),
                ),
                GridSize::new(24, 80),
            )
            .unwrap(),
            ResolvedPendingProfileActionPtyConfig::ProfileLaunch {
                key: PendingProfileActionKey::profile_launch(0),
                resolved: ResolvedProfileLaunchPtyConfig {
                    source_plugin: "profile-actions".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("open production".to_owned()),
                    config: {
                        let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                        config.env("TERM", "xterm-256color");
                        config.args(["-tt", "prod.example.com"]);
                        config
                    },
                },
            }
        );
        assert!(app.profile_launch_requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_keeps_pending_profile_action_queues_when_unified_take_fails() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileActionsPlugin).unwrap();

        app.invoke_command("profile-actions.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profile-actions.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::new();

        assert!(app
            .take_resolved_pending_profile_action_pty_config(
                &store,
                PendingProfileActionConfirmation::profile_picker(
                    PendingProfileActionKey::profile_picker(0),
                    "prod",
                ),
                GridSize::new(24, 80),
            )
            .is_err());
        assert_eq!(app.profile_picker_requests().len(), 1);
        assert_eq!(app.profile_launch_requests().len(), 1);

        assert!(app
            .take_resolved_pending_profile_action_pty_config(
                &store,
                PendingProfileActionConfirmation::profile_launch(
                    PendingProfileActionKey::profile_picker(0),
                ),
                GridSize::new(24, 80),
            )
            .is_err());
        assert_eq!(app.profile_picker_requests().len(), 1);
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[test]
    fn app_dismisses_pending_profile_picker_request_without_resolving() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfilePickerPlugin).unwrap();
        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.dismiss_pending_profile_picker_request(0).unwrap(),
            PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("choose profile".to_owned()),
            }
        );
        assert_eq!(
            app.profile_picker_requests(),
            &[PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("choose profile".to_owned()),
            }]
        );

        assert!(app.dismiss_pending_profile_picker_request(1).is_err());
        assert_eq!(app.profile_picker_requests().len(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_resolves_pending_profile_picker_selection_without_draining() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfilePickerPlugin).unwrap();
        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert_eq!(
            app.resolve_pending_profile_picker_selection(&store, 0, "prod")
                .unwrap(),
            ResolvedProfilePickerSelection {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("choose profile".to_owned()),
                profile: SshProfile::new("prod", "Production", "prod.example.com"),
            }
        );
        assert_eq!(
            app.resolve_pending_profile_picker_pty_config(&store, 0, "prod", GridSize::new(24, 80))
                .unwrap(),
            ResolvedProfilePickerPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("choose profile".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }
        );
        assert_eq!(app.profile_picker_requests().len(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_takes_resolved_profile_picker_pty_config_after_success() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfilePickerPlugin).unwrap();
        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert_eq!(
            app.take_resolved_profile_picker_pty_config(&store, 0, "prod", GridSize::new(24, 80))
                .unwrap(),
            ResolvedProfilePickerPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("choose profile".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }
        );
        assert!(app.profile_picker_requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_keeps_profile_picker_queue_when_take_resolution_fails() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfilePickerPlugin).unwrap();
        app.invoke_command("profiles.pick", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::new();

        assert!(app
            .take_resolved_profile_picker_pty_config(&store, 0, "prod", GridSize::new(24, 80))
            .is_err());
        assert_eq!(
            app.profile_picker_requests(),
            &[PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("choose profile".to_owned()),
            }]
        );
    }

    #[test]
    fn app_profile_picker_selection_rejects_missing_request_index() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let app = TerminalApp::new(transport, GridSize::new(24, 80));
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert!(app
            .resolve_pending_profile_picker_selection(&store, 0, "prod")
            .is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_reviews_and_resolves_pending_profile_launch_requests() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();

        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.tag("work");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![profile])
        };

        assert_eq!(
            app.review_pending_profile_launch_requests(&store).unwrap(),
            vec![PendingProfileLaunchRequestReview {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                status: PendingProfileLaunchRequestStatus::Launchable,
                profile_name: Some("Production".to_owned()),
                tags: vec!["work".to_owned()],
                is_default: true,
            }]
        );
        assert_eq!(
            app.resolve_pending_profile_launch_pty_configs(&store, GridSize::new(24, 80))
                .unwrap(),
            vec![ResolvedProfileLaunchPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }]
        );
        assert_eq!(
            app.resolve_pending_profile_launch_pty_config(&store, 0, GridSize::new(24, 80))
                .unwrap(),
            ResolvedProfileLaunchPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }
        );
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[test]
    fn app_dismisses_pending_profile_launch_request_without_resolving() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        assert_eq!(
            app.dismiss_pending_profile_launch_request(0).unwrap(),
            PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
            }
        );
        assert_eq!(
            app.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
            }]
        );

        assert!(app.dismiss_pending_profile_launch_request(1).is_err());
        assert_eq!(app.profile_launch_requests().len(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_takes_resolved_profile_launch_pty_configs_after_success() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert_eq!(
            app.take_resolved_profile_launch_pty_configs(&store, GridSize::new(24, 80))
                .unwrap(),
            vec![ResolvedProfileLaunchPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }]
        );
        assert!(app.profile_launch_requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_takes_one_resolved_profile_launch_pty_config_after_success() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert_eq!(
            app.take_resolved_profile_launch_pty_config(&store, 0, GridSize::new(24, 80))
                .unwrap(),
            ResolvedProfileLaunchPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(24, 80), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "prod.example.com"]);
                    config
                },
            }
        );
        assert_eq!(
            app.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
            }]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_keeps_one_profile_launch_queue_when_take_resolution_fails() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::new();

        assert!(app
            .take_resolved_profile_launch_pty_config(&store, 0, GridSize::new(24, 80))
            .is_err());
        assert_eq!(
            app.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
            }]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn app_keeps_profile_launch_queue_when_take_resolution_fails() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));
        app.install_builtin_plugin(ProfileLaunchPlugin).unwrap();
        app.invoke_command("profiles.launch", serde_json::Value::Null)
            .unwrap();

        let store = ProfileStoreV1::new();

        assert!(app
            .take_resolved_profile_launch_pty_configs(&store, GridSize::new(24, 80))
            .is_err());
        assert_eq!(
            app.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("open production".to_owned()),
            }]
        );
    }

    #[test]
    fn invoke_command_fails_for_unknown_command() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        assert!(app
            .invoke_command("missing.command", serde_json::Value::Null)
            .is_err());
    }

    #[test]
    fn runtime_registered_command_joins_app_and_host_routing() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.install_builtin_plugin(DynamicRegistrationPlugin {
            id: "dynamic",
            command_id: "dynamic.run",
        })
        .unwrap();

        let registered = app.dispatch_plugin_event(PluginEvent::AppStarted).unwrap();
        assert_eq!(
            registered,
            vec![PluginAction::RegisterCommand(CommandRegistration {
                id: "dynamic.run".to_owned(),
                title: "Dynamic Command".to_owned(),
                source_plugin: "dynamic".to_owned(),
            })]
        );
        assert!(app
            .commands()
            .iter()
            .any(|command| command.id == "dynamic.run" && command.source_plugin == "dynamic"));

        let actions = app
            .invoke_command("dynamic.run", serde_json::Value::Null)
            .unwrap();
        assert_eq!(
            actions,
            vec![PluginAction::ShowMessage {
                message: "dynamic handled dynamic.run".to_owned(),
            }]
        );
    }

    #[test]
    fn runtime_registered_command_cannot_collide_with_app_registry() {
        let transport = MockTransport::new(GridSize::new(24, 80));
        let mut app = TerminalApp::new(transport, GridSize::new(24, 80));

        app.register_command(command("builtin.search", "builtin"))
            .unwrap();
        app.install_builtin_plugin(DynamicRegistrationPlugin {
            id: "dynamic",
            command_id: "builtin.search",
        })
        .unwrap();

        assert!(app.dispatch_plugin_event(PluginEvent::AppStarted).is_err());
        assert_eq!(
            app.commands()
                .iter()
                .filter(|command| command.id == "builtin.search")
                .count(),
            1
        );
    }

    fn command(id: &str, source_plugin: &str) -> CommandRegistration {
        CommandRegistration {
            id: id.to_owned(),
            title: id.to_owned(),
            source_plugin: source_plugin.to_owned(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_fixture_component() -> std::path::PathBuf {
        let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("witty-plugin-wasm")
            .join("fixtures")
            .join("guest-plugin");
        let manifest_path = fixture_dir.join("Cargo.toml");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = std::process::Command::new(cargo)
            .args([
                "build",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
                manifest_path
                    .to_str()
                    .expect("fixture manifest path is utf-8"),
            ])
            .status()
            .expect("fixture component build should start");

        assert!(status.success(), "fixture component build failed");

        fixture_dir
            .join("target")
            .join("wasm32-wasip2")
            .join("debug")
            .join("witty_fixture_plugin.wasm")
    }

    fn profile_store_summary_fixture() -> ProfileStoreSummary {
        ProfileStoreSummary {
            profiles: vec![
                ProfileSummary {
                    id: "prod".to_owned(),
                    name: "Production".to_owned(),
                    tags: vec!["work".to_owned()],
                    launchability: SshProfileLaunchability::Launchable,
                    is_default: true,
                },
                ProfileSummary {
                    id: "vaulted".to_owned(),
                    name: "Vaulted".to_owned(),
                    tags: Vec::new(),
                    launchability: SshProfileLaunchability::RequiresCredentialResolver,
                    is_default: false,
                },
            ],
            default_profile_id: Some("prod".to_owned()),
            launchable_profiles: 1,
            credential_resolver_required_profiles: 1,
        }
    }

    struct EchoPlugin {
        id: &'static str,
        write_permission: TerminalWritePermission,
    }

    impl EchoPlugin {
        fn new(id: &'static str, write_permission: TerminalWritePermission) -> Self {
            Self {
                id,
                write_permission,
            }
        }
    }

    impl BuiltInPlugin for EchoPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Echo Plugin".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::None,
                    terminal_write: self.write_permission.clone(),
                    profile_read: false,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![command(&format!("{}.run", self.id), self.id)]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            if invocation.command_id != format!("{}.run", self.id) {
                return Ok(Vec::new());
            }

            Ok(vec![PluginAction::WriteTerminal {
                bytes: b"echo from plugin\n".to_vec(),
            }])
        }
    }

    struct ContextEchoPlugin;

    struct DynamicRegistrationPlugin {
        id: &'static str,
        command_id: &'static str,
    }

    struct ProfilePickerPlugin;

    struct ProfileLaunchPlugin;

    struct ProfileActionsPlugin;

    impl BuiltInPlugin for ContextEchoPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "context".to_owned(),
                name: "Context Echo Plugin".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::CurrentScreen,
                    terminal_write: TerminalWritePermission::AllowSession,
                    profile_read: false,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![command("context.run", "context")]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            if invocation.command_id != "context.run" {
                return Ok(Vec::new());
            }
            let path = invocation
                .context
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str())
                .unwrap_or("none");
            Ok(vec![PluginAction::WriteTerminal {
                bytes: format!("pwd {path}\n").into_bytes(),
            }])
        }
    }

    impl BuiltInPlugin for DynamicRegistrationPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Dynamic Registration".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::CurrentScreen,
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: false,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            match event {
                PluginEvent::AppStarted => {
                    Ok(vec![PluginAction::RegisterCommand(CommandRegistration {
                        id: self.command_id.to_owned(),
                        title: "Dynamic Command".to_owned(),
                        source_plugin: self.id.to_owned(),
                    })])
                }
                PluginEvent::CommandInvoked(invocation)
                    if invocation.command_id == self.command_id =>
                {
                    Ok(vec![PluginAction::ShowMessage {
                        message: format!("{} handled {}", self.id, invocation.command_id),
                    }])
                }
                _ => Ok(Vec::new()),
            }
        }
    }

    impl BuiltInPlugin for ProfilePickerPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "profiles".to_owned(),
                name: "Profile Picker".to_owned(),
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
            vec![command("profiles.pick", "profiles")]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            if invocation.command_id != "profiles.pick" {
                return Ok(Vec::new());
            }
            Ok(vec![PluginAction::RequestProfilePicker(
                PluginProfilePickerRequest {
                    reason: Some("choose profile".to_owned()),
                },
            )])
        }
    }

    impl BuiltInPlugin for ProfileLaunchPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "profiles".to_owned(),
                name: "Profile Launch".to_owned(),
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
            vec![command("profiles.launch", "profiles")]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            if invocation.command_id != "profiles.launch" {
                return Ok(Vec::new());
            }
            Ok(vec![PluginAction::RequestProfileLaunch(
                PluginProfileLaunchRequest {
                    profile_id: "prod".to_owned(),
                    reason: Some("open production".to_owned()),
                },
            )])
        }
    }

    impl BuiltInPlugin for ProfileActionsPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "profile-actions".to_owned(),
                name: "Profile Actions".to_owned(),
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
                command("profile-actions.pick", "profile-actions"),
                command("profile-actions.launch", "profile-actions"),
            ]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            match invocation.command_id.as_str() {
                "profile-actions.pick" => Ok(vec![PluginAction::RequestProfilePicker(
                    PluginProfilePickerRequest {
                        reason: Some("choose profile".to_owned()),
                    },
                )]),
                "profile-actions.launch" => Ok(vec![PluginAction::RequestProfileLaunch(
                    PluginProfileLaunchRequest {
                        profile_id: "prod".to_owned(),
                        reason: Some("open production".to_owned()),
                    },
                )]),
                _ => Ok(Vec::new()),
            }
        }
    }
}
