use super::gemini::apply_gemini_thinking_config;
use crate::models::transcription::MeetingChunkInsights;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;
fn normalize_insights(
    mut insights: MeetingChunkInsights,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
) -> MeetingChunkInsights {
    insights.chunk_index = chunk_index;
    insights.start_sec = start_sec;
    insights.end_sec = end_sec;
    if insights.topics.is_empty() && !insights.topic_evidence.is_empty() {
        insights.topics = insights
            .topic_evidence
            .iter()
            .map(|topic| topic.title.trim())
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .collect();
    }
    insights
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    output.trim().to_string()
}

fn extract_segment_text(segments_json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(segments_json) else {
        return String::new();
    };
    let Some(items) = value.as_array() else {
        return String::new();
    };

    items
        .iter()
        .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn fallback_chunk_insights(
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: &str,
) -> MeetingChunkInsights {
    let text = extract_segment_text(segments_json);
    let summary = if text.is_empty() {
        "Trecho processado sem fatos estruturados extraidos.".to_string()
    } else {
        truncate_chars(&text, 420)
    };

    MeetingChunkInsights {
        chunk_index,
        start_sec,
        end_sec,
        summary,
        topics: vec!["Trecho da reuniao".to_string()],
        topic_evidence: Vec::new(),
        decisions: Vec::new(),
        actions: Vec::new(),
        questions: Vec::new(),
        risks: Vec::new(),
    }
}

pub(super) fn parse_chunk_insights_or_fallback(
    text: &str,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: &str,
) -> MeetingChunkInsights {
    serde_json::from_str::<MeetingChunkInsights>(text)
        .map(|insights| normalize_insights(insights, chunk_index, start_sec, end_sec))
        .unwrap_or_else(|_| fallback_chunk_insights(chunk_index, start_sec, end_sec, segments_json))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactBatchChunkInput {
    pub chunk_index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub segments_json: String,
}

#[derive(Debug, Deserialize)]
struct MeetingFactBatchResponse {
    #[serde(default)]
    chunks: Vec<MeetingChunkInsights>,
}

pub(super) fn parse_batch_insights_or_fallback(
    text: &str,
    chunks: &[FactBatchChunkInput],
) -> Vec<MeetingChunkInsights> {
    let parsed = serde_json::from_str::<MeetingFactBatchResponse>(text)
        .map(|response| response.chunks)
        .or_else(|_| serde_json::from_str::<Vec<MeetingChunkInsights>>(text));

    let mut parsed = match parsed {
        Ok(items) => items,
        Err(_) => Vec::new(),
    };
    let source_by_chunk = chunks
        .iter()
        .map(|chunk| (chunk.chunk_index, chunk))
        .collect::<HashMap<_, _>>();

    for item in &mut parsed {
        if let Some(source) = source_by_chunk.get(&item.chunk_index) {
            item.start_sec = source.start_sec;
            item.end_sec = source.end_sec;
        }
    }

    let parsed_by_chunk = parsed
        .into_iter()
        .map(|item| (item.chunk_index, item))
        .collect::<HashMap<_, _>>();

    chunks
        .iter()
        .map(|chunk| {
            parsed_by_chunk
                .get(&chunk.chunk_index)
                .cloned()
                .unwrap_or_else(|| {
                    fallback_chunk_insights(
                        chunk.chunk_index,
                        chunk.start_sec,
                        chunk.end_sec,
                        &chunk.segments_json,
                    )
                })
        })
        .collect()
}

fn chunk_fact_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "chunkIndex": { "type": "integer" },
            "startSec": { "type": "number" },
            "endSec": { "type": "number" },
            "summary": { "type": "string" },
            "topics": { "type": "array", "items": { "type": "string" } },
            "topicEvidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "timestampSec": { "type": "number" },
                        "evidence": { "type": "string" }
                    },
                    "required": ["title", "timestampSec", "evidence"],
                    "additionalProperties": false
                }
            },
            "decisions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "owner": { "type": "string" },
                        "timestampSec": { "type": "number" },
                        "evidence": { "type": "string" }
                    },
                    "required": ["title", "owner", "timestampSec", "evidence"],
                    "additionalProperties": false
                }
            },
            "actions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "task": { "type": "string" },
                        "owner": { "type": "string" },
                        "deadline": { "type": "string" },
                        "timestampSec": { "type": "number" },
                        "evidence": { "type": "string" }
                    },
                    "required": ["task", "owner", "deadline", "timestampSec", "evidence"],
                    "additionalProperties": false
                }
            },
            "questions": { "type": "array", "items": { "type": "string" } },
            "risks": { "type": "array", "items": { "type": "string" } }
        },
        "required": [
            "chunkIndex",
            "startSec",
            "endSec",
            "summary",
            "topics",
            "topicEvidence",
            "decisions",
            "actions",
            "questions",
            "risks"
        ],
        "additionalProperties": false
    })
}

pub(super) fn gemini_chunk_facts_generation_config() -> serde_json::Value {
    apply_gemini_thinking_config(serde_json::json!({
        "temperature": 0.1,
        "maxOutputTokens": 8192,
        "responseMimeType": "application/json",
        "responseJsonSchema": {
            "type": "object",
            "properties": {
                "chunks": {
                    "type": "array",
                    "items": chunk_fact_schema()
                }
            },
            "required": ["chunks"],
            "additionalProperties": false
        }
    }))
}

pub(super) fn gemini_single_chunk_generation_config() -> serde_json::Value {
    apply_gemini_thinking_config(serde_json::json!({
        "temperature": 0.1,
        "maxOutputTokens": 8192,
        "responseMimeType": "application/json",
        "responseJsonSchema": chunk_fact_schema()
    }))
}

fn json_array_or_empty(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed
    } else {
        "[]"
    }
}

pub(super) fn fact_batch_prompt_payload(chunks: &[FactBatchChunkInput]) -> String {
    let mut payload = String::with_capacity(chunks.len().saturating_mul(512));
    payload.push('[');
    for (index, chunk) in chunks.iter().enumerate() {
        if index > 0 {
            payload.push(',');
        }
        let _ = write!(
            payload,
            "{{\"chunkIndex\":{},\"startSec\":{},\"endSec\":{},\"segments\":{}}}",
            chunk.chunk_index,
            chunk.start_sec,
            chunk.end_sec,
            json_array_or_empty(&chunk.segments_json)
        );
    }
    payload.push(']');
    payload
}
