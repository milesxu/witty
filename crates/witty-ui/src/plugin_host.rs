#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use anyhow::{bail, Context, Result};
#[cfg(not(target_arch = "wasm32"))]
use witty_core::GridSize;
use witty_plugin_api::{
    CommandInvocationContext, CommandRegistration, PluginAction, PluginEvent, PluginManifest,
    PluginPermissions, PluginProfileLaunchRequest, PluginProfileStoreSummary, PluginRuntime,
    TerminalReadPermission, TerminalWritePermission,
};
#[cfg(not(target_arch = "wasm32"))]
use witty_plugin_wasm::{WasmPluginInstance, WasmPluginRuntime, WasmPluginState};
#[cfg(not(target_arch = "wasm32"))]
use witty_transport::LocalPtyConfig;
use witty_transport::{ProfileStoreSummary, ProfileStoreV1, SshProfile, SshProfileLaunchability};

pub trait BuiltInPlugin: Send {
    fn manifest(&self) -> PluginManifest;

    fn commands(&self) -> Vec<CommandRegistration> {
        Vec::new()
    }

    fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>>;
}

#[derive(Default)]
pub struct PluginHost {
    plugins: Vec<InstalledPlugin>,
    commands: Vec<CommandRegistration>,
    disabled_plugins: Vec<String>,
    runtime_failures: Vec<PluginRuntimeFailure>,
    profile_picker_requests: Vec<PendingProfilePickerRequest>,
    profile_launch_requests: Vec<PendingProfileLaunchRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginRuntimeFailure {
    pub plugin_id: String,
    pub kind: PluginRuntimeFailureKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginRuntimeFailureKind {
    EventHandlerError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingProfilePickerRequest {
    pub source_plugin: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingProfilePickerRequestReview {
    pub source_plugin: String,
    pub reason: Option<String>,
    pub summary: ProfileStoreSummary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfilePickerSelection {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub profile: SshProfile,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfilePickerPtyConfig {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub config: LocalPtyConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingProfileLaunchRequest {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingProfileLaunchRequestReview {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub status: PendingProfileLaunchRequestStatus,
    pub profile_name: Option<String>,
    pub tags: Vec<String>,
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingProfileLaunchRequestStatus {
    Launchable,
    RequiresCredentialResolver,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfileLaunchRequest {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub profile: SshProfile,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfileLaunchPtyConfig {
    pub source_plugin: String,
    pub profile_id: String,
    pub reason: Option<String>,
    pub config: LocalPtyConfig,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct PreparedWasmPlugin {
    manifest: PluginManifest,
    commands: Vec<CommandRegistration>,
    plugin: WasmPluginInstance,
}

#[cfg(not(target_arch = "wasm32"))]
impl PreparedWasmPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn commands(&self) -> &[CommandRegistration] {
        &self.commands
    }
}

impl PluginHost {
    pub fn install_builtin<P>(&mut self, plugin: P) -> Result<Vec<CommandRegistration>>
    where
        P: BuiltInPlugin + 'static,
    {
        let manifest = plugin.manifest();
        if manifest.runtime != PluginRuntime::BuiltIn {
            bail!(
                "plugin {} uses {:?}, expected BuiltIn runtime",
                manifest.id,
                manifest.runtime
            );
        }

        let commands = plugin.commands();
        self.validate_install(&manifest, &commands)?;

        self.commands.extend(commands.clone());
        self.plugins.push(InstalledPlugin::BuiltIn {
            manifest,
            plugin: Box::new(plugin),
        });
        Ok(commands)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare_wasm_component_from_file(
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
    ) -> Result<PreparedWasmPlugin> {
        Self::prepare_wasm_component_from_file_with_state(runtime, path, WasmPluginState::new())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare_wasm_component_from_file_with_profile_store_summary(
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
        summary: &ProfileStoreSummary,
    ) -> Result<PreparedWasmPlugin> {
        Self::prepare_wasm_component_from_file_with_state(
            runtime,
            path,
            WasmPluginState::with_profile_store_summary(
                plugin_profile_store_summary_from_redacted(summary)?,
            ),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_wasm_component_from_file_with_state(
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
        state: WasmPluginState,
    ) -> Result<PreparedWasmPlugin> {
        let component = runtime.component_from_file(path)?;
        let plugin = runtime.instantiate_component(&component, state)?;

        Self::prepare_wasm_instance(plugin)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare_wasm_instance(mut plugin: WasmPluginInstance) -> Result<PreparedWasmPlugin> {
        let manifest = plugin.manifest()?;
        if manifest.runtime != PluginRuntime::WasmComponent {
            bail!(
                "plugin {} uses {:?}, expected WasmComponent runtime",
                manifest.id,
                manifest.runtime
            );
        }
        let commands = plugin.commands()?;

        Ok(PreparedWasmPlugin {
            manifest,
            commands,
            plugin,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn install_wasm(
        &mut self,
        prepared: PreparedWasmPlugin,
    ) -> Result<Vec<CommandRegistration>> {
        self.validate_install(&prepared.manifest, &prepared.commands)?;

        let commands = prepared.commands.clone();
        self.commands.extend(commands.clone());
        self.plugins.push(InstalledPlugin::Wasm {
            manifest: prepared.manifest,
            plugin: prepared.plugin,
        });

        Ok(commands)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn install_wasm_component_from_file(
        &mut self,
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
    ) -> Result<Vec<CommandRegistration>> {
        let prepared = Self::prepare_wasm_component_from_file(runtime, path)?;

        self.install_wasm(prepared)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn install_wasm_component_from_file_with_profile_store_summary(
        &mut self,
        runtime: &WasmPluginRuntime,
        path: impl AsRef<Path>,
        summary: &ProfileStoreSummary,
    ) -> Result<Vec<CommandRegistration>> {
        let prepared = Self::prepare_wasm_component_from_file_with_profile_store_summary(
            runtime, path, summary,
        )?;

        self.install_wasm(prepared)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &PluginManifest> {
        self.plugins.iter().map(InstalledPlugin::manifest)
    }

    pub fn commands(&self) -> &[CommandRegistration] {
        &self.commands
    }

    pub fn disabled_plugin_ids(&self) -> &[String] {
        &self.disabled_plugins
    }

    pub fn runtime_failures(&self) -> &[PluginRuntimeFailure] {
        &self.runtime_failures
    }

    pub fn profile_picker_requests(&self) -> &[PendingProfilePickerRequest] {
        &self.profile_picker_requests
    }

    pub fn take_profile_picker_requests(&mut self) -> Vec<PendingProfilePickerRequest> {
        std::mem::take(&mut self.profile_picker_requests)
    }

    pub fn take_profile_picker_request(
        &mut self,
        request_index: usize,
    ) -> Result<PendingProfilePickerRequest> {
        if request_index >= self.profile_picker_requests.len() {
            bail!("profile picker request index {request_index} out of range");
        }
        Ok(self.profile_picker_requests.remove(request_index))
    }

    pub fn profile_launch_requests(&self) -> &[PendingProfileLaunchRequest] {
        &self.profile_launch_requests
    }

    pub fn take_profile_launch_requests(&mut self) -> Vec<PendingProfileLaunchRequest> {
        std::mem::take(&mut self.profile_launch_requests)
    }

    pub fn take_profile_launch_request(
        &mut self,
        request_index: usize,
    ) -> Result<PendingProfileLaunchRequest> {
        if request_index >= self.profile_launch_requests.len() {
            bail!("profile launch request index {request_index} out of range");
        }
        Ok(self.profile_launch_requests.remove(request_index))
    }

    pub fn dispatch_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
        self.dispatch_event_with_reserved_commands(event, &[])
    }

    pub fn dispatch_event_with_reserved_commands(
        &mut self,
        event: &PluginEvent,
        reserved_commands: &[CommandRegistration],
    ) -> Result<Vec<PluginAction>> {
        let mut actions = Vec::new();
        let mut registered_commands = Vec::new();
        let mut disabled_by_dispatch = Vec::new();
        let command_owner = match event {
            PluginEvent::CommandInvoked(invocation) => self.command_owner(&invocation.command_id),
            _ => None,
        }
        .map(str::to_owned);

        if matches!(event, PluginEvent::CommandInvoked(_)) && command_owner.is_none() {
            return Ok(actions);
        }
        if command_owner
            .as_ref()
            .is_some_and(|owner| self.is_plugin_disabled(owner))
        {
            return Ok(actions);
        }

        for installed in &mut self.plugins {
            let plugin_id = installed.manifest().id.clone();
            if self.disabled_plugins.iter().any(|id| id == &plugin_id) {
                continue;
            }
            if command_owner
                .as_ref()
                .is_some_and(|owner| owner != &plugin_id)
            {
                continue;
            }

            let permissions = installed.manifest().permissions.clone();
            let Some(plugin_event) = plugin_event_for_permissions(event, &permissions) else {
                continue;
            };
            let plugin_actions = match installed.handle_event(&plugin_event) {
                Ok(actions) => actions,
                Err(error) => {
                    self.runtime_failures.push(PluginRuntimeFailure {
                        plugin_id: plugin_id.clone(),
                        kind: PluginRuntimeFailureKind::EventHandlerError,
                        message: error.to_string(),
                    });
                    disabled_by_dispatch.push(plugin_id);
                    continue;
                }
            };
            validate_actions(&plugin_id, &permissions, &plugin_actions)?;
            validate_registered_commands(
                &self.commands,
                reserved_commands,
                &registered_commands,
                &plugin_actions,
            )?;
            registered_commands.extend(plugin_actions.iter().filter_map(|action| {
                if let PluginAction::RegisterCommand(command) = action {
                    Some(command.clone())
                } else {
                    None
                }
            }));
            self.profile_picker_requests
                .extend(plugin_actions.iter().filter_map(|action| match action {
                    PluginAction::RequestProfilePicker(request) => {
                        Some(PendingProfilePickerRequest {
                            source_plugin: plugin_id.clone(),
                            reason: request.reason.clone(),
                        })
                    }
                    _ => None,
                }));
            self.profile_launch_requests
                .extend(plugin_actions.iter().filter_map(|action| match action {
                    PluginAction::RequestProfileLaunch(request) => {
                        Some(PendingProfileLaunchRequest {
                            source_plugin: plugin_id.clone(),
                            profile_id: request.profile_id.clone(),
                            reason: request.reason.clone(),
                        })
                    }
                    _ => None,
                }));
            actions.extend(plugin_actions);
        }
        for plugin_id in disabled_by_dispatch {
            if !self.disabled_plugins.iter().any(|id| id == &plugin_id) {
                self.disabled_plugins.push(plugin_id);
            }
        }
        self.commands.extend(registered_commands);
        Ok(actions)
    }

    fn command_owner(&self, command_id: &str) -> Option<&str> {
        self.commands
            .iter()
            .find(|command| command.id == command_id)
            .map(|command| command.source_plugin.as_str())
    }

    fn is_plugin_disabled(&self, plugin_id: &str) -> bool {
        self.disabled_plugins.iter().any(|id| id == plugin_id)
    }

    fn validate_install(
        &self,
        manifest: &PluginManifest,
        commands: &[CommandRegistration],
    ) -> Result<()> {
        if self
            .plugins
            .iter()
            .any(|installed| installed.manifest().id == manifest.id)
        {
            bail!("duplicate plugin id {}", manifest.id);
        }

        for command in commands {
            if command.source_plugin != manifest.id {
                bail!(
                    "command {} belongs to {}, expected {}",
                    command.id,
                    command.source_plugin,
                    manifest.id
                );
            }
            if self
                .commands
                .iter()
                .any(|existing| existing.id == command.id)
                || commands
                    .iter()
                    .filter(|other| other.id == command.id)
                    .count()
                    > 1
            {
                bail!("duplicate command id {}", command.id);
            }
        }

        Ok(())
    }
}

pub fn plugin_profile_store_summary_from_redacted(
    summary: &ProfileStoreSummary,
) -> Result<PluginProfileStoreSummary> {
    Ok(PluginProfileStoreSummary {
        profile_count: plugin_summary_u32("profile count", summary.profiles.len())?,
        default_profile_configured: summary.default_profile_id.is_some(),
        launchable_profiles: plugin_summary_u32(
            "launchable profile count",
            summary.launchable_profiles,
        )?,
        credential_resolver_required_profiles: plugin_summary_u32(
            "credential resolver required profile count",
            summary.credential_resolver_required_profiles,
        )?,
    })
}

pub fn review_profile_picker_requests(
    store: &ProfileStoreV1,
    requests: &[PendingProfilePickerRequest],
) -> Result<Vec<PendingProfilePickerRequestReview>> {
    let summary = store.redacted_summary()?;
    requests
        .iter()
        .map(|request| {
            validate_profile_picker_request(request.reason.as_deref())?;
            Ok(PendingProfilePickerRequestReview {
                source_plugin: request.source_plugin.clone(),
                reason: request.reason.clone(),
                summary: summary.clone(),
            })
        })
        .collect()
}

pub fn resolve_profile_picker_selection(
    store: &ProfileStoreV1,
    request: &PendingProfilePickerRequest,
    profile_id: &str,
) -> Result<ResolvedProfilePickerSelection> {
    store.validate()?;
    validate_profile_picker_request(request.reason.as_deref())?;
    validate_plugin_profile_id(profile_id)?;

    let profile = store
        .profile(profile_id)
        .with_context(|| format!("profile picker selection id {profile_id:?} was not found"))?;

    match profile.launchability()? {
        SshProfileLaunchability::Launchable => Ok(ResolvedProfilePickerSelection {
            source_plugin: request.source_plugin.clone(),
            profile_id: profile_id.to_owned(),
            reason: request.reason.clone(),
            profile: profile.clone(),
        }),
        SshProfileLaunchability::RequiresCredentialResolver => {
            bail!("profile picker selection id {profile_id:?} requires a credential resolver")
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_profile_picker_pty_config(
    store: &ProfileStoreV1,
    request: &PendingProfilePickerRequest,
    profile_id: &str,
    size: GridSize,
) -> Result<ResolvedProfilePickerPtyConfig> {
    let resolved = resolve_profile_picker_selection(store, request, profile_id)?;
    let config = resolved
        .profile
        .to_openssh_profile()
        .context("convert profile picker selection to OpenSSH profile")?
        .to_local_pty_config(size)
        .context("convert profile picker selection to local PTY config")?;
    Ok(ResolvedProfilePickerPtyConfig {
        source_plugin: resolved.source_plugin,
        profile_id: resolved.profile_id,
        reason: resolved.reason,
        config,
    })
}

pub fn review_profile_launch_requests(
    store: &ProfileStoreV1,
    requests: &[PendingProfileLaunchRequest],
) -> Result<Vec<PendingProfileLaunchRequestReview>> {
    store.validate()?;
    requests
        .iter()
        .map(|request| {
            validate_plugin_profile_id(&request.profile_id)?;
            validate_profile_request_reason(
                "profile launch request reason",
                request.reason.as_deref(),
            )?;
            Ok(match store.profile(&request.profile_id) {
                Some(profile) => {
                    let status = match profile.launchability()? {
                        SshProfileLaunchability::Launchable => {
                            PendingProfileLaunchRequestStatus::Launchable
                        }
                        SshProfileLaunchability::RequiresCredentialResolver => {
                            PendingProfileLaunchRequestStatus::RequiresCredentialResolver
                        }
                    };
                    PendingProfileLaunchRequestReview {
                        source_plugin: request.source_plugin.clone(),
                        profile_id: request.profile_id.clone(),
                        reason: request.reason.clone(),
                        status,
                        profile_name: Some(profile.name.clone()),
                        tags: profile.tags.clone(),
                        is_default: store.default_profile_id.as_deref()
                            == Some(profile.id.as_str()),
                    }
                }
                None => PendingProfileLaunchRequestReview {
                    source_plugin: request.source_plugin.clone(),
                    profile_id: request.profile_id.clone(),
                    reason: request.reason.clone(),
                    status: PendingProfileLaunchRequestStatus::NotFound,
                    profile_name: None,
                    tags: Vec::new(),
                    is_default: false,
                },
            })
        })
        .collect()
}

pub fn resolve_profile_launch_request(
    store: &ProfileStoreV1,
    request: &PendingProfileLaunchRequest,
) -> Result<ResolvedProfileLaunchRequest> {
    store.validate()?;
    validate_plugin_profile_id(&request.profile_id)?;
    validate_profile_request_reason("profile launch request reason", request.reason.as_deref())?;

    let profile = store.profile(&request.profile_id).with_context(|| {
        format!(
            "profile launch request id {:?} was not found",
            request.profile_id
        )
    })?;

    match profile.launchability()? {
        SshProfileLaunchability::Launchable => Ok(ResolvedProfileLaunchRequest {
            source_plugin: request.source_plugin.clone(),
            profile_id: request.profile_id.clone(),
            reason: request.reason.clone(),
            profile: profile.clone(),
        }),
        SshProfileLaunchability::RequiresCredentialResolver => {
            bail!(
                "profile launch request id {:?} requires a credential resolver",
                request.profile_id
            )
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_profile_launch_pty_config(
    store: &ProfileStoreV1,
    request: &PendingProfileLaunchRequest,
    size: GridSize,
) -> Result<ResolvedProfileLaunchPtyConfig> {
    let resolved = resolve_profile_launch_request(store, request)?;
    let config = resolved
        .profile
        .to_openssh_profile()
        .context("convert profile launch request to OpenSSH profile")?
        .to_local_pty_config(size)
        .context("convert profile launch request to local PTY config")?;
    Ok(ResolvedProfileLaunchPtyConfig {
        source_plugin: resolved.source_plugin,
        profile_id: resolved.profile_id,
        reason: resolved.reason,
        config,
    })
}

fn plugin_summary_u32(field: &str, value: usize) -> Result<u32> {
    u32::try_from(value).with_context(|| format!("{field} exceeds plugin ABI u32 range"))
}

enum InstalledPlugin {
    BuiltIn {
        manifest: PluginManifest,
        plugin: Box<dyn BuiltInPlugin>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Wasm {
        manifest: PluginManifest,
        plugin: WasmPluginInstance,
    },
}

impl InstalledPlugin {
    fn manifest(&self) -> &PluginManifest {
        match self {
            Self::BuiltIn { manifest, .. } => manifest,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Wasm { manifest, .. } => manifest,
        }
    }

    fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
        match self {
            Self::BuiltIn { plugin, .. } => plugin.handle_event(event),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Wasm { plugin, .. } => plugin.handle_event(event.clone()),
        }
    }
}

fn validate_actions(
    plugin_id: &str,
    permissions: &PluginPermissions,
    actions: &[PluginAction],
) -> Result<()> {
    for action in actions {
        match action {
            PluginAction::RegisterCommand(command) if command.source_plugin != plugin_id => {
                bail!(
                    "plugin {} attempted to register command {} for {}",
                    plugin_id,
                    command.id,
                    command.source_plugin
                );
            }
            PluginAction::WriteTerminal { .. }
                if permissions.terminal_write != TerminalWritePermission::AllowSession =>
            {
                bail!(
                    "plugin {} attempted terminal write without permission",
                    plugin_id
                );
            }
            PluginAction::RequestProfilePicker(request) => {
                if !permissions.profile_read {
                    bail!(
                        "plugin {} attempted profile picker request without profile-read permission",
                        plugin_id
                    );
                }
                validate_profile_picker_request(request.reason.as_deref())?;
            }
            PluginAction::RequestProfileLaunch(request) => {
                if !permissions.profile_read {
                    bail!(
                        "plugin {} attempted profile launch request without profile-read permission",
                        plugin_id
                    );
                }
                validate_profile_launch_request(request)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_profile_picker_request(reason: Option<&str>) -> Result<()> {
    validate_profile_request_reason("profile picker request reason", reason)
}

fn validate_profile_launch_request(request: &PluginProfileLaunchRequest) -> Result<()> {
    validate_plugin_profile_id(&request.profile_id)?;
    validate_profile_request_reason("profile launch request reason", request.reason.as_deref())
}

fn validate_plugin_profile_id(profile_id: &str) -> Result<()> {
    if profile_id.trim().is_empty() {
        bail!("profile launch request profile id must not be empty");
    }
    if profile_id.len() > 256 {
        bail!("profile launch request profile id exceeds 256 bytes");
    }
    if profile_id.chars().any(char::is_whitespace) {
        bail!("profile launch request profile id must not contain whitespace");
    }
    if profile_id.chars().any(char::is_control) {
        bail!("profile launch request profile id must not contain control characters");
    }
    Ok(())
}

fn validate_profile_request_reason(label: &str, reason: Option<&str>) -> Result<()> {
    let Some(reason) = reason else {
        return Ok(());
    };
    if reason.len() > 256 {
        bail!("{label} exceeds 256 bytes");
    }
    if reason.chars().any(char::is_control) {
        bail!("{label} contains control characters");
    }
    Ok(())
}

fn validate_registered_commands(
    existing_commands: &[CommandRegistration],
    reserved_commands: &[CommandRegistration],
    pending_commands: &[CommandRegistration],
    actions: &[PluginAction],
) -> Result<()> {
    let mut batch_ids = Vec::new();
    for action in actions {
        let PluginAction::RegisterCommand(command) = action else {
            continue;
        };

        if existing_commands
            .iter()
            .any(|existing| existing.id == command.id)
            || reserved_commands
                .iter()
                .any(|reserved| reserved.id == command.id)
            || pending_commands
                .iter()
                .any(|pending| pending.id == command.id)
            || batch_ids.iter().any(|id| id == &command.id)
        {
            bail!("duplicate command id {}", command.id);
        }
        batch_ids.push(command.id.clone());
    }
    Ok(())
}

fn plugin_event_for_permissions(
    event: &PluginEvent,
    permissions: &PluginPermissions,
) -> Option<PluginEvent> {
    match event {
        PluginEvent::CommandInvoked(invocation) => {
            let mut invocation = invocation.clone();
            invocation.context =
                command_context_for_permissions(&invocation.context, &permissions.terminal_read);
            Some(PluginEvent::CommandInvoked(invocation))
        }
        PluginEvent::TerminalOutput { .. }
            if !matches!(
                permissions.terminal_read,
                TerminalReadPermission::CurrentScreen | TerminalReadPermission::FullScrollback
            ) =>
        {
            None
        }
        PluginEvent::SelectionChanged { .. }
            if permissions.terminal_read == TerminalReadPermission::None =>
        {
            None
        }
        _ => Some(event.clone()),
    }
}

fn command_context_for_permissions(
    context: &CommandInvocationContext,
    terminal_read: &TerminalReadPermission,
) -> CommandInvocationContext {
    match terminal_read {
        TerminalReadPermission::None => CommandInvocationContext::default(),
        TerminalReadPermission::SelectionOnly => CommandInvocationContext {
            selected_command_block: context.selected_command_block.clone(),
            ..CommandInvocationContext::default()
        },
        TerminalReadPermission::CurrentScreen | TerminalReadPermission::FullScrollback => {
            context.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use witty_core::{CellPoint, CellRange, GridSize};
    use witty_plugin_api::{
        CommandInvocation, NetworkPermission, PluginCommandBlock, PluginCommandBlockTextRange,
        PluginCurrentDirectory, PluginProfileLaunchRequest, PluginProfilePickerRequest,
        PluginProfileStoreSummary, VaultPermission,
    };
    use witty_transport::{
        ProfileStoreSummary, ProfileStoreV1, ProfileSummary, SshCredentialRef, SshProfile,
        SshProfileLaunchability,
    };

    struct TestPlugin {
        id: &'static str,
        write_permission: TerminalWritePermission,
        write_on_start: bool,
    }

    struct ContextReporterPlugin {
        id: &'static str,
        terminal_read: TerminalReadPermission,
    }

    struct CommandReporterPlugin {
        id: &'static str,
        command_id: &'static str,
    }

    struct DynamicCommandPlugin {
        id: &'static str,
        command_id: &'static str,
    }

    struct EventReporterPlugin {
        id: &'static str,
        terminal_read: TerminalReadPermission,
    }

    struct FailingPlugin {
        id: &'static str,
        command_id: Option<&'static str>,
    }

    struct ProfilePickerPlugin {
        id: &'static str,
        profile_read: bool,
        reason: Option<&'static str>,
    }

    struct ProfileLaunchPlugin {
        id: &'static str,
        profile_read: bool,
        profile_id: &'static str,
        reason: Option<&'static str>,
    }

    impl BuiltInPlugin for TestPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Test Plugin".to_owned(),
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
            vec![CommandRegistration {
                id: format!("{}.hello", self.id),
                title: "Hello".to_owned(),
                source_plugin: self.id.to_owned(),
            }]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            if self.write_on_start && matches!(event, PluginEvent::AppStarted) {
                return Ok(vec![PluginAction::WriteTerminal {
                    bytes: b"echo plugin\n".to_vec(),
                }]);
            }
            Ok(Vec::new())
        }
    }

    impl BuiltInPlugin for ContextReporterPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Context Reporter".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: self.terminal_read.clone(),
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: false,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![CommandRegistration {
                id: "ctx.report".to_owned(),
                title: "Report Context".to_owned(),
                source_plugin: self.id.to_owned(),
            }]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };
            let path = invocation
                .context
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str())
                .unwrap_or("none");
            let block_id = invocation
                .context
                .selected_command_block
                .as_ref()
                .map(|block| block.id.to_string())
                .unwrap_or_else(|| "none".to_owned());
            let block_path = invocation
                .context
                .selected_command_block
                .as_ref()
                .and_then(|block| block.current_directory.as_ref())
                .map(|directory| directory.path.as_str())
                .unwrap_or("none");
            Ok(vec![PluginAction::ShowMessage {
                message: format!("{path} {block_id} {block_path}"),
            }])
        }
    }

    impl BuiltInPlugin for CommandReporterPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Command Reporter".to_owned(),
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

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![CommandRegistration {
                id: self.command_id.to_owned(),
                title: "Report Command".to_owned(),
                source_plugin: self.id.to_owned(),
            }]
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let PluginEvent::CommandInvoked(invocation) = event else {
                return Ok(Vec::new());
            };

            Ok(vec![PluginAction::ShowMessage {
                message: format!("{} saw {}", self.id, invocation.command_id),
            }])
        }
    }

    impl BuiltInPlugin for DynamicCommandPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Dynamic Command".to_owned(),
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

    impl BuiltInPlugin for EventReporterPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Event Reporter".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: self.terminal_read.clone(),
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: false,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
            let message = match event {
                PluginEvent::TerminalOutput { bytes } => format!("output {}", bytes.len()),
                PluginEvent::SelectionChanged { selection } => selection
                    .as_ref()
                    .map(|range| {
                        format!(
                            "selection {}:{}-{}:{}",
                            range.start.row, range.start.col, range.end.row, range.end.col
                        )
                    })
                    .unwrap_or_else(|| "selection none".to_owned()),
                _ => return Ok(Vec::new()),
            };

            Ok(vec![PluginAction::ShowMessage { message }])
        }
    }

    impl BuiltInPlugin for ProfilePickerPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Profile Picker".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::None,
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: self.profile_read,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![CommandRegistration {
                id: "profiles.pick".to_owned(),
                title: "Pick Profile".to_owned(),
                source_plugin: self.id.to_owned(),
            }]
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
                    reason: self.reason.map(str::to_owned),
                },
            )])
        }
    }

    impl BuiltInPlugin for ProfileLaunchPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Profile Launch".to_owned(),
                version: "0.1.0".to_owned(),
                runtime: PluginRuntime::BuiltIn,
                permissions: PluginPermissions {
                    terminal_read: TerminalReadPermission::None,
                    terminal_write: TerminalWritePermission::Deny,
                    profile_read: self.profile_read,
                    profile_write: false,
                    vault: VaultPermission::Deny,
                    network: NetworkPermission::Deny,
                },
            }
        }

        fn commands(&self) -> Vec<CommandRegistration> {
            vec![CommandRegistration {
                id: "profiles.launch".to_owned(),
                title: "Launch Profile".to_owned(),
                source_plugin: self.id.to_owned(),
            }]
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
                    profile_id: self.profile_id.to_owned(),
                    reason: self.reason.map(str::to_owned),
                },
            )])
        }
    }

    impl BuiltInPlugin for FailingPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.to_owned(),
                name: "Failing Plugin".to_owned(),
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

        fn commands(&self) -> Vec<CommandRegistration> {
            self.command_id
                .map(|command_id| CommandRegistration {
                    id: command_id.to_owned(),
                    title: "Failing Command".to_owned(),
                    source_plugin: self.id.to_owned(),
                })
                .into_iter()
                .collect()
        }

        fn handle_event(&mut self, _event: &PluginEvent) -> Result<Vec<PluginAction>> {
            anyhow::bail!("plugin {} failed while handling event", self.id)
        }
    }

    #[test]
    fn install_builtin_registers_manifest_and_commands() {
        let mut host = PluginHost::default();

        host.install_builtin(TestPlugin {
            id: "test",
            write_permission: TerminalWritePermission::Deny,
            write_on_start: false,
        })
        .unwrap();

        assert_eq!(host.manifests().count(), 1);
        assert_eq!(host.commands()[0].id, "test.hello");
    }

    #[test]
    fn terminal_write_requires_allow_session_permission() {
        let mut host = PluginHost::default();
        host.install_builtin(TestPlugin {
            id: "test",
            write_permission: TerminalWritePermission::Deny,
            write_on_start: true,
        })
        .unwrap();

        assert!(host.dispatch_event(&PluginEvent::AppStarted).is_err());
    }

    #[test]
    fn terminal_write_action_is_returned_when_allowed() {
        let mut host = PluginHost::default();
        host.install_builtin(TestPlugin {
            id: "test",
            write_permission: TerminalWritePermission::AllowSession,
            write_on_start: true,
        })
        .unwrap();

        let actions = host.dispatch_event(&PluginEvent::AppStarted).unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"echo plugin\n".to_vec()
            }]
        );
    }

    #[test]
    fn plugin_event_handler_errors_disable_plugin_and_continue_dispatch() {
        let mut host = PluginHost::default();
        host.install_builtin(FailingPlugin {
            id: "bad",
            command_id: None,
        })
        .unwrap();
        host.install_builtin(TestPlugin {
            id: "good",
            write_permission: TerminalWritePermission::AllowSession,
            write_on_start: true,
        })
        .unwrap();

        let actions = host.dispatch_event(&PluginEvent::AppStarted).unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"echo plugin\n".to_vec()
            }]
        );
        assert_eq!(host.disabled_plugin_ids(), &["bad".to_owned()]);
        assert_eq!(
            host.runtime_failures(),
            &[PluginRuntimeFailure {
                plugin_id: "bad".to_owned(),
                kind: PluginRuntimeFailureKind::EventHandlerError,
                message: "plugin bad failed while handling event".to_owned(),
            }]
        );

        let actions = host.dispatch_event(&PluginEvent::AppStarted).unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"echo plugin\n".to_vec()
            }]
        );
        assert_eq!(host.runtime_failures().len(), 1);
    }

    #[test]
    fn command_context_metadata_requires_terminal_read_permission() {
        let event =
            PluginEvent::CommandInvoked(command_invocation_with_cwd("/home/mingxu/project"));

        let mut denied = PluginHost::default();
        denied
            .install_builtin(ContextReporterPlugin {
                id: "ctx-denied",
                terminal_read: TerminalReadPermission::None,
            })
            .unwrap();
        assert_eq!(
            denied.dispatch_event(&event).unwrap(),
            vec![PluginAction::ShowMessage {
                message: "none none none".to_owned(),
            }]
        );

        let mut selection_only = PluginHost::default();
        selection_only
            .install_builtin(ContextReporterPlugin {
                id: "ctx-selection",
                terminal_read: TerminalReadPermission::SelectionOnly,
            })
            .unwrap();
        assert_eq!(
            selection_only.dispatch_event(&event).unwrap(),
            vec![PluginAction::ShowMessage {
                message: "none 42 /home/mingxu/project".to_owned(),
            }]
        );

        let mut allowed = PluginHost::default();
        allowed
            .install_builtin(ContextReporterPlugin {
                id: "ctx-allowed",
                terminal_read: TerminalReadPermission::CurrentScreen,
            })
            .unwrap();
        assert_eq!(
            allowed.dispatch_event(&event).unwrap(),
            vec![PluginAction::ShowMessage {
                message: "/home/mingxu/project 42 /home/mingxu/project".to_owned(),
            }]
        );
    }

    #[test]
    fn command_invocation_is_routed_to_registered_owner_only() {
        let mut host = PluginHost::default();
        host.install_builtin(CommandReporterPlugin {
            id: "owner",
            command_id: "owner.run",
        })
        .unwrap();
        host.install_builtin(CommandReporterPlugin {
            id: "observer",
            command_id: "observer.run",
        })
        .unwrap();

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "owner.run".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::ShowMessage {
                message: "owner saw owner.run".to_owned(),
            }]
        );
    }

    #[test]
    fn unknown_command_invocation_is_not_broadcast() {
        let mut host = PluginHost::default();
        host.install_builtin(CommandReporterPlugin {
            id: "observer",
            command_id: "observer.run",
        })
        .unwrap();

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "missing.run".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(actions, Vec::new());
    }

    #[test]
    fn disabled_command_owner_is_not_broadcast_or_retried() {
        let mut host = PluginHost::default();
        host.install_builtin(FailingPlugin {
            id: "bad",
            command_id: Some("bad.run"),
        })
        .unwrap();
        host.install_builtin(CommandReporterPlugin {
            id: "observer",
            command_id: "observer.run",
        })
        .unwrap();

        let invocation = PluginEvent::CommandInvoked(CommandInvocation {
            command_id: "bad.run".to_owned(),
            args: serde_json::Value::Null,
            context: CommandInvocationContext::default(),
        });

        assert_eq!(host.dispatch_event(&invocation).unwrap(), Vec::new());
        assert_eq!(host.disabled_plugin_ids(), &["bad".to_owned()]);
        assert_eq!(host.runtime_failures().len(), 1);

        assert_eq!(host.dispatch_event(&invocation).unwrap(), Vec::new());
        assert_eq!(host.runtime_failures().len(), 1);
    }

    #[test]
    fn runtime_registered_command_updates_owner_routing() {
        let mut host = PluginHost::default();
        host.install_builtin(DynamicCommandPlugin {
            id: "dynamic",
            command_id: "dynamic.run",
        })
        .unwrap();
        host.install_builtin(CommandReporterPlugin {
            id: "observer",
            command_id: "observer.run",
        })
        .unwrap();

        let registered = host.dispatch_event(&PluginEvent::AppStarted).unwrap();
        assert_eq!(
            registered,
            vec![PluginAction::RegisterCommand(CommandRegistration {
                id: "dynamic.run".to_owned(),
                title: "Dynamic Command".to_owned(),
                source_plugin: "dynamic".to_owned(),
            })]
        );
        assert!(host
            .commands()
            .iter()
            .any(|command| command.id == "dynamic.run" && command.source_plugin == "dynamic"));

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "dynamic.run".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::ShowMessage {
                message: "dynamic handled dynamic.run".to_owned(),
            }]
        );
    }

    #[test]
    fn runtime_registered_command_ids_must_be_unique() {
        let mut host = PluginHost::default();
        host.install_builtin(TestPlugin {
            id: "test",
            write_permission: TerminalWritePermission::Deny,
            write_on_start: false,
        })
        .unwrap();
        host.install_builtin(DynamicCommandPlugin {
            id: "dynamic",
            command_id: "test.hello",
        })
        .unwrap();

        assert!(host.dispatch_event(&PluginEvent::AppStarted).is_err());
        assert_eq!(
            host.commands()
                .iter()
                .filter(|command| command.id == "test.hello")
                .count(),
            1
        );
    }

    #[test]
    fn terminal_output_requires_screen_read_permission() {
        let event = PluginEvent::TerminalOutput {
            bytes: b"secret output".to_vec(),
        };

        let mut denied = PluginHost::default();
        denied
            .install_builtin(EventReporterPlugin {
                id: "output-denied",
                terminal_read: TerminalReadPermission::None,
            })
            .unwrap();
        assert_eq!(denied.dispatch_event(&event).unwrap(), Vec::new());

        let mut selection_only = PluginHost::default();
        selection_only
            .install_builtin(EventReporterPlugin {
                id: "output-selection",
                terminal_read: TerminalReadPermission::SelectionOnly,
            })
            .unwrap();
        assert_eq!(selection_only.dispatch_event(&event).unwrap(), Vec::new());

        let mut allowed = PluginHost::default();
        allowed
            .install_builtin(EventReporterPlugin {
                id: "output-allowed",
                terminal_read: TerminalReadPermission::CurrentScreen,
            })
            .unwrap();
        assert_eq!(
            allowed.dispatch_event(&event).unwrap(),
            vec![PluginAction::ShowMessage {
                message: "output 13".to_owned(),
            }]
        );
    }

    #[test]
    fn selection_changed_requires_selection_read_permission() {
        let event = PluginEvent::SelectionChanged {
            selection: Some(CellRange {
                start: CellPoint::new(1, 2),
                end: CellPoint::new(3, 4),
            }),
        };

        let mut denied = PluginHost::default();
        denied
            .install_builtin(EventReporterPlugin {
                id: "selection-denied",
                terminal_read: TerminalReadPermission::None,
            })
            .unwrap();
        assert_eq!(denied.dispatch_event(&event).unwrap(), Vec::new());

        let mut selection_only = PluginHost::default();
        selection_only
            .install_builtin(EventReporterPlugin {
                id: "selection-allowed",
                terminal_read: TerminalReadPermission::SelectionOnly,
            })
            .unwrap();
        assert_eq!(
            selection_only.dispatch_event(&event).unwrap(),
            vec![PluginAction::ShowMessage {
                message: "selection 1:2-3:4".to_owned(),
            }]
        );
    }

    #[test]
    fn redacted_profile_store_summary_maps_to_plugin_counts_only() {
        let summary = profile_store_summary_fixture();
        let plugin_summary = plugin_profile_store_summary_from_redacted(&summary).unwrap();

        assert_eq!(
            plugin_summary,
            PluginProfileStoreSummary {
                profile_count: 2,
                default_profile_configured: true,
                launchable_profiles: 1,
                credential_resolver_required_profiles: 1,
            }
        );

        let json = serde_json::to_string(&plugin_summary).unwrap();
        for forbidden in [
            "prod",
            "Production",
            "work",
            "vaulted",
            "Vaulted",
            "default_profile_id",
            "launchability",
        ] {
            assert!(
                !json.contains(forbidden),
                "plugin summary leaked {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn profile_picker_request_is_queued_with_source_plugin() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfilePickerPlugin {
            id: "profiles",
            profile_read: true,
            reason: Some("open from command"),
        })
        .unwrap();

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.pick".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::RequestProfilePicker(
                PluginProfilePickerRequest {
                    reason: Some("open from command".to_owned()),
                }
            )]
        );
        assert_eq!(
            host.profile_picker_requests(),
            &[PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("open from command".to_owned()),
            }]
        );
        assert_eq!(
            host.take_profile_picker_requests(),
            vec![PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("open from command".to_owned()),
            }]
        );
        assert!(host.profile_picker_requests().is_empty());
        assert!(host.take_profile_picker_requests().is_empty());
    }

    #[test]
    fn profile_picker_request_can_take_one_by_index() {
        let mut host = PluginHost {
            profile_picker_requests: vec![
                PendingProfilePickerRequest {
                    source_plugin: "profiles".to_owned(),
                    reason: Some("first".to_owned()),
                },
                PendingProfilePickerRequest {
                    source_plugin: "profiles".to_owned(),
                    reason: Some("second".to_owned()),
                },
                PendingProfilePickerRequest {
                    source_plugin: "profiles".to_owned(),
                    reason: Some("third".to_owned()),
                },
            ],
            ..PluginHost::default()
        };

        assert_eq!(
            host.take_profile_picker_request(1).unwrap(),
            PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("second".to_owned()),
            }
        );
        assert_eq!(
            host.profile_picker_requests(),
            &[
                PendingProfilePickerRequest {
                    source_plugin: "profiles".to_owned(),
                    reason: Some("first".to_owned()),
                },
                PendingProfilePickerRequest {
                    source_plugin: "profiles".to_owned(),
                    reason: Some("third".to_owned()),
                },
            ]
        );

        assert!(host.take_profile_picker_request(2).is_err());
        assert_eq!(host.profile_picker_requests().len(), 2);
    }

    #[test]
    fn profile_picker_request_requires_profile_read_permission() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfilePickerPlugin {
            id: "profiles",
            profile_read: false,
            reason: Some("open from command"),
        })
        .unwrap();

        assert!(host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.pick".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .is_err());
        assert!(host.profile_picker_requests().is_empty());
    }

    #[test]
    fn profile_picker_request_rejects_control_character_reason() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfilePickerPlugin {
            id: "profiles",
            profile_read: true,
            reason: Some("bad\nreason"),
        })
        .unwrap();

        assert!(host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.pick".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .is_err());
        assert!(host.profile_picker_requests().is_empty());
    }

    #[test]
    fn profile_picker_request_review_returns_redacted_host_summary() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vaulted.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vaulted-prod".to_owned(),
        };
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, vaulted])
        };
        let requests = vec![PendingProfilePickerRequest {
            source_plugin: "profiles".to_owned(),
            reason: Some("pick a profile".to_owned()),
        }];

        let reviews = review_profile_picker_requests(&store, &requests).unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(
            reviews[0],
            PendingProfilePickerRequestReview {
                source_plugin: "profiles".to_owned(),
                reason: Some("pick a profile".to_owned()),
                summary: store.redacted_summary().unwrap(),
            }
        );
        assert_eq!(
            reviews[0].summary.default_profile_id.as_deref(),
            Some("prod")
        );
        assert_eq!(reviews[0].summary.launchable_profiles, 1);
        assert_eq!(reviews[0].summary.credential_resolver_required_profiles, 1);
        assert_eq!(reviews[0].summary.profiles[0].id, "prod");
        assert_eq!(reviews[0].summary.profiles[0].name, "Production");
        assert_eq!(reviews[0].summary.profiles[0].tags, vec!["work"]);
    }

    #[test]
    fn profile_picker_request_review_rejects_invalid_pending_reason() {
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert!(review_profile_picker_requests(
            &store,
            &[PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("bad\nreason".to_owned()),
            }],
        )
        .is_err());
    }

    #[test]
    fn profile_picker_selection_resolution_returns_launchable_profile() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        let store = ProfileStoreV1::with_profiles(vec![prod.clone()]);
        let request = PendingProfilePickerRequest {
            source_plugin: "profiles".to_owned(),
            reason: Some("pick a profile".to_owned()),
        };

        assert_eq!(
            resolve_profile_picker_selection(&store, &request, "prod").unwrap(),
            ResolvedProfilePickerSelection {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("pick a profile".to_owned()),
                profile: prod,
            }
        );
    }

    #[test]
    fn profile_picker_selection_resolution_fails_closed() {
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vaulted.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vaulted-prod".to_owned(),
        };
        let store = ProfileStoreV1::with_profiles(vec![
            SshProfile::new("prod", "Production", "prod.example.com"),
            vaulted,
        ]);
        let request = PendingProfilePickerRequest {
            source_plugin: "profiles".to_owned(),
            reason: Some("pick a profile".to_owned()),
        };

        assert!(resolve_profile_picker_selection(&store, &request, "vaulted").is_err());
        assert!(resolve_profile_picker_selection(&store, &request, "missing").is_err());
        assert!(resolve_profile_picker_selection(&store, &request, "bad id").is_err());
        assert!(resolve_profile_picker_selection(
            &store,
            &PendingProfilePickerRequest {
                source_plugin: "profiles".to_owned(),
                reason: Some("bad\nreason".to_owned()),
            },
            "prod",
        )
        .is_err());
    }

    #[test]
    fn profile_picker_pty_config_resolution_does_not_spawn() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user = Some("deploy".to_owned());
        let store = ProfileStoreV1::with_profiles(vec![prod]);
        let request = PendingProfilePickerRequest {
            source_plugin: "profiles".to_owned(),
            reason: Some("pick a profile".to_owned()),
        };

        assert_eq!(
            resolve_profile_picker_pty_config(&store, &request, "prod", GridSize::new(40, 120))
                .unwrap(),
            ResolvedProfilePickerPtyConfig {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("pick a profile".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(40, 120), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "deploy@prod.example.com"]);
                    config
                },
            }
        );
    }

    #[test]
    fn profile_launch_request_is_queued_with_source_plugin() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfileLaunchPlugin {
            id: "profiles",
            profile_read: true,
            profile_id: "prod-db",
            reason: Some("launch from command"),
        })
        .unwrap();

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.launch".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::RequestProfileLaunch(
                PluginProfileLaunchRequest {
                    profile_id: "prod-db".to_owned(),
                    reason: Some("launch from command".to_owned()),
                }
            )]
        );
        assert_eq!(
            host.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod-db".to_owned(),
                reason: Some("launch from command".to_owned()),
            }]
        );
        assert_eq!(
            host.take_profile_launch_requests(),
            vec![PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "prod-db".to_owned(),
                reason: Some("launch from command".to_owned()),
            }]
        );
        assert!(host.profile_launch_requests().is_empty());
        assert!(host.take_profile_launch_requests().is_empty());
    }

    #[test]
    fn profile_launch_request_can_take_one_by_index() {
        let mut host = PluginHost {
            profile_launch_requests: vec![
                PendingProfileLaunchRequest {
                    source_plugin: "profiles".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("first".to_owned()),
                },
                PendingProfileLaunchRequest {
                    source_plugin: "profiles".to_owned(),
                    profile_id: "stage".to_owned(),
                    reason: Some("second".to_owned()),
                },
                PendingProfileLaunchRequest {
                    source_plugin: "profiles".to_owned(),
                    profile_id: "dev".to_owned(),
                    reason: Some("third".to_owned()),
                },
            ],
            ..PluginHost::default()
        };

        assert_eq!(
            host.take_profile_launch_request(1).unwrap(),
            PendingProfileLaunchRequest {
                source_plugin: "profiles".to_owned(),
                profile_id: "stage".to_owned(),
                reason: Some("second".to_owned()),
            }
        );
        assert_eq!(
            host.profile_launch_requests(),
            &[
                PendingProfileLaunchRequest {
                    source_plugin: "profiles".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("first".to_owned()),
                },
                PendingProfileLaunchRequest {
                    source_plugin: "profiles".to_owned(),
                    profile_id: "dev".to_owned(),
                    reason: Some("third".to_owned()),
                },
            ]
        );

        assert!(host.take_profile_launch_request(2).is_err());
        assert_eq!(host.profile_launch_requests().len(), 2);
    }

    #[test]
    fn profile_launch_request_requires_profile_read_permission() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfileLaunchPlugin {
            id: "profiles",
            profile_read: false,
            profile_id: "prod-db",
            reason: Some("launch from command"),
        })
        .unwrap();

        assert!(host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.launch".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .is_err());
        assert!(host.profile_launch_requests().is_empty());
    }

    #[test]
    fn profile_launch_request_rejects_unsafe_profile_id() {
        let mut host = PluginHost::default();
        host.install_builtin(ProfileLaunchPlugin {
            id: "profiles",
            profile_read: true,
            profile_id: "bad id",
            reason: Some("launch from command"),
        })
        .unwrap();

        assert!(host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "profiles.launch".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .is_err());
        assert!(host.profile_launch_requests().is_empty());
    }

    #[test]
    fn profile_launch_request_review_revalidates_store_without_targets() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vaulted.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vaulted-prod".to_owned(),
        };
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod, vaulted])
        };
        let requests = vec![
            PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("operator command".to_owned()),
            },
            PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "vaulted".to_owned(),
                reason: None,
            },
            PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "missing".to_owned(),
                reason: None,
            },
        ];

        assert_eq!(
            review_profile_launch_requests(&store, &requests).unwrap(),
            vec![
                PendingProfileLaunchRequestReview {
                    source_plugin: "launcher".to_owned(),
                    profile_id: "prod".to_owned(),
                    reason: Some("operator command".to_owned()),
                    status: PendingProfileLaunchRequestStatus::Launchable,
                    profile_name: Some("Production".to_owned()),
                    tags: vec!["work".to_owned()],
                    is_default: true,
                },
                PendingProfileLaunchRequestReview {
                    source_plugin: "launcher".to_owned(),
                    profile_id: "vaulted".to_owned(),
                    reason: None,
                    status: PendingProfileLaunchRequestStatus::RequiresCredentialResolver,
                    profile_name: Some("Vaulted".to_owned()),
                    tags: Vec::new(),
                    is_default: false,
                },
                PendingProfileLaunchRequestReview {
                    source_plugin: "launcher".to_owned(),
                    profile_id: "missing".to_owned(),
                    reason: None,
                    status: PendingProfileLaunchRequestStatus::NotFound,
                    profile_name: None,
                    tags: Vec::new(),
                    is_default: false,
                },
            ]
        );
    }

    #[test]
    fn profile_launch_request_review_rejects_invalid_pending_request() {
        let store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);

        assert!(review_profile_launch_requests(
            &store,
            &[PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "bad id".to_owned(),
                reason: None,
            }],
        )
        .is_err());
    }

    #[test]
    fn profile_launch_request_resolution_returns_launchable_profile() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        let store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![prod.clone()])
        };
        let request = PendingProfileLaunchRequest {
            source_plugin: "launcher".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("operator command".to_owned()),
        };

        assert_eq!(
            resolve_profile_launch_request(&store, &request).unwrap(),
            ResolvedProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("operator command".to_owned()),
                profile: prod,
            }
        );
    }

    #[test]
    fn profile_launch_request_resolution_fails_closed() {
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vaulted.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vaulted-prod".to_owned(),
        };
        let store = ProfileStoreV1::with_profiles(vec![
            SshProfile::new("prod", "Production", "prod.example.com"),
            vaulted,
        ]);

        assert!(resolve_profile_launch_request(
            &store,
            &PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "vaulted".to_owned(),
                reason: None,
            },
        )
        .is_err());
        assert!(resolve_profile_launch_request(
            &store,
            &PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "missing".to_owned(),
                reason: None,
            },
        )
        .is_err());
        assert!(resolve_profile_launch_request(
            &store,
            &PendingProfileLaunchRequest {
                source_plugin: "launcher".to_owned(),
                profile_id: "bad id".to_owned(),
                reason: None,
            },
        )
        .is_err());
    }

    #[test]
    fn profile_launch_request_pty_config_resolution_does_not_spawn() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target.user = Some("deploy".to_owned());
        prod.target.port = Some(2222);
        let store = ProfileStoreV1::with_profiles(vec![prod]);
        let request = PendingProfileLaunchRequest {
            source_plugin: "launcher".to_owned(),
            profile_id: "prod".to_owned(),
            reason: Some("operator command".to_owned()),
        };

        assert_eq!(
            resolve_profile_launch_pty_config(&store, &request, GridSize::new(30, 100)).unwrap(),
            ResolvedProfileLaunchPtyConfig {
                source_plugin: "launcher".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("operator command".to_owned()),
                config: {
                    let mut config = LocalPtyConfig::command(GridSize::new(30, 100), "ssh");
                    config.env("TERM", "xterm-256color");
                    config.args(["-tt", "-p", "2222", "deploy@prod.example.com"]);
                    config
                },
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn install_wasm_component_registers_manifest_commands_and_dispatches() {
        let mut host = PluginHost::default();
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();

        let commands = host
            .install_wasm_component_from_file(&runtime, component_path)
            .unwrap();

        assert_eq!(host.manifests().next().unwrap().id, "fixture");
        assert_eq!(
            host.manifests().next().unwrap().runtime,
            PluginRuntime::WasmComponent
        );
        assert_eq!(
            commands,
            vec![
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

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.echo".to_owned(),
                args: serde_json::json!({"value": "ok"}),
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: br#"echo fixture {"value":"ok"}
"#
                .to_vec()
            }]
        );

        let host_info_actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.host-info".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            host_info_actions,
            vec![PluginAction::WriteTerminal {
                bytes: format!(
                    "host Witty {} {}\n",
                    env!("CARGO_PKG_VERSION"),
                    witty_plugin_api::PLUGIN_ABI_VERSION
                )
                .into_bytes(),
            }]
        );

        let profile_summary_actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.profile-summary".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            profile_summary_actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"profiles none\n".to_vec(),
            }]
        );

        let profile_picker_actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.profile-picker".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
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
            host.profile_picker_requests(),
            &[PendingProfilePickerRequest {
                source_plugin: "fixture".to_owned(),
                reason: Some("fixture profile command".to_owned()),
            }]
        );

        let profile_launch_actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.profile-launch".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
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
            host.profile_launch_requests(),
            &[PendingProfileLaunchRequest {
                source_plugin: "fixture".to_owned(),
                profile_id: "prod".to_owned(),
                reason: Some("fixture launch command".to_owned()),
            }]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn install_wasm_component_with_profile_store_summary_exposes_counts_only() {
        let mut host = PluginHost::default();
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();

        host.install_wasm_component_from_file_with_profile_store_summary(
            &runtime,
            component_path,
            &profile_store_summary_fixture(),
        )
        .unwrap();

        let actions = host
            .dispatch_event(&PluginEvent::CommandInvoked(CommandInvocation {
                command_id: "fixture.profile-summary".to_owned(),
                args: serde_json::Value::Null,
                context: CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![PluginAction::WriteTerminal {
                bytes: b"profiles 2 default=true launchable=1 resolver=1\n".to_vec(),
            }]
        );
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

    fn command_invocation_with_cwd(path: &str) -> CommandInvocation {
        CommandInvocation {
            command_id: "ctx.report".to_owned(),
            args: serde_json::Value::Null,
            context: CommandInvocationContext {
                current_directory: Some(PluginCurrentDirectory {
                    uri: format!("file://localhost{path}"),
                    host: Some("localhost".to_owned()),
                    path: path.to_owned(),
                }),
                selected_command_block: Some(PluginCommandBlock {
                    id: 42,
                    command_range: PluginCommandBlockTextRange {
                        start: CellPoint::new(0, 2),
                        end_exclusive: CellPoint::new(0, 8),
                    },
                    output_range: Some(PluginCommandBlockTextRange {
                        start: CellPoint::new(0, 8),
                        end_exclusive: CellPoint::new(2, 0),
                    }),
                    exit_code: Some(7),
                    started_at_ms: Some(100),
                    finished_at_ms: Some(400),
                    duration_ms: Some(300),
                    current_directory: Some(PluginCurrentDirectory {
                        uri: format!("file://localhost{path}"),
                        host: Some("localhost".to_owned()),
                        path: path.to_owned(),
                    }),
                }),
                ..CommandInvocationContext::default()
            },
        }
    }
}
