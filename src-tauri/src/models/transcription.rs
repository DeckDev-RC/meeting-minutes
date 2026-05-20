use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptionSegment {
    pub id: i32,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiarizedSegment {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiarizedResult {
    pub speakers: Vec<String>,
    pub segments: Vec<DiarizedSegment>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDecision {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub timestamp_sec: f64,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingAction {
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub timestamp_sec: f64,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingChunkInsights {
    pub chunk_index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<MeetingDecision>,
    #[serde(default)]
    pub actions: Vec<MeetingAction>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}
