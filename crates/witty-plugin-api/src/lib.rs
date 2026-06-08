//! Stable plugin-facing data contracts.

use serde::{Deserialize, Serialize};
use witty_core::{CellPoint, CellRange, TerminalCurrentDirectory};

pub const PLUGIN_WIT: &str = include_str!("../wit/witty-plugin.wit");
pub const PLUGIN_ABI_VERSION: &str = "0.1.0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime: PluginRuntime,
    pub permissions: PluginPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PluginRuntime {
    WasmComponent,
    Extism,
    BuiltIn,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub terminal_read: TerminalReadPermission,
    pub terminal_write: TerminalWritePermission,
    pub profile_read: bool,
    pub profile_write: bool,
    pub vault: VaultPermission,
    pub network: NetworkPermission,
}

impl Default for PluginPermissions {
    fn default() -> Self {
        Self {
            terminal_read: TerminalReadPermission::None,
            terminal_write: TerminalWritePermission::Deny,
            profile_read: false,
            profile_write: false,
            vault: VaultPermission::Deny,
            network: NetworkPermission::Deny,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TerminalReadPermission {
    None,
    SelectionOnly,
    CurrentScreen,
    FullScrollback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TerminalWritePermission {
    Deny,
    Confirm,
    AllowSession,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VaultPermission {
    Deny,
    RequestOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NetworkPermission {
    Deny,
    AllowList(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandRegistration {
    pub id: String,
    pub title: String,
    pub source_plugin: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginHostInfo {
    pub app_name: String,
    pub app_version: String,
    pub plugin_abi_version: String,
}

impl Default for PluginHostInfo {
    fn default() -> Self {
        Self {
            app_name: "Witty".to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            plugin_abi_version: PLUGIN_ABI_VERSION.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginProfileStoreSummary {
    pub profile_count: u32,
    pub default_profile_configured: bool,
    pub launchable_profiles: u32,
    pub credential_resolver_required_profiles: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandInvocation {
    pub command_id: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub context: CommandInvocationContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandInvocationContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_directory: Option<PluginCurrentDirectory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_command_block: Option<PluginCommandBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginCurrentDirectory {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub path: String,
}

impl From<&TerminalCurrentDirectory> for PluginCurrentDirectory {
    fn from(directory: &TerminalCurrentDirectory) -> Self {
        Self {
            uri: directory.uri.clone(),
            host: directory.host.clone(),
            path: directory.path.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginCommandBlock {
    pub id: u64,
    pub command_range: PluginCommandBlockTextRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_range: Option<PluginCommandBlockTextRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_directory: Option<PluginCurrentDirectory>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginCommandBlockTextRange {
    pub start: CellPoint,
    pub end_exclusive: CellPoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PluginEvent {
    AppStarted,
    CommandInvoked(CommandInvocation),
    TerminalOutput { bytes: Vec<u8> },
    SelectionChanged { selection: Option<CellRange> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PluginAction {
    RegisterCommand(CommandRegistration),
    WriteTerminal { bytes: Vec<u8> },
    ShowMessage { message: String },
    RenderOverlay(RenderOverlay),
    RequestProfilePicker(PluginProfilePickerRequest),
    RequestProfileLaunch(PluginProfileLaunchRequest),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderOverlay {
    pub range: CellRange,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginProfilePickerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginProfileLaunchRequest {
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_permissions_are_closed() {
        let permissions = PluginPermissions::default();

        assert_eq!(permissions.terminal_read, TerminalReadPermission::None);
        assert_eq!(permissions.terminal_write, TerminalWritePermission::Deny);
        assert_eq!(permissions.vault, VaultPermission::Deny);
    }

    #[test]
    fn bundled_wit_defines_terminal_plugin_world() {
        let mut resolve = wit_parser::Resolve::default();
        let package_id = resolve
            .push_source("witty-plugin.wit", PLUGIN_WIT)
            .expect("bundled plugin WIT should parse");
        let package = &resolve.packages[package_id];
        let world_id = package
            .worlds
            .get("terminal-plugin")
            .expect("plugin WIT should define terminal-plugin world");
        let world = &resolve.worlds[*world_id];

        let has_export = |export_name: &str| {
            world
                .exports
                .keys()
                .any(|key| matches!(key, wit_parser::WorldKey::Name(name) if name == export_name))
        };

        assert!(has_export("manifest"));
        assert!(has_export("commands"));
        assert!(has_export("handle-event"));

        let has_import = |import_name: &str| {
            world.imports.values().any(|item| {
                let wit_parser::WorldItem::Interface { id, .. } = item else {
                    return false;
                };
                resolve.interfaces[*id].name.as_deref() == Some(import_name)
            })
        };
        assert!(has_import("host"));
    }

    #[test]
    fn command_invocation_deserializes_without_context() {
        let invocation: CommandInvocation =
            serde_json::from_str(r#"{"command_id":"demo.run","args":null}"#).unwrap();

        assert_eq!(invocation.command_id, "demo.run");
        assert_eq!(invocation.context, CommandInvocationContext::default());
    }
}
