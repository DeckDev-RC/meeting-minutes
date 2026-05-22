use crate::models::audio::{ExportedChunk, ProcessingChunkRecord};
use crate::models::meeting::Meeting;
use rusqlite::{params, Connection};
use std::sync::Mutex;
use tauri::command;

pub struct DbState(pub Mutex<Connection>);

const PROCESSING_CHUNKS_SCHEMA: &str = "CREATE TABLE processing_chunks (
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
    facts_status TEXT NOT NULL DEFAULT 'pending',
    facts_json TEXT,
    facts_error_msg TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (meeting_id, index_no)
)";

fn migrate_processing_chunks_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    let table_sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'processing_chunks'",
        [],
        |row| row.get(0),
    )?;

    if table_sql.contains("CHECK(index_no >= 0)") {
        return Ok(());
    }

    conn.execute_batch(&format!(
        "BEGIN;
        ALTER TABLE processing_chunks RENAME TO processing_chunks_old;
        {schema};
        INSERT INTO processing_chunks
            (meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, error_msg, created_at, updated_at)
        SELECT meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, error_msg, created_at, updated_at
        FROM processing_chunks_old
        WHERE index_no >= 0;
        DROP TABLE processing_chunks_old;
        COMMIT;",
        schema = PROCESSING_CHUNKS_SCHEMA
    ))
}

fn processing_chunks_has_column(
    conn: &Connection,
    column_name: &str,
) -> Result<bool, rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(processing_chunks)")?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn add_processing_chunk_column_if_missing(
    conn: &Connection,
    column_name: &str,
    definition: &str,
) -> Result<(), rusqlite::Error> {
    if processing_chunks_has_column(conn, column_name)? {
        return Ok(());
    }

    conn.execute(
        &format!("ALTER TABLE processing_chunks ADD COLUMN {definition}"),
        [],
    )?;
    Ok(())
}

fn migrate_processing_chunk_fact_cache(conn: &Connection) -> Result<(), rusqlite::Error> {
    add_processing_chunk_column_if_missing(
        conn,
        "facts_status",
        "facts_status TEXT NOT NULL DEFAULT 'pending'",
    )?;
    add_processing_chunk_column_if_missing(conn, "facts_json", "facts_json TEXT")?;
    add_processing_chunk_column_if_missing(conn, "facts_error_msg", "facts_error_msg TEXT")?;
    Ok(())
}

fn meetings_has_column(conn: &Connection, column_name: &str) -> Result<bool, rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(meetings)")?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn add_meeting_column_if_missing(
    conn: &Connection,
    column_name: &str,
    definition: &str,
) -> Result<(), rusqlite::Error> {
    if meetings_has_column(conn, column_name)? {
        return Ok(());
    }

    conn.execute(&format!("ALTER TABLE meetings ADD COLUMN {definition}"), [])?;
    Ok(())
}

fn migrate_meetings_metadata(conn: &Connection) -> Result<(), rusqlite::Error> {
    add_meeting_column_if_missing(conn, "participants_hint", "participants_hint TEXT")?;
    add_meeting_column_if_missing(
        conn,
        "processing_profile",
        "processing_profile TEXT NOT NULL DEFAULT 'balanced'",
    )?;
    add_meeting_column_if_missing(conn, "transcription_profile", "transcription_profile TEXT")?;
    Ok(())
}

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
            language TEXT DEFAULT 'pt',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS minutes (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL,
            html_content TEXT NOT NULL,
            pdf_path TEXT,
            model_used TEXT NOT NULL,
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
    migrate_processing_chunks_schema(&conn).expect("Failed to migrate processing_chunks schema");
    migrate_processing_chunk_fact_cache(&conn)
        .expect("Failed to migrate processing chunk fact cache schema");
    migrate_meetings_metadata(&conn).expect("Failed to migrate meetings metadata schema");

    conn
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
    ).map_err(|e| e.to_string())?;

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
    Ok(())
}

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
pub fn save_minutes(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    html_content: String,
    pdf_path: Option<String>,
    model_used: String,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, meeting_id, html_content, pdf_path, model_used, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn get_minutes_by_meeting(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.prepare(
        "SELECT id, meeting_id, html_content, pdf_path, model_used, created_at FROM minutes WHERE meeting_id = ?1 ORDER BY created_at DESC LIMIT 1"
    ).map_err(|e| e.to_string())?;

    let result = stmt.query_row(params![meeting_id], |row| {
        let id: String = row.get(0)?;
        let mid: String = row.get(1)?;
        let html: String = row.get(2)?;
        let pdf: Option<String> = row.get(3)?;
        let model: String = row.get(4)?;
        let created: String = row.get(5)?;
        Ok(serde_json::json!({
            "id": id,
            "meeting_id": mid,
            "html_content": html,
            "pdf_path": pdf,
            "model_used": model,
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
pub fn delete_meeting(state: tauri::State<'_, DbState>, id: String) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;

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
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn temp_app_dir(test_name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "meeting-minutes-{test_name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn processing_chunk_columns(conn: &Connection) -> HashSet<String> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(processing_chunks)")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn meeting_columns(conn: &Connection) -> HashSet<String> {
        let mut stmt = conn.prepare("PRAGMA table_info(meetings)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn init_db_creates_fact_cache_columns_for_new_databases() {
        let dir = temp_app_dir("fact-columns-new");
        let conn = init_db(&dir);

        let columns = processing_chunk_columns(&conn);

        assert!(columns.contains("facts_status"));
        assert!(columns.contains("facts_json"));
        assert!(columns.contains("facts_error_msg"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_migrates_existing_processing_chunks_to_fact_cache_columns() {
        let dir = temp_app_dir("fact-columns-migration");
        let db_path = dir.join("db.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE processing_chunks (
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
            )",
        )
        .unwrap();
        drop(conn);

        let migrated = init_db(&dir);
        let columns = processing_chunk_columns(&migrated);

        assert!(columns.contains("facts_status"));
        assert!(columns.contains("facts_json"));
        assert!(columns.contains("facts_error_msg"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_creates_meeting_metadata_columns_for_new_databases() {
        let dir = temp_app_dir("meeting-metadata-new");
        let conn = init_db(&dir);

        let columns = meeting_columns(&conn);

        assert!(columns.contains("participants_hint"));
        assert!(columns.contains("processing_profile"));
        assert!(columns.contains("transcription_profile"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_migrates_existing_meetings_to_metadata_columns() {
        let dir = temp_app_dir("meeting-metadata-migration");
        let db_path = dir.join("db.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT,
                file_path TEXT NOT NULL,
                audio_path TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .unwrap();
        drop(conn);

        let migrated = init_db(&dir);
        let columns = meeting_columns(&migrated);

        assert!(columns.contains("participants_hint"));
        assert!(columns.contains("processing_profile"));
        assert!(columns.contains("transcription_profile"));

        std::fs::remove_dir_all(dir).ok();
    }
}
