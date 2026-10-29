//! Contratos para el futuro puente con VSRender/Typewriter u otros plugins.
//! Ningún endpoint se simula ni se contacta hasta configurar un servidor real.
#![allow(dead_code)]
use serde::{Serialize, Deserialize};

pub const API_VERSION: &str = "v1";
pub const HEALTH: &str = "/api/v1/health";
pub const CAPABILITIES: &str = "/api/v1/capabilities";
pub const MEDIA_JOBS: &str = "/api/v1/media/jobs";
pub const RADIO_SESSIONS: &str = "/api/v1/radio/sessions";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub api_version: String,
    pub audio_process: bool,
    pub radio_broadcast: bool,
    pub live_monitoring: bool,
    pub accepted_formats: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessRequest {
    pub asset_id: String,
    pub operation: String,
    pub arguments: serde_json::Value,
    pub request_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobState {
    pub id: String,
    pub status: String,
    pub progress: f32,
    pub result_url: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RadioSessionRequest {
    pub playlist_id: String,
    pub destination_id: String,
    pub codec: String,
}
