//! Wasmtime Component Model runtime entry points for Witty plugins.

use std::path::Path;

use anyhow::{anyhow, Result};
use wasmtime::{
    component::{Component, Linker},
    Config, Engine, Store,
};
use witty_core::{CellPoint, CellRange};
use witty_plugin_api as api;

pub mod bindings {
    wasmtime::component::bindgen!({
        world: "terminal-plugin",
        path: "../witty-plugin-api/wit",
    });
}

use bindings::witty::plugin::types as wit;

pub type WasmHostInfo = api::PluginHostInfo;

#[derive(Clone)]
pub struct WasmPluginRuntime {
    engine: Engine,
}

impl WasmPluginRuntime {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config)?;

        Ok(Self { engine })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn compile_component(&self, bytes: impl AsRef<[u8]>) -> Result<Component> {
        Ok(Component::new(&self.engine, bytes)?)
    }

    pub fn component_from_file(&self, path: impl AsRef<Path>) -> Result<Component> {
        Ok(Component::from_file(&self.engine, path)?)
    }

    pub fn empty_linker(&self) -> Linker<WasmPluginState> {
        Linker::new(&self.engine)
    }

    pub fn host_linker(&self) -> Result<Linker<WasmPluginState>> {
        let mut linker = self.empty_linker();
        bindings::TerminalPlugin::add_to_linker::<
            WasmPluginState,
            wasmtime::component::HasSelf<WasmPluginState>,
        >(&mut linker, |state| state)?;

        Ok(linker)
    }

    pub fn new_store(&self, state: WasmPluginState) -> Store<WasmPluginState> {
        Store::new(&self.engine, state)
    }

    pub fn instantiate_component(
        &self,
        component: &Component,
        state: WasmPluginState,
    ) -> Result<WasmPluginInstance> {
        let linker = self.host_linker()?;

        self.instantiate_with_linker(component, state, &linker)
    }

    pub fn instantiate_with_linker(
        &self,
        component: &Component,
        state: WasmPluginState,
        linker: &Linker<WasmPluginState>,
    ) -> Result<WasmPluginInstance> {
        let mut store = self.new_store(state);
        let plugin = bindings::TerminalPlugin::instantiate(&mut store, component, linker)?;

        Ok(WasmPluginInstance { store, plugin })
    }
}

impl Default for WasmPluginRuntime {
    fn default() -> Self {
        Self::new().expect("default Wasmtime component runtime should be constructible")
    }
}

#[derive(Debug, Default)]
pub struct WasmPluginState {
    plugin_id: Option<String>,
    plugin_permissions: Option<api::PluginPermissions>,
    host_info: WasmHostInfo,
    profile_store_summary: Option<api::PluginProfileStoreSummary>,
}

impl WasmPluginState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_plugin_id(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: Some(plugin_id.into()),
            ..Self::default()
        }
    }

    pub fn with_host_info(host_info: WasmHostInfo) -> Self {
        Self {
            host_info,
            ..Self::default()
        }
    }

    pub fn with_profile_store_summary(summary: api::PluginProfileStoreSummary) -> Self {
        Self {
            profile_store_summary: Some(summary),
            ..Self::default()
        }
    }

    pub fn plugin_id(&self) -> Option<&str> {
        self.plugin_id.as_deref()
    }

    pub fn set_plugin_id(&mut self, plugin_id: impl Into<String>) {
        self.plugin_id = Some(plugin_id.into());
    }

    pub fn plugin_permissions(&self) -> Option<&api::PluginPermissions> {
        self.plugin_permissions.as_ref()
    }

    pub fn set_plugin_permissions(&mut self, permissions: api::PluginPermissions) {
        self.plugin_permissions = Some(permissions);
    }

    pub fn host_info(&self) -> &WasmHostInfo {
        &self.host_info
    }

    pub fn profile_store_summary_for_plugin(&self) -> Option<&api::PluginProfileStoreSummary> {
        let permissions = self.plugin_permissions.as_ref()?;
        if permissions.profile_read {
            self.profile_store_summary.as_ref()
        } else {
            None
        }
    }
}

impl bindings::witty::plugin::host::Host for WasmPluginState {
    fn get_host_info(&mut self) -> wit::HostInfo {
        wit::HostInfo {
            app_name: self.host_info.app_name.clone(),
            app_version: self.host_info.app_version.clone(),
            plugin_abi_version: self.host_info.plugin_abi_version.clone(),
        }
    }

    fn get_profile_store_summary(&mut self) -> Option<wit::ProfileStoreSummary> {
        self.profile_store_summary_for_plugin()
            .map(|summary| wit::ProfileStoreSummary {
                profile_count: summary.profile_count,
                default_profile_configured: summary.default_profile_configured,
                launchable_profiles: summary.launchable_profiles,
                credential_resolver_required_profiles: summary
                    .credential_resolver_required_profiles,
            })
    }
}

impl bindings::witty::plugin::types::Host for WasmPluginState {}

pub struct WasmPluginInstance {
    store: Store<WasmPluginState>,
    plugin: bindings::TerminalPlugin,
}

impl WasmPluginInstance {
    pub fn manifest(&mut self) -> Result<api::PluginManifest> {
        let manifest = guest_manifest_to_api(self.plugin.call_manifest(&mut self.store)?);
        let state = self.store.data_mut();
        state.set_plugin_id(manifest.id.clone());
        state.set_plugin_permissions(manifest.permissions.clone());

        Ok(manifest)
    }

    pub fn commands(&mut self) -> Result<Vec<api::CommandRegistration>> {
        let plugin_id = self.require_plugin_id()?;
        let commands = self.plugin.call_commands(&mut self.store)?;

        Ok(guest_commands_to_api(commands, &plugin_id))
    }

    pub fn handle_event(&mut self, event: api::PluginEvent) -> Result<Vec<api::PluginAction>> {
        let plugin_id = self.require_plugin_id()?;
        let guest_event = api_event_to_guest(event);
        let actions = self
            .plugin
            .call_handle_event(&mut self.store, &guest_event)?;

        Ok(guest_actions_to_api(actions, &plugin_id))
    }

    pub fn state(&self) -> &WasmPluginState {
        self.store.data()
    }

    fn require_plugin_id(&self) -> Result<String> {
        self.state()
            .plugin_id()
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("load plugin manifest before commands or events"))
    }
}

pub fn guest_manifest_to_api(manifest: bindings::PluginManifest) -> api::PluginManifest {
    api::PluginManifest {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        runtime: api::PluginRuntime::WasmComponent,
        permissions: guest_permissions_to_api(manifest.permissions),
    }
}

pub fn guest_command_to_api(
    command: bindings::CommandRegistration,
    source_plugin: impl Into<String>,
) -> api::CommandRegistration {
    api::CommandRegistration {
        id: command.id,
        title: command.title,
        source_plugin: source_plugin.into(),
    }
}

pub fn guest_commands_to_api(
    commands: Vec<bindings::CommandRegistration>,
    source_plugin: &str,
) -> Vec<api::CommandRegistration> {
    commands
        .into_iter()
        .map(|command| guest_command_to_api(command, source_plugin))
        .collect()
}

pub fn api_event_to_guest(event: api::PluginEvent) -> bindings::PluginEvent {
    match event {
        api::PluginEvent::AppStarted => bindings::PluginEvent::AppStarted,
        api::PluginEvent::CommandInvoked(invocation) => {
            bindings::PluginEvent::CommandInvoked(wit::CommandInvocation {
                command_id: invocation.command_id,
                args_json: invocation.args.to_string(),
                context: api_command_context_to_guest(invocation.context),
            })
        }
        api::PluginEvent::TerminalOutput { bytes } => bindings::PluginEvent::TerminalOutput(bytes),
        api::PluginEvent::SelectionChanged { selection } => {
            bindings::PluginEvent::SelectionChanged(selection.map(api_range_to_guest))
        }
    }
}

fn api_command_context_to_guest(
    context: api::CommandInvocationContext,
) -> wit::CommandInvocationContext {
    wit::CommandInvocationContext {
        current_directory: context
            .current_directory
            .map(|directory| wit::CurrentDirectory {
                uri: directory.uri,
                host: directory.host,
                path: directory.path,
            }),
        selected_command_block: context
            .selected_command_block
            .map(|block| wit::CommandBlock {
                id: block.id,
                command_range: api_command_block_text_range_to_guest(block.command_range),
                output_range: block
                    .output_range
                    .map(api_command_block_text_range_to_guest),
                exit_code: block.exit_code,
                started_at_ms: block.started_at_ms,
                finished_at_ms: block.finished_at_ms,
                duration_ms: block.duration_ms,
                current_directory: block
                    .current_directory
                    .map(|directory| wit::CurrentDirectory {
                        uri: directory.uri,
                        host: directory.host,
                        path: directory.path,
                    }),
            }),
    }
}

fn api_command_block_text_range_to_guest(
    range: api::PluginCommandBlockTextRange,
) -> wit::CommandBlockTextRange {
    wit::CommandBlockTextRange {
        start: wit::CellPoint {
            row: range.start.row,
            col: range.start.col,
        },
        end_exclusive: wit::CellPoint {
            row: range.end_exclusive.row,
            col: range.end_exclusive.col,
        },
    }
}

pub fn guest_action_to_api(
    action: bindings::PluginAction,
    source_plugin: &str,
) -> api::PluginAction {
    match action {
        bindings::PluginAction::RegisterCommand(command) => {
            api::PluginAction::RegisterCommand(guest_command_to_api(command, source_plugin))
        }
        bindings::PluginAction::WriteTerminal(bytes) => api::PluginAction::WriteTerminal { bytes },
        bindings::PluginAction::ShowMessage(message) => api::PluginAction::ShowMessage { message },
        bindings::PluginAction::RenderOverlay(overlay) => {
            api::PluginAction::RenderOverlay(api::RenderOverlay {
                range: guest_range_to_api(overlay.range),
                label: overlay.label,
            })
        }
        bindings::PluginAction::RequestProfilePicker(request) => {
            api::PluginAction::RequestProfilePicker(api::PluginProfilePickerRequest {
                reason: request.reason,
            })
        }
        bindings::PluginAction::RequestProfileLaunch(request) => {
            api::PluginAction::RequestProfileLaunch(api::PluginProfileLaunchRequest {
                profile_id: request.profile_id,
                reason: request.reason,
            })
        }
    }
}

pub fn guest_actions_to_api(
    actions: Vec<bindings::PluginAction>,
    source_plugin: &str,
) -> Vec<api::PluginAction> {
    actions
        .into_iter()
        .map(|action| guest_action_to_api(action, source_plugin))
        .collect()
}

fn guest_permissions_to_api(permissions: wit::PluginPermissions) -> api::PluginPermissions {
    api::PluginPermissions {
        terminal_read: match permissions.terminal_read {
            wit::TerminalReadPermission::None => api::TerminalReadPermission::None,
            wit::TerminalReadPermission::SelectionOnly => {
                api::TerminalReadPermission::SelectionOnly
            }
            wit::TerminalReadPermission::CurrentScreen => {
                api::TerminalReadPermission::CurrentScreen
            }
            wit::TerminalReadPermission::FullScrollback => {
                api::TerminalReadPermission::FullScrollback
            }
        },
        terminal_write: match permissions.terminal_write {
            wit::TerminalWritePermission::Deny => api::TerminalWritePermission::Deny,
            wit::TerminalWritePermission::Confirm => api::TerminalWritePermission::Confirm,
            wit::TerminalWritePermission::AllowSession => {
                api::TerminalWritePermission::AllowSession
            }
        },
        profile_read: permissions.profile_read,
        profile_write: permissions.profile_write,
        vault: match permissions.vault {
            wit::VaultPermission::Deny => api::VaultPermission::Deny,
            wit::VaultPermission::RequestOnly => api::VaultPermission::RequestOnly,
        },
        network: match permissions.network {
            wit::NetworkPermission::Deny => api::NetworkPermission::Deny,
            wit::NetworkPermission::AllowList(allow_list) => {
                api::NetworkPermission::AllowList(allow_list.hosts)
            }
        },
    }
}

fn api_range_to_guest(range: CellRange) -> wit::CellRange {
    wit::CellRange {
        start: wit::CellPoint {
            row: range.start.row,
            col: range.start.col,
        },
        end: wit::CellPoint {
            row: range.end.row,
            col: range.end.col,
        },
    }
}

fn guest_range_to_api(range: wit::CellRange) -> CellRange {
    CellRange {
        start: CellPoint {
            row: range.start.row,
            col: range.start.col,
        },
        end: CellPoint {
            row: range.end.row,
            col: range.end.col,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";
    const EMPTY_CORE_MODULE: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn generated_bindings_include_terminal_plugin_world() {
        let type_name = std::any::type_name::<bindings::TerminalPlugin>();

        assert!(type_name.contains("TerminalPlugin"));
    }

    #[test]
    fn runtime_compiles_component_bytes() {
        let runtime = WasmPluginRuntime::new().unwrap();

        runtime.compile_component(EMPTY_COMPONENT).unwrap();
    }

    #[test]
    fn fixture_component_exports_are_callable() {
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();
        let component = runtime.component_from_file(&component_path).unwrap();
        let mut plugin = runtime
            .instantiate_component(&component, WasmPluginState::new())
            .unwrap();

        let manifest = plugin.manifest().unwrap();

        assert_eq!(manifest.id, "fixture");
        assert_eq!(manifest.runtime, api::PluginRuntime::WasmComponent);
        assert_eq!(plugin.state().plugin_id(), Some("fixture"));

        let commands = plugin.commands().unwrap();

        assert_eq!(
            commands,
            vec![
                api::CommandRegistration {
                    id: "fixture.echo".to_owned(),
                    title: "Fixture Echo".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                api::CommandRegistration {
                    id: "fixture.host-info".to_owned(),
                    title: "Fixture Host Info".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                api::CommandRegistration {
                    id: "fixture.profile-summary".to_owned(),
                    title: "Fixture Profile Summary".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                api::CommandRegistration {
                    id: "fixture.profile-picker".to_owned(),
                    title: "Fixture Profile Picker".to_owned(),
                    source_plugin: "fixture".to_owned(),
                },
                api::CommandRegistration {
                    id: "fixture.profile-launch".to_owned(),
                    title: "Fixture Profile Launch".to_owned(),
                    source_plugin: "fixture".to_owned(),
                }
            ]
        );

        let actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.echo".to_owned(),
                args: serde_json::json!({"value": "ok"}),
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![api::PluginAction::WriteTerminal {
                bytes: br#"echo fixture {"value":"ok"}
"#
                .to_vec(),
            }]
        );

        let host_info_actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.host-info".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            host_info_actions,
            vec![api::PluginAction::WriteTerminal {
                bytes: format!(
                    "host Witty {} {}\n",
                    env!("CARGO_PKG_VERSION"),
                    api::PLUGIN_ABI_VERSION
                )
                .into_bytes(),
            }]
        );

        let profile_summary_actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.profile-summary".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            profile_summary_actions,
            vec![api::PluginAction::WriteTerminal {
                bytes: b"profiles none\n".to_vec(),
            }]
        );

        let profile_picker_actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.profile-picker".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            profile_picker_actions,
            vec![api::PluginAction::RequestProfilePicker(
                api::PluginProfilePickerRequest {
                    reason: Some("fixture profile command".to_owned()),
                }
            )]
        );

        let profile_launch_actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.profile-launch".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            profile_launch_actions,
            vec![api::PluginAction::RequestProfileLaunch(
                api::PluginProfileLaunchRequest {
                    profile_id: "prod".to_owned(),
                    reason: Some("fixture launch command".to_owned()),
                }
            )]
        );
    }

    #[test]
    fn fixture_can_read_custom_host_info_import() {
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();
        let component = runtime.component_from_file(&component_path).unwrap();
        let mut plugin = runtime
            .instantiate_component(
                &component,
                WasmPluginState::with_host_info(WasmHostInfo {
                    app_name: "CustomHost".to_owned(),
                    app_version: "9.8.7".to_owned(),
                    plugin_abi_version: "abi-test".to_owned(),
                }),
            )
            .unwrap();

        plugin.manifest().unwrap();
        let actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.host-info".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![api::PluginAction::WriteTerminal {
                bytes: b"host CustomHost 9.8.7 abi-test\n".to_vec(),
            }]
        );
    }

    #[test]
    fn fixture_can_read_permission_gated_profile_store_summary_import() {
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();
        let component = runtime.component_from_file(&component_path).unwrap();
        let mut plugin = runtime
            .instantiate_component(
                &component,
                WasmPluginState::with_profile_store_summary(api::PluginProfileStoreSummary {
                    profile_count: 3,
                    default_profile_configured: true,
                    launchable_profiles: 2,
                    credential_resolver_required_profiles: 1,
                }),
            )
            .unwrap();

        plugin.manifest().unwrap();
        let actions = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.profile-summary".to_owned(),
                args: serde_json::Value::Null,
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap();

        assert_eq!(
            actions,
            vec![api::PluginAction::WriteTerminal {
                bytes: b"profiles 3 default=true launchable=2 resolver=1\n".to_vec(),
            }]
        );
    }

    #[test]
    fn fixture_instance_requires_manifest_before_host_owned_fields() {
        let runtime = WasmPluginRuntime::new().unwrap();
        let component_path = build_fixture_component();
        let component = runtime.component_from_file(&component_path).unwrap();
        let mut plugin = runtime
            .instantiate_component(&component, WasmPluginState::new())
            .unwrap();

        let command_error = plugin.commands().unwrap_err().to_string();
        assert!(command_error.contains("load plugin manifest"));

        let action_error = plugin
            .handle_event(api::PluginEvent::CommandInvoked(api::CommandInvocation {
                command_id: "fixture.echo".to_owned(),
                args: serde_json::json!({"value": "ok"}),
                context: api::CommandInvocationContext::default(),
            }))
            .unwrap_err()
            .to_string();
        assert!(action_error.contains("load plugin manifest"));
    }

    #[test]
    fn runtime_rejects_core_wasm_module() {
        let runtime = WasmPluginRuntime::new().unwrap();

        assert!(runtime.compile_component(EMPTY_CORE_MODULE).is_err());
    }

    #[test]
    fn store_keeps_plugin_state() {
        let runtime = WasmPluginRuntime::new().unwrap();
        let store = runtime.new_store(WasmPluginState::with_plugin_id("demo"));

        assert_eq!(store.data().plugin_id(), Some("demo"));
        assert_eq!(store.data().plugin_permissions(), None);
        assert_eq!(store.data().host_info().app_name, "Witty");
    }

    #[test]
    fn profile_store_summary_import_requires_profile_read_permission() {
        let summary = api::PluginProfileStoreSummary {
            profile_count: 2,
            default_profile_configured: true,
            launchable_profiles: 1,
            credential_resolver_required_profiles: 1,
        };
        let mut state = WasmPluginState::with_profile_store_summary(summary);

        assert_eq!(state.profile_store_summary_for_plugin(), None);

        state.set_plugin_permissions(api::PluginPermissions::default());
        assert_eq!(state.profile_store_summary_for_plugin(), None);

        state.set_plugin_permissions(api::PluginPermissions {
            profile_read: true,
            ..api::PluginPermissions::default()
        });
        assert_eq!(
            state.profile_store_summary_for_plugin(),
            Some(&api::PluginProfileStoreSummary {
                profile_count: 2,
                default_profile_configured: true,
                launchable_profiles: 1,
                credential_resolver_required_profiles: 1,
            })
        );
    }

    #[test]
    fn guest_manifest_maps_to_wasm_component_runtime() {
        let manifest = guest_manifest_to_api(bindings::PluginManifest {
            id: "demo".to_owned(),
            name: "Demo".to_owned(),
            version: "0.1.0".to_owned(),
            permissions: guest_permissions(),
        });

        assert_eq!(manifest.id, "demo");
        assert_eq!(manifest.runtime, api::PluginRuntime::WasmComponent);
        assert_eq!(
            manifest.permissions.terminal_write,
            api::TerminalWritePermission::AllowSession
        );
        assert_eq!(
            manifest.permissions.network,
            api::NetworkPermission::AllowList(vec!["example.com".to_owned()])
        );
    }

    #[test]
    fn guest_commands_gain_source_plugin() {
        let command = guest_command_to_api(
            bindings::CommandRegistration {
                id: "demo.run".to_owned(),
                title: "Run".to_owned(),
            },
            "demo",
        );

        assert_eq!(command.source_plugin, "demo");
    }

    #[test]
    fn api_command_event_serializes_args_json() {
        let event = api_event_to_guest(api::PluginEvent::CommandInvoked(api::CommandInvocation {
            command_id: "demo.run".to_owned(),
            args: serde_json::json!({"count": 2}),
            context: api::CommandInvocationContext::default(),
        }));

        let bindings::PluginEvent::CommandInvoked(invocation) = event else {
            panic!("expected command event");
        };
        assert_eq!(invocation.command_id, "demo.run");
        assert_eq!(invocation.args_json, r#"{"count":2}"#);
        assert!(invocation.context.current_directory.is_none());
    }

    #[test]
    fn api_command_event_maps_current_directory_context() {
        let event = api_event_to_guest(api::PluginEvent::CommandInvoked(api::CommandInvocation {
            command_id: "demo.run".to_owned(),
            args: serde_json::Value::Null,
            context: api::CommandInvocationContext {
                current_directory: Some(api::PluginCurrentDirectory {
                    uri: "file://localhost/home/mingxu/demo".to_owned(),
                    host: Some("localhost".to_owned()),
                    path: "/home/mingxu/demo".to_owned(),
                }),
                ..api::CommandInvocationContext::default()
            },
        }));

        let bindings::PluginEvent::CommandInvoked(invocation) = event else {
            panic!("expected command event");
        };
        let directory = invocation
            .context
            .current_directory
            .expect("current directory should be forwarded");
        assert_eq!(directory.uri, "file://localhost/home/mingxu/demo");
        assert_eq!(directory.host.as_deref(), Some("localhost"));
        assert_eq!(directory.path, "/home/mingxu/demo");
    }

    #[test]
    fn api_command_event_maps_selected_command_block_context() {
        let event = api_event_to_guest(api::PluginEvent::CommandInvoked(api::CommandInvocation {
            command_id: "demo.run".to_owned(),
            args: serde_json::Value::Null,
            context: api::CommandInvocationContext {
                selected_command_block: Some(api::PluginCommandBlock {
                    id: 7,
                    command_range: api::PluginCommandBlockTextRange {
                        start: CellPoint { row: 0, col: 2 },
                        end_exclusive: CellPoint { row: 0, col: 8 },
                    },
                    output_range: Some(api::PluginCommandBlockTextRange {
                        start: CellPoint { row: 0, col: 8 },
                        end_exclusive: CellPoint { row: 2, col: 0 },
                    }),
                    exit_code: Some(2),
                    started_at_ms: Some(100),
                    finished_at_ms: Some(350),
                    duration_ms: Some(250),
                    current_directory: Some(api::PluginCurrentDirectory {
                        uri: "file://localhost/home/mingxu/block".to_owned(),
                        host: Some("localhost".to_owned()),
                        path: "/home/mingxu/block".to_owned(),
                    }),
                }),
                ..api::CommandInvocationContext::default()
            },
        }));

        let bindings::PluginEvent::CommandInvoked(invocation) = event else {
            panic!("expected command event");
        };
        let block = invocation
            .context
            .selected_command_block
            .expect("selected command block should be forwarded");
        assert_eq!(block.id, 7);
        assert_eq!(block.command_range.start.row, 0);
        assert_eq!(block.command_range.start.col, 2);
        assert_eq!(block.command_range.end_exclusive.row, 0);
        assert_eq!(block.command_range.end_exclusive.col, 8);
        let output_range = block
            .output_range
            .expect("output range should be forwarded");
        assert_eq!(output_range.start.row, 0);
        assert_eq!(output_range.start.col, 8);
        assert_eq!(output_range.end_exclusive.row, 2);
        assert_eq!(output_range.end_exclusive.col, 0);
        assert_eq!(block.exit_code, Some(2));
        assert_eq!(block.started_at_ms, Some(100));
        assert_eq!(block.finished_at_ms, Some(350));
        assert_eq!(block.duration_ms, Some(250));
        let directory = block
            .current_directory
            .expect("block current directory should be forwarded");
        assert_eq!(directory.path, "/home/mingxu/block");
    }

    #[test]
    fn guest_actions_gain_host_owned_fields() {
        let actions = guest_actions_to_api(
            vec![
                bindings::PluginAction::RegisterCommand(bindings::CommandRegistration {
                    id: "demo.run".to_owned(),
                    title: "Run".to_owned(),
                }),
                bindings::PluginAction::WriteTerminal(b"echo demo\n".to_vec()),
                bindings::PluginAction::RequestProfilePicker(wit::ProfilePickerRequest {
                    reason: Some("demo profile picker".to_owned()),
                }),
                bindings::PluginAction::RequestProfileLaunch(wit::ProfileLaunchRequest {
                    profile_id: "prod".to_owned(),
                    reason: Some("demo profile launch".to_owned()),
                }),
            ],
            "demo",
        );

        assert_eq!(
            actions[0],
            api::PluginAction::RegisterCommand(api::CommandRegistration {
                id: "demo.run".to_owned(),
                title: "Run".to_owned(),
                source_plugin: "demo".to_owned(),
            })
        );
        assert_eq!(
            actions[1],
            api::PluginAction::WriteTerminal {
                bytes: b"echo demo\n".to_vec()
            }
        );
        assert_eq!(
            actions[2],
            api::PluginAction::RequestProfilePicker(api::PluginProfilePickerRequest {
                reason: Some("demo profile picker".to_owned()),
            })
        );
        assert_eq!(
            actions[3],
            api::PluginAction::RequestProfileLaunch(api::PluginProfileLaunchRequest {
                profile_id: "prod".to_owned(),
                reason: Some("demo profile launch".to_owned()),
            })
        );
    }

    fn guest_permissions() -> wit::PluginPermissions {
        wit::PluginPermissions {
            terminal_read: wit::TerminalReadPermission::CurrentScreen,
            terminal_write: wit::TerminalWritePermission::AllowSession,
            profile_read: true,
            profile_write: false,
            vault: wit::VaultPermission::RequestOnly,
            network: wit::NetworkPermission::AllowList(wit::NetworkAllowList {
                hosts: vec!["example.com".to_owned()],
            }),
        }
    }

    fn build_fixture_component() -> std::path::PathBuf {
        let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
}
