//! Versioned wire schema shared by the Rust engine and the Java adapter.
//! Transport is length-prefixed UTF-8 JSON; do not send unbounded raw assets here.
use serde::{Deserialize, Serialize};
use vsmedia_api::{AssetKey, AssetKind, AssetRecord, API_VERSION};

pub const WIRE_VERSION: u32 = API_VERSION;
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_INLINE_BLOB_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRequest {
    pub version: u32,
    pub request_id: String,
    pub producer: String,
    pub token: String,
    pub command: WireCommand,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireCommand {
    Ping,
    Publish {
        key: AssetKey,
        kind: AssetKind,
        content_base64: String,
    },
    PublishNative { key: AssetKey, extension: String, content_base64: String },
    ReadNative { key: AssetKey, extension: String },
    Lookup { key: AssetKey },
    Read { key: AssetKey },
    BeginRead { key: AssetKey },
    ReadChunk { transfer_id: String, offset: u64 },
    EndRead { transfer_id: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireResponse {
    pub version: u32,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<WireResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireResult {
    Pong,
    Acknowledged,
    Download { transfer_id: String, size: u64, sha256: String },
    Chunk { offset: u64, content_base64: String, done: bool },
    Asset { record: AssetRecord },
    Content { content_base64: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireError {
    pub code: String,
    pub message: String,
}
impl WireResponse {
    pub fn ok(request_id: String, result: WireResult) -> Self {
        Self { version: WIRE_VERSION, request_id, result: Some(result), error: None }
    }
    pub fn failure(request_id: String, code: &str, message: &str) -> Self {
        Self {
            version: WIRE_VERSION, request_id, result: None,
            error: Some(WireError { code: code.into(), message: message.into() })
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_round_trip_is_stable() {
        let original = WireRequest {
            version: WIRE_VERSION,
            request_id: "abc".into(),
            producer: "vsrender".into(),
            token: "secret".into(),
            command: WireCommand::Lookup { key: AssetKey::new("demo", "hello").unwrap() },
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"operation\":\"lookup\""));
        let restored: WireRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.request_id, "abc");
    }
    #[test]
    fn unknown_operations_are_rejected() {
        let json = r#"{"version":1,"request_id":"x","producer":"test","token":"t","command":{"operation":"execute_shell"}}"#;
        assert!(serde_json::from_str::<WireRequest>(json).is_err());
    }
}
