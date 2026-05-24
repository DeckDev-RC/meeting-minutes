use super::DbState;
use crate::models::audio::{ExportedChunk, ProcessingChunkRecord};
use rusqlite::params;
use tauri::command;

#[command]
pub fn save_processing_chunks(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    chunks: Vec<ExportedChunk>,
) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    let existing_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM processing_chunks WHERE meeting_id = ?1",
            params![&meeting_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if existing_count > 0 {
        return Err(format!(
            "processing chunks already exist for meeting_id {meeting_id}; refusing to overwrite resume state"
        ));
    }

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO processing_chunks
                (meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
            )
            .map_err(|e| e.to_string())?;

        for chunk in chunks {
            stmt.execute(params![
                &meeting_id,
                i64::try_from(chunk.index).map_err(|_| "chunk index is too large".to_string())?,
                chunk.audio_path,
                chunk.start_sec,
                chunk.end_sec,
                chunk.offset_sec,
                chunk.duration_sec,
                &now,
                &now
            ])
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub fn get_processing_chunks(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Vec<ProcessingChunkRecord>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, error_msg, facts_status, facts_json, facts_error_msg
             FROM processing_chunks
             WHERE meeting_id = ?1
             ORDER BY index_no ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut chunks = Vec::new();
    let mut rows = stmt.query(params![meeting_id]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let index_no: i64 = row.get(1).map_err(|e| e.to_string())?;
        let index = usize::try_from(index_no)
            .map_err(|_| format!("invalid processing chunk index_no: {index_no}"))?;

        chunks.push(ProcessingChunkRecord {
            meeting_id: row.get(0).map_err(|e| e.to_string())?,
            index,
            audio_path: row.get(2).map_err(|e| e.to_string())?,
            start_sec: row.get(3).map_err(|e| e.to_string())?,
            end_sec: row.get(4).map_err(|e| e.to_string())?,
            offset_sec: row.get(5).map_err(|e| e.to_string())?,
            duration_sec: row.get(6).map_err(|e| e.to_string())?,
            status: row.get(7).map_err(|e| e.to_string())?,
            raw_segments_json: row.get(8).map_err(|e| e.to_string())?,
            error_msg: row.get(9).map_err(|e| e.to_string())?,
            facts_status: row.get(10).map_err(|e| e.to_string())?,
            facts_json: row.get(11).map_err(|e| e.to_string())?,
            facts_error_msg: row.get(12).map_err(|e| e.to_string())?,
        });
    }
    Ok(chunks)
}

#[command]
pub fn update_processing_chunk_result(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    index: usize,
    status: String,
    raw_segments_json: Option<String>,
    error_msg: Option<String>,
) -> Result<(), String> {
    if !matches!(status.as_str(), "pending" | "running" | "done" | "error") {
        return Err(format!("invalid processing chunk status: {status}"));
    }
    if status == "done" && raw_segments_json.is_none() {
        return Err("raw_segments_json is required when status is done".to_string());
    }

    let db = state.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let index_no =
        i64::try_from(index).map_err(|_| "processing chunk index is too large".to_string())?;
    let affected = db
        .execute(
            "UPDATE processing_chunks
         SET status = ?1, raw_segments_json = ?2, error_msg = ?3, updated_at = ?4
         WHERE meeting_id = ?5 AND index_no = ?6",
            params![
                status,
                raw_segments_json,
                error_msg,
                now,
                meeting_id,
                index_no
            ],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err(format!(
            "processing chunk not found for meeting_id {meeting_id} and index {index}"
        ));
    }

    Ok(())
}

#[command]
pub fn update_processing_chunk_facts(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    index: usize,
    status: String,
    facts_json: Option<String>,
    error_msg: Option<String>,
) -> Result<(), String> {
    if !matches!(status.as_str(), "pending" | "running" | "done" | "error") {
        return Err(format!("invalid processing chunk facts status: {status}"));
    }
    if status == "done" && facts_json.is_none() {
        return Err("facts_json is required when facts status is done".to_string());
    }

    let db = state.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let index_no =
        i64::try_from(index).map_err(|_| "processing chunk index is too large".to_string())?;
    let affected = db
        .execute(
            "UPDATE processing_chunks
         SET facts_status = ?1, facts_json = ?2, facts_error_msg = ?3, updated_at = ?4
         WHERE meeting_id = ?5 AND index_no = ?6",
            params![status, facts_json, error_msg, now, meeting_id, index_no],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err(format!(
            "processing chunk not found for meeting_id {meeting_id} and index {index}"
        ));
    }

    Ok(())
}
