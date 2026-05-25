use super::{processing_jobs::finalize_processing_jobs_for_meeting_record, DbState};
use crate::models::meeting::Meeting;
use rusqlite::params;
use tauri::command;

fn normalize_processing_profile(value: Option<&str>) -> &str {
    match value {
        Some("turbo") => "turbo",
        Some("precision") => "precision",
        _ => "balanced",
    }
}

fn normalize_transcription_profile(value: Option<&str>) -> Option<&str> {
    match value {
        Some("smart-low-cost") => Some("smart-low-cost"),
        Some("max-quality") => Some("max-quality"),
        Some("groq-turbo") => Some("groq-turbo"),
        Some("offline-free") => Some("offline-free"),
        Some("manual") => Some("manual"),
        _ => None,
    }
}

#[command]
pub fn save_meeting(
    state: tauri::State<'_, DbState>,
    meeting: serde_json::Value,
) -> Result<String, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let file_path = meeting["filePath"].as_str().unwrap_or("");
    let status = meeting["status"].as_str().unwrap_or("pending");
    let title = meeting["title"].as_str();
    let participants_hint = meeting["participantsHint"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let processing_profile = normalize_processing_profile(meeting["processingProfile"].as_str());
    let transcription_profile =
        normalize_transcription_profile(meeting["transcriptionProfile"].as_str());

    db.execute(
        "INSERT INTO meetings
            (id, title, file_path, participants_hint, processing_profile, transcription_profile, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            title,
            file_path,
            participants_hint,
            processing_profile,
            transcription_profile,
            status,
            now,
            now
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(id)
}

#[command]
pub fn get_meetings(state: tauri::State<'_, DbState>) -> Result<Vec<Meeting>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.prepare(
        "SELECT id, title, file_path, audio_path, participants_hint, processing_profile, transcription_profile, status, created_at, updated_at
         FROM meetings
         ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Meeting {
                id: row.get(0)?,
                title: row.get(1)?,
                file_path: row.get(2)?,
                audio_path: row.get(3)?,
                participants_hint: row.get(4)?,
                processing_profile: row.get(5)?,
                transcription_profile: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut meetings = Vec::new();
    for row in rows {
        meetings.push(row.map_err(|e| e.to_string())?);
    }
    Ok(meetings)
}

#[command]
pub fn update_meeting_status(
    state: tauri::State<'_, DbState>,
    id: String,
    status: String,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "UPDATE meetings SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, now, id],
    )
    .map_err(|e| e.to_string())?;
    finalize_processing_jobs_for_meeting_record(&db, &id, &status, &now)?;
    Ok(())
}
