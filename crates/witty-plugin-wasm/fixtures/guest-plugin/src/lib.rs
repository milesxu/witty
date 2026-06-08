#![no_std]

extern crate alloc;

use alloc::{borrow::ToOwned, format, vec, vec::Vec};

#[global_allocator]
static ALLOCATOR: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, len: usize) -> i32 {
    for index in 0..len {
        let left_byte = *left.add(index);
        let right_byte = *right.add(index);
        if left_byte != right_byte {
            return left_byte as i32 - right_byte as i32;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn cabi_realloc(
    old_ptr: *mut u8,
    old_len: usize,
    align: usize,
    new_len: usize,
) -> *mut u8 {
    use alloc::alloc::{alloc, handle_alloc_error, realloc, Layout};

    let layout;
    let ptr = if old_len == 0 {
        if new_len == 0 {
            return align as *mut u8;
        }
        layout = Layout::from_size_align_unchecked(new_len, align);
        alloc(layout)
    } else {
        layout = Layout::from_size_align_unchecked(old_len, align);
        realloc(old_ptr, layout, new_len)
    };

    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    ptr
}

wit_bindgen::generate!({
    world: "terminal-plugin",
    path: "../../../witty-plugin-api/wit",
});

use crate::witty::plugin::types::{
    NetworkPermission, PluginPermissions, ProfileLaunchRequest, ProfilePickerRequest,
    TerminalReadPermission, TerminalWritePermission, VaultPermission,
};
use crate::witty::plugin::host::{get_host_info, get_profile_store_summary};

struct FixturePlugin;

impl Guest for FixturePlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "fixture".to_owned(),
            name: "Fixture Plugin".to_owned(),
            version: "0.1.0".to_owned(),
            permissions: PluginPermissions {
                terminal_read: TerminalReadPermission::CurrentScreen,
                terminal_write: TerminalWritePermission::AllowSession,
                profile_read: true,
                profile_write: false,
                vault: VaultPermission::Deny,
                network: NetworkPermission::Deny,
            },
        }
    }

    fn commands() -> Vec<CommandRegistration> {
        vec![
            CommandRegistration {
                id: "fixture.echo".to_owned(),
                title: "Fixture Echo".to_owned(),
            },
            CommandRegistration {
                id: "fixture.host-info".to_owned(),
                title: "Fixture Host Info".to_owned(),
            },
            CommandRegistration {
                id: "fixture.profile-summary".to_owned(),
                title: "Fixture Profile Summary".to_owned(),
            },
            CommandRegistration {
                id: "fixture.profile-picker".to_owned(),
                title: "Fixture Profile Picker".to_owned(),
            },
            CommandRegistration {
                id: "fixture.profile-launch".to_owned(),
                title: "Fixture Profile Launch".to_owned(),
            },
        ]
    }

    fn handle_event(event: PluginEvent) -> Vec<PluginAction> {
        match event {
            PluginEvent::CommandInvoked(invocation) if invocation.command_id == "fixture.echo" => {
                vec![PluginAction::WriteTerminal(
                    format!("echo fixture {}\n", invocation.args_json).into_bytes(),
                )]
            }
            PluginEvent::CommandInvoked(invocation)
                if invocation.command_id == "fixture.host-info" =>
            {
                let host = get_host_info();
                vec![PluginAction::WriteTerminal(
                    format!(
                        "host {} {} {}\n",
                        host.app_name, host.app_version, host.plugin_abi_version
                    )
                    .into_bytes(),
                )]
            }
            PluginEvent::CommandInvoked(invocation)
                if invocation.command_id == "fixture.profile-summary" =>
            {
                let message = match get_profile_store_summary() {
                    Some(summary) => format!(
                        "profiles {} default={} launchable={} resolver={}\n",
                        summary.profile_count,
                        summary.default_profile_configured,
                        summary.launchable_profiles,
                        summary.credential_resolver_required_profiles
                    ),
                    None => "profiles none\n".to_owned(),
                };
                vec![PluginAction::WriteTerminal(message.into_bytes())]
            }
            PluginEvent::CommandInvoked(invocation)
                if invocation.command_id == "fixture.profile-picker" =>
            {
                vec![PluginAction::RequestProfilePicker(ProfilePickerRequest {
                    reason: Some("fixture profile command".to_owned()),
                })]
            }
            PluginEvent::CommandInvoked(invocation)
                if invocation.command_id == "fixture.profile-launch" =>
            {
                vec![PluginAction::RequestProfileLaunch(ProfileLaunchRequest {
                    profile_id: "prod".to_owned(),
                    reason: Some("fixture launch command".to_owned()),
                })]
            }
            _ => Vec::new(),
        }
    }
}

export!(FixturePlugin);
