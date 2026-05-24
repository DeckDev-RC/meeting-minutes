use super::DbState;
use rusqlite::params;
use tauri::command;

#[command]
pub fn save_transcription(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    raw_whisper: String,
    diarized: String,
    speakers: String,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO transcriptions (id, meeting_id, raw_whisper, diarized, speakers, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, meeting_id, raw_whisper, diarized, speakers, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn get_transcription_by_meeting(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT id, meeting_id, raw_whisper, diarized, speakers, speaker_map, language, created_at
             FROM transcriptions
             WHERE meeting_id = ?1
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let result = stmt.query_row(params![meeting_id], |row| {
        let id: String = row.get(0)?;
        let mid: String = row.get(1)?;
        let raw_whisper: Option<String> = row.get(2)?;
        let diarized: Option<String> = row.get(3)?;
        let speakers: Option<String> = row.get(4)?;
        let speaker_map: Option<String> = row.get(5)?;
        let language: Option<String> = row.get(6)?;
        let created: String = row.get(7)?;
        Ok(serde_json::json!({
            "id": id,
            "meeting_id": mid,
            "raw_whisper": raw_whisper,
            "diarized": diarized,
            "speakers": speakers,
            "speaker_map": speaker_map,
            "language": language,
            "created_at": created,
        }))
    });

    match result {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[command]
pub fn save_speaker_map(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    speaker_map: String,
) -> Result<(), String> {
    let parsed = serde_json::from_str::<serde_json::Value>(&speaker_map)
        .map_err(|e| format!("invalid speaker_map JSON: {e}"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| "speaker_map must be a JSON object".to_string())?;
    if object
        .iter()
        .any(|(speaker, name)| speaker.trim().is_empty() || !name.is_string())
    {
        return Err("speaker_map must map speaker labels to names".to_string());
    }

    let db = state.0.lock().map_err(|e| e.to_string())?;
    let affected = db
        .execute(
            "UPDATE transcriptions
             SET speaker_map = ?1
             WHERE id = (
                SELECT id FROM transcriptions
                WHERE meeting_id = ?2
                ORDER BY created_at DESC
                LIMIT 1
             )",
            params![speaker_map, meeting_id],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err("no transcription found for meeting".to_string());
    }

    Ok(())
}
