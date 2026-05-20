use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Meeting {
    pub id: String,
    pub title: Option<String>,
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "audioPath")]
    pub audio_path: Option<String>,
    #[serde(rename = "participantsHint")]
    pub participants_hint: Option<String>,
    #[serde(rename = "processingProfile")]
    pub processing_profile: String,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingMetadata {
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_file_name: Option<String>,
    #[serde(default)]
    pub source_title: Option<String>,
    #[serde(default)]
    pub embedded_created_at: Option<String>,
    #[serde(default)]
    pub file_created_at: Option<String>,
    #[serde(default)]
    pub file_modified_at: Option<String>,
    #[serde(default)]
    pub recorded_at: Option<String>,
    #[serde(default)]
    pub recorded_at_source: Option<String>,
}
