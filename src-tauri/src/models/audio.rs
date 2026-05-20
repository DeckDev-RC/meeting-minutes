use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SilenceRange {
    pub start_sec: f64,
    pub end_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkPlan {
    pub index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedChunk {
    pub index: usize,
    pub audio_path: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
    pub duration_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SmartChunkOptions {
    pub target_sec: f64,
    pub min_sec: f64,
    pub max_sec: f64,
    pub overlap_sec: f64,
    pub silence_min_duration_sec: f64,
    pub silence_noise_db: f64,
    pub output_format: String,
}

impl Default for SmartChunkOptions {
    fn default() -> Self {
        Self {
            target_sec: 360.0,
            min_sec: 180.0,
            max_sec: 480.0,
            overlap_sec: 3.0,
            silence_min_duration_sec: 0.45,
            silence_noise_db: -35.0,
            output_format: "flac".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingChunkRecord {
    pub meeting_id: String,
    pub index: usize,
    pub audio_path: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
    pub duration_sec: f64,
    pub status: String,
    pub raw_segments_json: Option<String>,
    pub error_msg: Option<String>,
    pub facts_status: String,
    pub facts_json: Option<String>,
    pub facts_error_msg: Option<String>,
}
