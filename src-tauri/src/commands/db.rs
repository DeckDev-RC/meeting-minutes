use rusqlite::{params, Connection};
use std::sync::Mutex;
use tauri::command;

pub struct DbState(pub Mutex<Connection>);

pub mod meetings;
pub mod processing_chunks;
pub mod processing_jobs;
mod schema;
pub mod structured_minutes;
pub mod transcriptions;

pub use meetings::{get_meetings, save_meeting, update_meeting_status};
pub use processing_chunks::{
    get_processing_chunks, save_processing_chunks, update_processing_chunk_facts,
    update_processing_chunk_result,
};
pub use processing_jobs::{get_processing_jobs, upsert_processing_job};
pub use structured_minutes::edit::{
    restore_minute_version, save_minute_revision, update_minute_action, update_minute_decision,
    update_minute_participants,
};
pub use structured_minutes::{
    get_minute_evidences, get_minutes_by_meeting, get_structured_minutes_by_meeting, save_minutes,
};
pub use transcriptions::{get_transcription_by_meeting, save_speaker_map, save_transcription};

pub fn init_db(app_data_dir: &std::path::Path) -> Connection {
    std::fs::create_dir_all(app_data_dir).ok();
    let db_path = app_data_dir.join("db.sqlite");
    let conn = Connection::open(db_path).expect("Failed to open database");

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meetings (
            id TEXT PRIMARY KEY,
            title TEXT,
            file_path TEXT NOT NULL,
            audio_path TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL,
            raw_whisper TEXT,
            diarized TEXT,
            speakers TEXT,
            speaker_map TEXT,
            language TEXT DEFAULT 'pt',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS minutes (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL,
            html_content TEXT NOT NULL,
            pdf_path TEXT,
            model_used TEXT NOT NULL,
            user_edited INTEGER NOT NULL DEFAULT 0,
            participant_names_json TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            error_msg TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS processing_chunks (
            meeting_id TEXT NOT NULL,
            index_no INTEGER NOT NULL CHECK(index_no >= 0),
            audio_path TEXT NOT NULL,
            start_sec REAL NOT NULL,
            end_sec REAL NOT NULL,
            offset_sec REAL NOT NULL,
            duration_sec REAL NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            raw_segments_json TEXT,
            error_msg TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (meeting_id, index_no)
        );",
    )
    .expect("Failed to create tables");
    schema::migrate_processing_chunks_schema(&conn)
        .expect("Failed to migrate processing_chunks schema");
    schema::migrate_processing_chunk_fact_cache(&conn)
        .expect("Failed to migrate processing chunk fact cache schema");
    schema::migrate_meetings_metadata(&conn).expect("Failed to migrate meetings metadata schema");
    schema::migrate_transcriptions_speaker_map(&conn)
        .expect("Failed to migrate transcriptions speaker map schema");
    schema::migrate_structured_minutes_schema(&conn)
        .expect("Failed to migrate structured minutes schema");

    conn
}

#[command]
pub fn delete_meeting(state: tauri::State<'_, DbState>, id: String) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;

    tx.execute(
        "DELETE FROM minute_evidences WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM minute_decisions WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM minute_actions WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM minute_versions WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM minutes WHERE meeting_id = ?1", params![&id])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM transcriptions WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM jobs WHERE meeting_id = ?1", params![&id])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM processing_jobs WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM processing_chunks WHERE meeting_id = ?1",
        params![&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM meetings WHERE id = ?1", params![&id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests;
