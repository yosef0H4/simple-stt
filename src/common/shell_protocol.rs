use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SHELL_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        protocol: u32,
        token: String,
    },
    Command {
        request_id: u64,
        command: ShellCommand,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    HelloAck {
        protocol: u32,
        service_pid: u32,
    },
    Response {
        request_id: u64,
        response: ShellResponse,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum ShellCommand {
    Ping,
    StartRecording { session_id: u64 },
    StopRecording { session_id: u64 },
    PollEvents { after_seq: u64 },
    ReloadConfig,
    UnloadModel,
    TestModel,
    DownloadModel { filename: String },
    ListInputs,
    ListModels,
    RefreshModels,
    ShowNotice { level: NoticeLevel, text: String },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShellResponse {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub events: Vec<ServiceEvent>,
}

impl ShellResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            values: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            values: BTreeMap::new(),
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    #[default]
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceEvent {
    pub seq: u64,
    pub kind: String,
    #[serde(default)]
    pub session_id: Option<u64>,
    #[serde(default)]
    pub level: NoticeLevel,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

impl ServiceEvent {
    pub fn simple(kind: impl Into<String>) -> Self {
        Self {
            seq: 0,
            kind: kind.into(),
            session_id: None,
            level: NoticeLevel::Info,
            text: String::new(),
            values: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trips_unicode_transcript() {
        let event = ServiceEvent {
            seq: 7,
            kind: "transcript".into(),
            session_id: Some(42),
            level: NoticeLevel::Info,
            text: "مرحبا 世界 🙂".into(),
            values: BTreeMap::new(),
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: ServiceEvent = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(serde_json::from_str::<ClientMessage>("{not json}").is_err());
    }
}
