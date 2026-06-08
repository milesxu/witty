//! Transport abstractions for local PTY, SSH processes, and browser gateways.

#[cfg(not(target_arch = "wasm32"))]
mod local_pty;
mod openssh;

use std::collections::VecDeque;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use witty_core::GridSize;

#[cfg(not(target_arch = "wasm32"))]
pub use local_pty::{LocalPtyConfig, LocalPtyTransport};
pub use openssh::{
    apply_openssh_import_preview, parse_openssh_import_preview, OpenSshAdvancedOptions,
    OpenSshImportApplyReport, OpenSshImportCandidate, OpenSshImportCandidateSummary,
    OpenSshImportConflict, OpenSshImportConflictPolicy, OpenSshImportPreview, OpenSshImportReview,
    OpenSshImportSelection, OpenSshImportSource, OpenSshImportWarning, OpenSshProfile,
    ProfileStoreDefaultPolicy, ProfileStoreMutation, ProfileStoreSummary, ProfileStoreV1,
    ProfileStoreValidation, ProfileSummary, SshCredentialRef, SshProfile, SshProfileLaunchability,
    SshProfileTarget, SshTerminalOptions, PROFILE_STORE_APP, PROFILE_STORE_MAX_JSON_BYTES,
    PROFILE_STORE_MAX_OPENSSH_EXTRA_ARGS, PROFILE_STORE_MAX_PROFILES,
    PROFILE_STORE_MAX_REMOTE_COMMAND_ARGS, PROFILE_STORE_MAX_TAGS_PER_PROFILE,
    PROFILE_STORE_SCHEMA_V1,
};
#[cfg(not(target_arch = "wasm32"))]
pub use openssh::{
    default_profile_store_path, edit_profile_store, read_profile_store,
    run_openssh_config_dump_smoke, write_profile_store_atomic, OpenSshConfigDumpSmoke,
    ProfileStoreEditOpenMode, ProfileStoreEditReport, ProfileStoreWriteReport,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportEvent {
    Output(Vec<u8>),
    Exit { code: Option<i32> },
    Error(String),
}

pub trait TerminalTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<()>;
    fn resize(&mut self, size: GridSize) -> Result<()>;
    fn poll_event(&mut self) -> Result<Option<TransportEvent>>;
}

pub const BROWSER_GATEWAY_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserGatewayClientMessage {
    Hello { protocol: u16 },
    Input { bytes: Vec<u8> },
    Resize { rows: u16, cols: u16 },
    Pong { id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserGatewayServerMessage {
    Ready { protocol: u16 },
    Output { bytes: Vec<u8> },
    Error { message: String },
    Exit { code: Option<i32> },
    Ping { id: String },
}

impl BrowserGatewayClientMessage {
    pub fn input(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Input {
            bytes: bytes.into(),
        }
    }

    pub fn resize(size: GridSize) -> Self {
        Self::Resize {
            rows: size.rows,
            cols: size.cols,
        }
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

impl BrowserGatewayServerMessage {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    pub fn into_transport_event(self) -> Option<TransportEvent> {
        match self {
            Self::Output { bytes } => Some(TransportEvent::Output(bytes)),
            Self::Error { message } => Some(TransportEvent::Error(message)),
            Self::Exit { code } => Some(TransportEvent::Exit { code }),
            Self::Ready { .. } | Self::Ping { .. } => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct MockTransport {
    written: Vec<u8>,
    events: VecDeque<TransportEvent>,
    size: GridSize,
}

impl MockTransport {
    pub fn new(size: GridSize) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    pub fn push_output(&mut self, output: impl Into<Vec<u8>>) {
        self.events.push_back(TransportEvent::Output(output.into()));
    }

    pub fn written(&self) -> &[u8] {
        &self.written
    }

    pub fn size(&self) -> GridSize {
        self.size
    }
}

impl TerminalTransport for MockTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.written.extend_from_slice(bytes);
        Ok(())
    }

    fn resize(&mut self, size: GridSize) -> Result<()> {
        self.size = size;
        Ok(())
    }

    fn poll_event(&mut self) -> Result<Option<TransportEvent>> {
        Ok(self.events.pop_front())
    }
}

#[derive(Debug, Default)]
pub struct BrowserGatewayTransport {
    outbound: Vec<u8>,
    events: VecDeque<TransportEvent>,
    size: GridSize,
}

impl BrowserGatewayTransport {
    pub fn new(size: GridSize) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    pub fn push_output(&mut self, output: impl Into<Vec<u8>>) {
        self.events.push_back(TransportEvent::Output(output.into()));
    }

    pub fn push_error(&mut self, message: impl Into<String>) {
        self.events.push_back(TransportEvent::Error(message.into()));
    }

    pub fn push_exit(&mut self, code: Option<i32>) {
        self.events.push_back(TransportEvent::Exit { code });
    }

    pub fn outbound(&self) -> &[u8] {
        &self.outbound
    }

    pub fn drain_outbound(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbound)
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    pub fn drain_outbound_message(&mut self) -> Option<BrowserGatewayClientMessage> {
        let bytes = self.drain_outbound();
        (!bytes.is_empty()).then(|| BrowserGatewayClientMessage::input(bytes))
    }

    pub fn resize_message(&self) -> BrowserGatewayClientMessage {
        BrowserGatewayClientMessage::resize(self.size)
    }

    pub fn push_server_message(&mut self, message: BrowserGatewayServerMessage) {
        if let Some(event) = message.into_transport_event() {
            self.events.push_back(event);
        }
    }
}

impl TerminalTransport for BrowserGatewayTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.outbound.extend_from_slice(bytes);
        Ok(())
    }

    fn resize(&mut self, size: GridSize) -> Result<()> {
        self.size = size;
        Ok(())
    }

    fn poll_event(&mut self) -> Result<Option<TransportEvent>> {
        Ok(self.events.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_transport_records_writes() {
        let mut transport = MockTransport::new(GridSize::new(24, 80));

        transport.write(b"ls\n").unwrap();
        transport.resize(GridSize::new(40, 120)).unwrap();

        assert_eq!(transport.written(), b"ls\n");
        assert_eq!(transport.size(), GridSize::new(40, 120));
    }

    #[test]
    fn browser_gateway_transport_separates_outbound_and_inbound() {
        let mut transport = BrowserGatewayTransport::new(GridSize::new(24, 80));

        transport.write(b"xy\r").unwrap();
        transport.push_output(b"ok".to_vec());
        transport.push_exit(Some(0));
        transport.resize(GridSize::new(40, 120)).unwrap();

        assert_eq!(transport.outbound(), b"xy\r");
        assert_eq!(transport.drain_outbound(), b"xy\r");
        assert!(transport.outbound().is_empty());
        assert_eq!(
            transport.poll_event().unwrap(),
            Some(TransportEvent::Output(b"ok".to_vec()))
        );
        assert_eq!(
            transport.poll_event().unwrap(),
            Some(TransportEvent::Exit { code: Some(0) })
        );
        assert_eq!(transport.poll_event().unwrap(), None);
        assert_eq!(transport.size(), GridSize::new(40, 120));
    }

    #[test]
    fn browser_gateway_client_messages_roundtrip_as_json() {
        let hello = BrowserGatewayClientMessage::Hello {
            protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
        };
        assert_eq!(
            BrowserGatewayClientMessage::from_json(&hello.to_json().unwrap()).unwrap(),
            hello
        );

        let input = BrowserGatewayClientMessage::input(b"xy\r".to_vec());
        assert_eq!(
            input.to_json().unwrap(),
            r#"{"type":"input","bytes":[120,121,13]}"#
        );
        assert_eq!(
            BrowserGatewayClientMessage::resize(GridSize::new(24, 80))
                .to_json()
                .unwrap(),
            r#"{"type":"resize","rows":24,"cols":80}"#
        );
    }

    #[test]
    fn browser_gateway_server_messages_map_to_transport_events() {
        let output = BrowserGatewayServerMessage::Output {
            bytes: b"ok".to_vec(),
        };
        assert_eq!(
            BrowserGatewayServerMessage::from_json(&output.to_json().unwrap())
                .unwrap()
                .into_transport_event(),
            Some(TransportEvent::Output(b"ok".to_vec()))
        );
        assert_eq!(
            BrowserGatewayServerMessage::Error {
                message: "failed".to_owned(),
            }
            .into_transport_event(),
            Some(TransportEvent::Error("failed".to_owned()))
        );
        assert_eq!(
            BrowserGatewayServerMessage::Exit { code: Some(7) }.into_transport_event(),
            Some(TransportEvent::Exit { code: Some(7) })
        );
        assert_eq!(
            BrowserGatewayServerMessage::Ready {
                protocol: BROWSER_GATEWAY_PROTOCOL_VERSION,
            }
            .into_transport_event(),
            None
        );
    }

    #[test]
    fn browser_gateway_transport_drains_protocol_messages() {
        let mut transport = BrowserGatewayTransport::new(GridSize::new(24, 80));

        assert_eq!(transport.drain_outbound_message(), None);
        transport.write(b"ls\r").unwrap();
        assert_eq!(
            transport.drain_outbound_message(),
            Some(BrowserGatewayClientMessage::Input {
                bytes: b"ls\r".to_vec(),
            })
        );
        assert_eq!(
            transport.resize_message(),
            BrowserGatewayClientMessage::Resize { rows: 24, cols: 80 }
        );

        transport.push_server_message(BrowserGatewayServerMessage::Output {
            bytes: b"remote".to_vec(),
        });
        assert_eq!(
            transport.poll_event().unwrap(),
            Some(TransportEvent::Output(b"remote".to_vec()))
        );
    }
}
