use crate::commands::minutes_validator::validate_evidence_against_segments_json;
use crate::models::audio::{ExportedChunk, ProcessingChunkRecord};
use crate::models::meeting::Meeting;
use crate::models::transcription::{MeetingAction, MeetingChunkInsights, MeetingDecision};
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

const STRUCTURED_MINUTES_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS minute_versions (
    id TEXT PRIMARY KEY,
    minute_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    version_no INTEGER NOT NULL CHECK(version_no > 0),
    html_content TEXT NOT NULL,
    facts_json TEXT,
    diarized_json TEXT,
    participant_names_json TEXT,
    change_reason TEXT,
    snapshot_json TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_minute_versions_meeting ON minute_versions (meeting_id, created_at);

CREATE TABLE IF NOT EXISTS minute_evidences (
    id TEXT PRIMARY KEY,
    minute_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    parent_type TEXT NOT NULL CHECK(parent_type IN ('decision', 'action')),
    parent_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
    quote TEXT NOT NULL,
    transcript_excerpt TEXT,
    validated INTEGER NOT NULL,
    validation_score REAL NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_minute_evidences_meeting ON minute_evidences (meeting_id);
CREATE INDEX IF NOT EXISTS idx_minute_evidences_parent ON minute_evidences (parent_type, parent_id);

CREATE TABLE IF NOT EXISTS minute_decisions (
    id TEXT PRIMARY KEY,
    minute_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    item_index INTEGER NOT NULL CHECK(item_index >= 0),
    chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
    title TEXT NOT NULL,
    owner TEXT,
    timestamp_sec REAL NOT NULL,
    evidence TEXT NOT NULL,
    evidence_id TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_minute_decisions_meeting ON minute_decisions (meeting_id);

CREATE TABLE IF NOT EXISTS minute_actions (
    id TEXT PRIMARY KEY,
    minute_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    item_index INTEGER NOT NULL CHECK(item_index >= 0),
    chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
    task TEXT NOT NULL,
    owner TEXT,
    deadline TEXT,
    timestamp_sec REAL NOT NULL,
    evidence TEXT NOT NULL,
    evidence_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'normal',
    completed_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_minute_actions_meeting ON minute_actions (meeting_id);

CREATE TABLE IF NOT EXISTS processing_jobs (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    stage TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'done', 'error')),
    progress_pct INTEGER NOT NULL CHECK(progress_pct BETWEEN 0 AND 100),
    error_msg TEXT,
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(meeting_id, stage)
);
CREATE INDEX IF NOT EXISTS idx_processing_jobs_meeting ON processing_jobs (meeting_id);
CREATE INDEX IF NOT EXISTS idx_processing_jobs_status ON processing_jobs (status);
";

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

fn transcriptions_has_column(
    conn: &Connection,
    column_name: &str,
) -> Result<bool, rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(transcriptions)")?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn table_has_column(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn add_table_column_if_missing(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    definition: &str,
) -> Result<(), rusqlite::Error> {
    if table_has_column(conn, table_name, column_name)? {
        return Ok(());
    }

    conn.execute(
        &format!("ALTER TABLE {table_name} ADD COLUMN {definition}"),
        [],
    )?;
    Ok(())
}

fn add_transcription_column_if_missing(
    conn: &Connection,
    column_name: &str,
    definition: &str,
) -> Result<(), rusqlite::Error> {
    if transcriptions_has_column(conn, column_name)? {
        return Ok(());
    }

    conn.execute(
        &format!("ALTER TABLE transcriptions ADD COLUMN {definition}"),
        [],
    )?;
    Ok(())
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

fn migrate_transcriptions_speaker_map(conn: &Connection) -> Result<(), rusqlite::Error> {
    add_transcription_column_if_missing(conn, "speaker_map", "speaker_map TEXT")?;
    Ok(())
}

fn migrate_structured_minutes_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(STRUCTURED_MINUTES_SCHEMA)?;
    add_table_column_if_missing(
        conn,
        "minutes",
        "user_edited",
        "user_edited INTEGER NOT NULL DEFAULT 0",
    )?;
    add_table_column_if_missing(
        conn,
        "minutes",
        "participant_names_json",
        "participant_names_json TEXT",
    )?;
    add_table_column_if_missing(
        conn,
        "minute_versions",
        "participant_names_json",
        "participant_names_json TEXT",
    )?;
    add_table_column_if_missing(
        conn,
        "minute_versions",
        "change_reason",
        "change_reason TEXT",
    )?;
    add_table_column_if_missing(
        conn,
        "minute_versions",
        "snapshot_json",
        "snapshot_json TEXT",
    )?;
    add_table_column_if_missing(
        conn,
        "minute_actions",
        "status",
        "status TEXT NOT NULL DEFAULT 'pending'",
    )?;
    add_table_column_if_missing(
        conn,
        "minute_actions",
        "priority",
        "priority TEXT NOT NULL DEFAULT 'normal'",
    )?;
    add_table_column_if_missing(conn, "minute_actions", "completed_at", "completed_at TEXT")?;
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
    migrate_processing_chunks_schema(&conn).expect("Failed to migrate processing_chunks schema");
    migrate_processing_chunk_fact_cache(&conn)
        .expect("Failed to migrate processing chunk fact cache schema");
    migrate_meetings_metadata(&conn).expect("Failed to migrate meetings metadata schema");
    migrate_transcriptions_speaker_map(&conn)
        .expect("Failed to migrate transcriptions speaker map schema");
    migrate_structured_minutes_schema(&conn).expect("Failed to migrate structured minutes schema");

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

fn normalize_processing_job_status(status: &str) -> Result<&str, String> {
    match status {
        "pending" | "running" | "done" | "error" => Ok(status),
        _ => Err(format!("invalid processing job status: {status}")),
    }
}

fn clamp_progress_pct(progress_pct: i64) -> i64 {
    progress_pct.clamp(0, 100)
}

#[command]
pub fn upsert_processing_job(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    stage: String,
    status: String,
    progress_pct: i64,
    error_msg: Option<String>,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    upsert_processing_job_record(&db, &meeting_id, &stage, &status, progress_pct, error_msg)
}

fn upsert_processing_job_record(
    db: &Connection,
    meeting_id: &str,
    stage: &str,
    status: &str,
    progress_pct: i64,
    error_msg: Option<String>,
) -> Result<(), String> {
    let stage = stage.trim();
    if stage.is_empty() {
        return Err("processing job stage is required".to_string());
    }
    let status = normalize_processing_job_status(status)?;
    let progress_pct = clamp_progress_pct(progress_pct);
    let now = chrono::Utc::now().to_rfc3339();
    let started_at = if status == "running" {
        Some(now.clone())
    } else {
        None
    };
    let finished_at = if matches!(status, "done" | "error") {
        Some(now.clone())
    } else {
        None
    };

    db.execute(
        "INSERT INTO processing_jobs
            (id, meeting_id, stage, status, progress_pct, error_msg, started_at, finished_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(meeting_id, stage) DO UPDATE SET
            status = excluded.status,
            progress_pct = excluded.progress_pct,
            error_msg = excluded.error_msg,
            started_at = COALESCE(processing_jobs.started_at, excluded.started_at),
            finished_at = excluded.finished_at,
            updated_at = excluded.updated_at",
        params![
            uuid::Uuid::new_v4().to_string(),
            meeting_id,
            stage,
            status,
            progress_pct,
            error_msg,
            started_at,
            finished_at,
            now,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn get_processing_jobs(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT id, meeting_id, stage, status, progress_pct, error_msg, started_at, finished_at, created_at, updated_at
             FROM processing_jobs
             WHERE meeting_id = ?1
             ORDER BY created_at ASC, stage ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![meeting_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "meetingId": row.get::<_, String>(1)?,
                "stage": row.get::<_, String>(2)?,
                "status": row.get::<_, String>(3)?,
                "progressPct": row.get::<_, i64>(4)?,
                "errorMsg": row.get::<_, Option<String>>(5)?,
                "startedAt": row.get::<_, Option<String>>(6)?,
                "finishedAt": row.get::<_, Option<String>>(7)?,
                "createdAt": row.get::<_, String>(8)?,
                "updatedAt": row.get::<_, String>(9)?,
            }))
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
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

fn parse_minutes_facts(facts_json: Option<&str>) -> Result<Vec<MeetingChunkInsights>, String> {
    let Some(facts_json) = facts_json.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };

    serde_json::from_str::<Vec<MeetingChunkInsights>>(facts_json)
        .map_err(|e| format!("invalid facts_json for structured minutes: {e}"))
}

fn normalize_participant_names(names: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();

    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let key = name.to_lowercase();
        if seen.insert(key) {
            normalized.push(name.to_string());
        }
    }

    normalized
}

fn encode_participant_names_json(names: Vec<String>) -> Result<Option<String>, String> {
    let names = normalize_participant_names(names);
    if names.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&names)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn parse_participant_names_json(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
        .map(normalize_participant_names)
        .unwrap_or_default()
}

fn chunk_segments_by_index(
    tx: &rusqlite::Transaction<'_>,
    meeting_id: &str,
) -> Result<std::collections::HashMap<usize, String>, String> {
    let mut stmt = tx
        .prepare(
            "SELECT index_no, raw_segments_json
             FROM processing_chunks
             WHERE meeting_id = ?1 AND raw_segments_json IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![meeting_id]).map_err(|e| e.to_string())?;
    let mut values = std::collections::HashMap::new();

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let index_no: i64 = row.get(0).map_err(|e| e.to_string())?;
        let index = usize::try_from(index_no)
            .map_err(|_| format!("invalid processing chunk index_no: {index_no}"))?;
        let raw_segments_json: String = row.get(1).map_err(|e| e.to_string())?;
        values.insert(index, raw_segments_json);
    }

    Ok(values)
}

fn next_minute_version_no(tx: &rusqlite::Transaction<'_>, meeting_id: &str) -> Result<i64, String> {
    tx.query_row(
        "SELECT COALESCE(MAX(version_no), 0) + 1 FROM minute_versions WHERE meeting_id = ?1",
        params![meeting_id],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())
}

fn insert_minute_evidence(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    parent_type: &str,
    parent_id: &str,
    chunk_index: usize,
    quote: &str,
    raw_segments_json: Option<&str>,
    now: &str,
) -> Result<String, String> {
    let evidence_id = uuid::Uuid::new_v4().to_string();
    let validation = validate_evidence_against_segments_json(quote, raw_segments_json);
    let chunk_index = i64::try_from(chunk_index).map_err(|_| "chunk index is too large")?;
    tx.execute(
        "INSERT INTO minute_evidences
            (id, minute_id, meeting_id, parent_type, parent_id, chunk_index, quote, transcript_excerpt, validated, validation_score, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            evidence_id,
            minute_id,
            meeting_id,
            parent_type,
            parent_id,
            chunk_index,
            quote,
            validation.transcript_excerpt,
            if validation.verified { 1_i64 } else { 0_i64 },
            validation.score,
            now
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(evidence_id)
}

fn insert_structured_decision(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    item_index: usize,
    chunk_index: usize,
    decision: &MeetingDecision,
    raw_segments_json: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let decision_id = uuid::Uuid::new_v4().to_string();
    let evidence_id = insert_minute_evidence(
        tx,
        minute_id,
        meeting_id,
        "decision",
        &decision_id,
        chunk_index,
        &decision.evidence,
        raw_segments_json,
        now,
    )?;
    tx.execute(
        "INSERT INTO minute_decisions
            (id, minute_id, meeting_id, item_index, chunk_index, title, owner, timestamp_sec, evidence, evidence_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            decision_id,
            minute_id,
            meeting_id,
            i64::try_from(item_index).map_err(|_| "decision index is too large")?,
            i64::try_from(chunk_index).map_err(|_| "chunk index is too large")?,
            decision.title,
            decision.owner,
            decision.timestamp_sec,
            decision.evidence,
            evidence_id,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_structured_action(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    item_index: usize,
    chunk_index: usize,
    action: &MeetingAction,
    raw_segments_json: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let action_id = uuid::Uuid::new_v4().to_string();
    let evidence_id = insert_minute_evidence(
        tx,
        minute_id,
        meeting_id,
        "action",
        &action_id,
        chunk_index,
        &action.evidence,
        raw_segments_json,
        now,
    )?;
    tx.execute(
        "INSERT INTO minute_actions
            (id, minute_id, meeting_id, item_index, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            action_id,
            minute_id,
            meeting_id,
            i64::try_from(item_index).map_err(|_| "action index is too large")?,
            i64::try_from(chunk_index).map_err(|_| "chunk index is too large")?,
            action.task,
            action.owner,
            action.deadline,
            action.timestamp_sec,
            action.evidence,
            evidence_id,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn persist_structured_minutes(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    facts: &[MeetingChunkInsights],
    now: &str,
) -> Result<(), String> {
    let segments_by_index = chunk_segments_by_index(tx, meeting_id)?;
    let mut decision_index = 0usize;
    let mut action_index = 0usize;

    for chunk in facts {
        let raw_segments_json = segments_by_index
            .get(&chunk.chunk_index)
            .map(String::as_str);
        for decision in &chunk.decisions {
            insert_structured_decision(
                tx,
                minute_id,
                meeting_id,
                decision_index,
                chunk.chunk_index,
                decision,
                raw_segments_json,
                now,
            )?;
            decision_index += 1;
        }
        for action in &chunk.actions {
            insert_structured_action(
                tx,
                minute_id,
                meeting_id,
                action_index,
                chunk.chunk_index,
                action,
                raw_segments_json,
                now,
            )?;
            action_index += 1;
        }
    }

    Ok(())
}

#[command]
pub fn save_minutes(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    html_content: String,
    pdf_path: Option<String>,
    model_used: String,
    facts_json: Option<String>,
    diarized_json: Option<String>,
    participant_names: Option<Vec<String>>,
) -> Result<(), String> {
    let facts = parse_minutes_facts(facts_json.as_deref())?;
    let participant_names_json = participant_names
        .map(encode_participant_names_json)
        .transpose()?
        .flatten();
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, participant_names_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &id,
            &meeting_id,
            &html_content,
            &pdf_path,
            &model_used,
            &participant_names_json,
            &now
        ],
    )
    .map_err(|e| e.to_string())?;
    let version_no = next_minute_version_no(&tx, &meeting_id)?;
    tx.execute(
        "INSERT INTO minute_versions
            (id, minute_id, meeting_id, version_no, html_content, facts_json, diarized_json, participant_names_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            uuid::Uuid::new_v4().to_string(),
            &id,
            &meeting_id,
            version_no,
            &html_content,
            &facts_json,
            &diarized_json,
            &participant_names_json,
            &now
        ],
    )
    .map_err(|e| e.to_string())?;

    if !facts.is_empty() {
        persist_structured_minutes(&tx, &id, &meeting_id, &facts, &now)?;
    }

    tx.commit().map_err(|e| e.to_string())?;
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

struct StoredMinuteRow {
    id: String,
    meeting_id: String,
    html_content: String,
    pdf_path: Option<String>,
    model_used: String,
    user_edited: bool,
    participant_names: Vec<String>,
    created_at: String,
}

fn latest_structured_minute(
    db: &Connection,
    meeting_id: &str,
) -> Result<Option<StoredMinuteRow>, String> {
    let mut stmt = db
        .prepare(
            "SELECT m.id, m.meeting_id, m.html_content, m.pdf_path, m.model_used, m.user_edited,
                    COALESCE(
                        m.participant_names_json,
                        (
                            SELECT v.participant_names_json
                            FROM minute_versions v
                            WHERE v.minute_id = m.id
                              AND v.participant_names_json IS NOT NULL
                            ORDER BY v.version_no DESC
                            LIMIT 1
                        )
                    ) AS participant_names_json,
                    m.created_at
             FROM minutes m
             WHERE m.meeting_id = ?1
               AND EXISTS (
                   SELECT 1
                   FROM minute_versions v
                   WHERE v.minute_id = m.id
               )
             ORDER BY m.created_at DESC
             LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let result = stmt.query_row(params![meeting_id], |row| {
        Ok(StoredMinuteRow {
            id: row.get(0)?,
            meeting_id: row.get(1)?,
            html_content: row.get(2)?,
            pdf_path: row.get(3)?,
            model_used: row.get(4)?,
            user_edited: row.get::<_, i64>(5)? == 1,
            participant_names: parse_participant_names_json(
                row.get::<_, Option<String>>(6)?.as_deref(),
            ),
            created_at: row.get(7)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn structured_decisions(
    db: &Connection,
    minute_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, minute_id, meeting_id, item_index, chunk_index, title, owner, timestamp_sec, evidence, evidence_id, created_at
             FROM minute_decisions
             WHERE minute_id = ?1
             ORDER BY item_index ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let values = stmt
        .query_map(params![minute_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "minuteId": row.get::<_, String>(1)?,
                "meetingId": row.get::<_, String>(2)?,
                "itemIndex": row.get::<_, i64>(3)?,
                "chunkIndex": row.get::<_, i64>(4)?,
                "title": row.get::<_, String>(5)?,
                "owner": row.get::<_, Option<String>>(6)?,
                "timestampSec": row.get::<_, f64>(7)?,
                "evidence": row.get::<_, String>(8)?,
                "evidenceId": row.get::<_, Option<String>>(9)?,
                "createdAt": row.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(values)
}

fn structured_actions(db: &Connection, minute_id: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, minute_id, meeting_id, item_index, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, status, priority, completed_at, created_at
             FROM minute_actions
             WHERE minute_id = ?1
             ORDER BY item_index ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let values = stmt
        .query_map(params![minute_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "minuteId": row.get::<_, String>(1)?,
                "meetingId": row.get::<_, String>(2)?,
                "itemIndex": row.get::<_, i64>(3)?,
                "chunkIndex": row.get::<_, i64>(4)?,
                "task": row.get::<_, String>(5)?,
                "owner": row.get::<_, Option<String>>(6)?,
                "deadline": row.get::<_, Option<String>>(7)?,
                "timestampSec": row.get::<_, f64>(8)?,
                "evidence": row.get::<_, String>(9)?,
                "evidenceId": row.get::<_, Option<String>>(10)?,
                "status": row.get::<_, String>(11)?,
                "priority": row.get::<_, String>(12)?,
                "completedAt": row.get::<_, Option<String>>(13)?,
                "createdAt": row.get::<_, String>(14)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(values)
}

fn minute_evidences_for_minute(
    db: &Connection,
    minute_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, minute_id, meeting_id, parent_type, parent_id, chunk_index, quote, transcript_excerpt, validated, validation_score, created_at
             FROM minute_evidences
             WHERE minute_id = ?1
             ORDER BY chunk_index ASC, parent_type ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let values = stmt
        .query_map(params![minute_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "minuteId": row.get::<_, String>(1)?,
                "meetingId": row.get::<_, String>(2)?,
                "parentType": row.get::<_, String>(3)?,
                "parentId": row.get::<_, String>(4)?,
                "chunkIndex": row.get::<_, i64>(5)?,
                "quote": row.get::<_, String>(6)?,
                "transcriptExcerpt": row.get::<_, Option<String>>(7)?,
                "validated": row.get::<_, i64>(8)? == 1,
                "validationScore": row.get::<_, f64>(9)?,
                "createdAt": row.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(values)
}

fn minute_versions_for_minute(
    db: &Connection,
    minute_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, minute_id, meeting_id, version_no, change_reason, snapshot_json, created_at
             FROM minute_versions
             WHERE minute_id = ?1
             ORDER BY version_no ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let values = stmt
        .query_map(params![minute_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "minuteId": row.get::<_, String>(1)?,
                "meetingId": row.get::<_, String>(2)?,
                "versionNo": row.get::<_, i64>(3)?,
                "changeReason": row.get::<_, Option<String>>(4)?,
                "hasSnapshot": row.get::<_, Option<String>>(5)?.is_some(),
                "createdAt": row.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(values)
}

fn get_structured_minutes_by_meeting_record(
    db: &Connection,
    meeting_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    let Some(minute) = latest_structured_minute(db, meeting_id)? else {
        return Ok(None);
    };

    let decisions = structured_decisions(db, &minute.id)?;
    let actions = structured_actions(db, &minute.id)?;
    let evidences = minute_evidences_for_minute(db, &minute.id)?;
    let versions = minute_versions_for_minute(db, &minute.id)?;

    Ok(Some(serde_json::json!({
        "minuteId": minute.id,
        "meetingId": minute.meeting_id,
        "htmlContent": minute.html_content,
        "pdfPath": minute.pdf_path,
        "modelUsed": minute.model_used,
        "userEdited": minute.user_edited,
        "participantNames": minute.participant_names,
        "createdAt": minute.created_at,
        "decisions": decisions,
        "actions": actions,
        "evidences": evidences,
        "versions": versions,
    })))
}

fn get_minute_evidences_record(
    db: &Connection,
    meeting_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let Some(minute) = latest_structured_minute(db, meeting_id)? else {
        return Ok(Vec::new());
    };

    minute_evidences_for_minute(db, &minute.id)
}

struct StoredActionRow {
    id: String,
    minute_id: String,
    meeting_id: String,
    chunk_index: i64,
    task: String,
    owner: Option<String>,
    deadline: Option<String>,
    timestamp_sec: f64,
    evidence: String,
    evidence_id: Option<String>,
    status: String,
    priority: String,
    completed_at: Option<String>,
}

struct StoredDecisionRow {
    id: String,
    minute_id: String,
    meeting_id: String,
    chunk_index: i64,
    title: String,
    owner: Option<String>,
    timestamp_sec: f64,
    evidence: String,
    evidence_id: Option<String>,
}

struct StoredVersionRow {
    minute_id: String,
    meeting_id: String,
    html_content: String,
    snapshot_json: String,
    version_no: i64,
}

fn stored_action_by_id(db: &Connection, action_id: &str) -> Result<StoredActionRow, String> {
    db.query_row(
        "SELECT id, minute_id, meeting_id, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, status, priority, completed_at
         FROM minute_actions
         WHERE id = ?1",
        params![action_id],
        |row| {
            Ok(StoredActionRow {
                id: row.get(0)?,
                minute_id: row.get(1)?,
                meeting_id: row.get(2)?,
                chunk_index: row.get(3)?,
                task: row.get(4)?,
                owner: row.get(5)?,
                deadline: row.get(6)?,
                timestamp_sec: row.get(7)?,
                evidence: row.get(8)?,
                evidence_id: row.get(9)?,
                status: row.get(10)?,
                priority: row.get(11)?,
                completed_at: row.get(12)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "minute action not found".to_string(),
        other => other.to_string(),
    })
}

fn stored_decision_by_id(db: &Connection, decision_id: &str) -> Result<StoredDecisionRow, String> {
    db.query_row(
        "SELECT id, minute_id, meeting_id, chunk_index, title, owner, timestamp_sec, evidence, evidence_id
         FROM minute_decisions
         WHERE id = ?1",
        params![decision_id],
        |row| {
            Ok(StoredDecisionRow {
                id: row.get(0)?,
                minute_id: row.get(1)?,
                meeting_id: row.get(2)?,
                chunk_index: row.get(3)?,
                title: row.get(4)?,
                owner: row.get(5)?,
                timestamp_sec: row.get(6)?,
                evidence: row.get(7)?,
                evidence_id: row.get(8)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "minute decision not found".to_string(),
        other => other.to_string(),
    })
}

fn stored_version_by_id(db: &Connection, version_id: &str) -> Result<StoredVersionRow, String> {
    db.query_row(
        "SELECT minute_id, meeting_id, html_content, snapshot_json, version_no
         FROM minute_versions
         WHERE id = ?1",
        params![version_id],
        |row| {
            Ok(StoredVersionRow {
                minute_id: row.get(0)?,
                meeting_id: row.get(1)?,
                html_content: row.get(2)?,
                snapshot_json: row.get(3)?,
                version_no: row.get(4)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => "minute version not found".to_string(),
        other => other.to_string(),
    })
}

fn patch_string(
    patch: &serde_json::Value,
    key: &str,
    current: &str,
    required: bool,
) -> Result<String, String> {
    let Some(value) = patch.get(key) else {
        return Ok(current.to_string());
    };
    let Some(text) = value.as_str() else {
        return Err(format!("{key} must be a string"));
    };
    let text = text.trim();
    if required && text.is_empty() {
        return Err(format!("{key} cannot be empty"));
    }
    Ok(text.to_string())
}

fn patch_optional_string(
    patch: &serde_json::Value,
    key: &str,
    current: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(value) = patch.get(key) else {
        return Ok(current.map(ToOwned::to_owned));
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(text) = value.as_str() else {
        return Err(format!("{key} must be a string or null"));
    };
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_string()))
}

fn patch_f64(patch: &serde_json::Value, key: &str, current: f64) -> Result<f64, String> {
    let Some(value) = patch.get(key) else {
        return Ok(current);
    };
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .ok_or_else(|| format!("{key} must be a non-negative number"))
}

fn normalize_action_status(status: &str) -> Result<&str, String> {
    match status {
        "pending" | "in_progress" | "done" | "canceled" => Ok(status),
        _ => Err(format!("invalid action status: {status}")),
    }
}

fn normalize_action_priority(priority: &str) -> Result<&str, String> {
    match priority {
        "low" | "normal" | "high" => Ok(priority),
        _ => Err(format!("invalid action priority: {priority}")),
    }
}

fn insert_minute_snapshot_version(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    html_content: &str,
    reason: &str,
    snapshot_json: &str,
    now: &str,
) -> Result<String, String> {
    let version_id = uuid::Uuid::new_v4().to_string();
    let version_no = next_minute_version_no(tx, meeting_id)?;
    tx.execute(
        "INSERT INTO minute_versions
            (id, minute_id, meeting_id, version_no, html_content, change_reason, snapshot_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            version_id,
            minute_id,
            meeting_id,
            version_no,
            html_content,
            reason,
            snapshot_json,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(version_id)
}

fn raw_segments_json_for_chunk(
    tx: &rusqlite::Transaction<'_>,
    meeting_id: &str,
    chunk_index: i64,
) -> Result<Option<String>, String> {
    match tx.query_row(
        "SELECT raw_segments_json
         FROM processing_chunks
         WHERE meeting_id = ?1 AND index_no = ?2",
        params![meeting_id, chunk_index],
        |row| row.get::<_, Option<String>>(0),
    ) {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn update_existing_evidence_validation(
    tx: &rusqlite::Transaction<'_>,
    evidence_id: Option<&str>,
    meeting_id: &str,
    chunk_index: i64,
    quote: &str,
) -> Result<(), String> {
    let Some(evidence_id) = evidence_id else {
        return Ok(());
    };
    let raw_segments_json = raw_segments_json_for_chunk(tx, meeting_id, chunk_index)?;
    let validation = validate_evidence_against_segments_json(quote, raw_segments_json.as_deref());
    tx.execute(
        "UPDATE minute_evidences
         SET quote = ?1, transcript_excerpt = ?2, validated = ?3, validation_score = ?4
         WHERE id = ?5",
        params![
            quote,
            validation.transcript_excerpt,
            if validation.verified { 1_i64 } else { 0_i64 },
            validation.score,
            evidence_id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_minute_action_record(
    db: &mut Connection,
    action_id: &str,
    patch: serde_json::Value,
    reason: Option<String>,
) -> Result<(), String> {
    let action = stored_action_by_id(db, action_id)?;
    let snapshot = get_structured_minutes_by_meeting_record(db, &action.meeting_id)?
        .ok_or_else(|| "structured minute not found for action".to_string())?;
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Atualizacao de acao")
        .to_string();

    let task = patch_string(&patch, "task", &action.task, true)?;
    let owner = patch_optional_string(&patch, "owner", action.owner.as_deref())?;
    let deadline = patch_optional_string(&patch, "deadline", action.deadline.as_deref())?;
    let timestamp_sec = patch_f64(&patch, "timestampSec", action.timestamp_sec)?;
    let evidence = patch_string(&patch, "evidence", &action.evidence, true)?;
    let status = patch_string(&patch, "status", &action.status, true)?;
    let status = normalize_action_status(&status)?.to_string();
    let priority = patch_string(&patch, "priority", &action.priority, true)?;
    let priority = normalize_action_priority(&priority)?.to_string();
    let completed_at =
        patch_optional_string(&patch, "completedAt", action.completed_at.as_deref())?;

    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let html_content: String = tx
        .query_row(
            "SELECT html_content FROM minutes WHERE id = ?1",
            params![&action.minute_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    insert_minute_snapshot_version(
        &tx,
        &action.minute_id,
        &action.meeting_id,
        &html_content,
        &reason,
        &snapshot_json,
        &now,
    )?;
    tx.execute(
        "UPDATE minute_actions
         SET task = ?1, owner = ?2, deadline = ?3, timestamp_sec = ?4, evidence = ?5,
             status = ?6, priority = ?7, completed_at = ?8
         WHERE id = ?9",
        params![
            task,
            owner,
            deadline,
            timestamp_sec,
            evidence,
            status,
            priority,
            completed_at,
            action.id
        ],
    )
    .map_err(|e| e.to_string())?;
    update_existing_evidence_validation(
        &tx,
        action.evidence_id.as_deref(),
        &action.meeting_id,
        action.chunk_index,
        &evidence,
    )?;
    tx.execute(
        "UPDATE minutes SET user_edited = 1 WHERE id = ?1",
        params![&action.minute_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn update_minute_decision_record(
    db: &mut Connection,
    decision_id: &str,
    patch: serde_json::Value,
    reason: Option<String>,
) -> Result<(), String> {
    let decision = stored_decision_by_id(db, decision_id)?;
    let snapshot = get_structured_minutes_by_meeting_record(db, &decision.meeting_id)?
        .ok_or_else(|| "structured minute not found for decision".to_string())?;
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Atualizacao de decisao")
        .to_string();

    let title = patch_string(&patch, "title", &decision.title, true)?;
    let owner = patch_optional_string(&patch, "owner", decision.owner.as_deref())?;
    let timestamp_sec = patch_f64(&patch, "timestampSec", decision.timestamp_sec)?;
    let evidence = patch_string(&patch, "evidence", &decision.evidence, true)?;

    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let html_content: String = tx
        .query_row(
            "SELECT html_content FROM minutes WHERE id = ?1",
            params![&decision.minute_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    insert_minute_snapshot_version(
        &tx,
        &decision.minute_id,
        &decision.meeting_id,
        &html_content,
        &reason,
        &snapshot_json,
        &now,
    )?;
    tx.execute(
        "UPDATE minute_decisions
         SET title = ?1, owner = ?2, timestamp_sec = ?3, evidence = ?4
         WHERE id = ?5",
        params![title, owner, timestamp_sec, evidence, decision.id],
    )
    .map_err(|e| e.to_string())?;
    update_existing_evidence_validation(
        &tx,
        decision.evidence_id.as_deref(),
        &decision.meeting_id,
        decision.chunk_index,
        &evidence,
    )?;
    tx.execute(
        "UPDATE minutes SET user_edited = 1 WHERE id = ?1",
        params![&decision.minute_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn json_string(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{key} is required"))
}

fn json_optional_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn json_i64(value: &serde_json::Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| format!("{key} is required"))
}

fn json_f64(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| format!("{key} is required"))
}

fn json_bool(value: &serde_json::Value, key: &str) -> Result<bool, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| format!("{key} is required"))
}

fn json_string_array(value: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    let Some(items) = value.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = items.as_array() else {
        return Err(format!("{key} must be an array in minute snapshot"));
    };
    Ok(normalize_participant_names(
        items
            .iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect(),
    ))
}

fn restore_evidence_rows(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    evidences: &[serde_json::Value],
) -> Result<(), String> {
    for evidence in evidences {
        tx.execute(
            "INSERT INTO minute_evidences
                (id, minute_id, meeting_id, parent_type, parent_id, chunk_index, quote, transcript_excerpt, validated, validation_score, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                json_string(evidence, "id")?,
                minute_id,
                meeting_id,
                json_string(evidence, "parentType")?,
                json_string(evidence, "parentId")?,
                json_i64(evidence, "chunkIndex")?,
                json_string(evidence, "quote")?,
                json_optional_string(evidence, "transcriptExcerpt"),
                if json_bool(evidence, "validated")? { 1_i64 } else { 0_i64 },
                json_f64(evidence, "validationScore")?,
                json_string(evidence, "createdAt")?,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn restore_decision_rows(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    decisions: &[serde_json::Value],
) -> Result<(), String> {
    for decision in decisions {
        tx.execute(
            "INSERT INTO minute_decisions
                (id, minute_id, meeting_id, item_index, chunk_index, title, owner, timestamp_sec, evidence, evidence_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                json_string(decision, "id")?,
                minute_id,
                meeting_id,
                json_i64(decision, "itemIndex")?,
                json_i64(decision, "chunkIndex")?,
                json_string(decision, "title")?,
                json_optional_string(decision, "owner"),
                json_f64(decision, "timestampSec")?,
                json_string(decision, "evidence")?,
                json_optional_string(decision, "evidenceId"),
                json_string(decision, "createdAt")?,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn restore_action_rows(
    tx: &rusqlite::Transaction<'_>,
    minute_id: &str,
    meeting_id: &str,
    actions: &[serde_json::Value],
) -> Result<(), String> {
    for action in actions {
        tx.execute(
            "INSERT INTO minute_actions
                (id, minute_id, meeting_id, item_index, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, status, priority, completed_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                json_string(action, "id")?,
                minute_id,
                meeting_id,
                json_i64(action, "itemIndex")?,
                json_i64(action, "chunkIndex")?,
                json_string(action, "task")?,
                json_optional_string(action, "owner"),
                json_optional_string(action, "deadline"),
                json_f64(action, "timestampSec")?,
                json_string(action, "evidence")?,
                json_optional_string(action, "evidenceId"),
                json_optional_string(action, "status").unwrap_or_else(|| "pending".to_string()),
                json_optional_string(action, "priority").unwrap_or_else(|| "normal".to_string()),
                json_optional_string(action, "completedAt"),
                json_string(action, "createdAt")?,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn snapshot_array<'a>(
    snapshot: &'a serde_json::Value,
    key: &str,
) -> Result<&'a [serde_json::Value], String> {
    snapshot
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} must be an array in minute snapshot"))
}

fn restore_minute_version_record(db: &mut Connection, version_id: &str) -> Result<(), String> {
    let version = stored_version_by_id(db, version_id)?;
    let target_snapshot: serde_json::Value =
        serde_json::from_str(&version.snapshot_json).map_err(|e| {
            format!(
                "minute version {} does not contain a valid restore snapshot: {e}",
                version.version_no
            )
        })?;
    let current_snapshot = get_structured_minutes_by_meeting_record(db, &version.meeting_id)?
        .ok_or_else(|| "current structured minute not found".to_string())?;
    let current_snapshot_json =
        serde_json::to_string(&current_snapshot).map_err(|e| e.to_string())?;
    let decisions = snapshot_array(&target_snapshot, "decisions")?;
    let actions = snapshot_array(&target_snapshot, "actions")?;
    let evidences = snapshot_array(&target_snapshot, "evidences")?;
    let html_content = target_snapshot
        .get("htmlContent")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&version.html_content)
        .to_string();
    let participant_names_json =
        encode_participant_names_json(json_string_array(&target_snapshot, "participantNames")?)?;

    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    insert_minute_snapshot_version(
        &tx,
        &version.minute_id,
        &version.meeting_id,
        &version.html_content,
        &format!("Restaurar versao {}", version.version_no),
        &current_snapshot_json,
        &now,
    )?;
    tx.execute(
        "DELETE FROM minute_evidences WHERE minute_id = ?1",
        params![&version.minute_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM minute_decisions WHERE minute_id = ?1",
        params![&version.minute_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM minute_actions WHERE minute_id = ?1",
        params![&version.minute_id],
    )
    .map_err(|e| e.to_string())?;
    restore_evidence_rows(&tx, &version.minute_id, &version.meeting_id, evidences)?;
    restore_decision_rows(&tx, &version.minute_id, &version.meeting_id, decisions)?;
    restore_action_rows(&tx, &version.minute_id, &version.meeting_id, actions)?;
    tx.execute(
        "UPDATE minutes SET html_content = ?1, participant_names_json = ?2, user_edited = 1 WHERE id = ?3",
        params![html_content, participant_names_json, &version.minute_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn save_minute_revision_record(
    db: &mut Connection,
    meeting_id: &str,
    reason: Option<String>,
    structured_payload: Option<serde_json::Value>,
) -> Result<String, String> {
    let minute = latest_structured_minute(db, meeting_id)?
        .ok_or_else(|| "structured minute not found".to_string())?;
    let snapshot = match structured_payload {
        Some(value) if value.is_object() => value,
        Some(_) => return Err("structured_payload must be a JSON object".to_string()),
        None => get_structured_minutes_by_meeting_record(db, meeting_id)?
            .ok_or_else(|| "structured minute not found".to_string())?,
    };
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Revisao manual")
        .to_string();
    let html_content = snapshot
        .get("htmlContent")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&minute.html_content)
        .to_string();
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let version_id = insert_minute_snapshot_version(
        &tx,
        &minute.id,
        &minute.meeting_id,
        &html_content,
        &reason,
        &snapshot_json,
        &now,
    )?;
    tx.execute(
        "UPDATE minutes SET user_edited = 1 WHERE id = ?1",
        params![&minute.id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(version_id)
}

fn update_minute_participants_record(
    db: &mut Connection,
    meeting_id: &str,
    participant_names: Vec<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let minute = latest_structured_minute(db, meeting_id)?
        .ok_or_else(|| "structured minute not found".to_string())?;
    let snapshot = get_structured_minutes_by_meeting_record(db, meeting_id)?
        .ok_or_else(|| "structured minute not found".to_string())?;
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let participant_names_json = encode_participant_names_json(participant_names)?;
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Revisao manual dos participantes")
        .to_string();

    let tx = db.transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    insert_minute_snapshot_version(
        &tx,
        &minute.id,
        &minute.meeting_id,
        &minute.html_content,
        &reason,
        &snapshot_json,
        &now,
    )?;
    tx.execute(
        "UPDATE minutes
         SET participant_names_json = ?1, user_edited = 1
         WHERE id = ?2",
        params![participant_names_json, &minute.id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn get_structured_minutes_by_meeting(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    get_structured_minutes_by_meeting_record(&db, &meeting_id)
}

#[command]
pub fn get_minute_evidences(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    get_minute_evidences_record(&db, &meeting_id)
}

#[command]
pub fn update_minute_action(
    state: tauri::State<'_, DbState>,
    action_id: String,
    patch: serde_json::Value,
    reason: Option<String>,
) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    update_minute_action_record(&mut db, &action_id, patch, reason)
}

#[command]
pub fn update_minute_decision(
    state: tauri::State<'_, DbState>,
    decision_id: String,
    patch: serde_json::Value,
    reason: Option<String>,
) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    update_minute_decision_record(&mut db, &decision_id, patch, reason)
}

#[command]
pub fn save_minute_revision(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    reason: Option<String>,
    structured_payload: Option<serde_json::Value>,
) -> Result<String, String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    save_minute_revision_record(&mut db, &meeting_id, reason, structured_payload)
}

#[command]
pub fn update_minute_participants(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    participant_names: Vec<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    update_minute_participants_record(&mut db, &meeting_id, participant_names, reason)
}

#[command]
pub fn restore_minute_version(
    state: tauri::State<'_, DbState>,
    version_id: String,
) -> Result<(), String> {
    let mut db = state.0.lock().map_err(|e| e.to_string())?;
    restore_minute_version_record(&mut db, &version_id)
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

    fn transcription_columns(conn: &Connection) -> HashSet<String> {
        let mut stmt = conn.prepare("PRAGMA table_info(transcriptions)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn table_columns(conn: &Connection, table: &str) -> HashSet<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn table_names(conn: &Connection) -> HashSet<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn row_count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn seed_reviewable_minute(conn: &Connection) {
        conn.execute(
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, participant_names_json, created_at)
             VALUES ('minute-1', 'meeting-1', '<h1>Ata</h1>', NULL, 'gemini', '[\"Caio\",\"Maria\"]', '2026-05-24T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO processing_chunks
                (meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, created_at, updated_at)
             VALUES ('meeting-1', 0, 'chunk.flac', 0, 60, 0, 60, 'done', ?1, 'now', 'now')",
            params![serde_json::json!([
                { "id": 0, "start": 0.0, "end": 5.0, "text": "Caio aprovou a entrega final." },
                { "id": 1, "start": 5.0, "end": 9.0, "text": "Maria envia o resumo revisado hoje." }
            ])
            .to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_versions
                (id, minute_id, meeting_id, version_no, html_content, facts_json, diarized_json, participant_names_json, created_at)
             VALUES ('version-1', 'minute-1', 'meeting-1', 1, '<h1>Ata</h1>', '[]', '{}', NULL, '2026-05-24T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_evidences
                (id, minute_id, meeting_id, parent_type, parent_id, chunk_index, quote, transcript_excerpt, validated, validation_score, created_at)
             VALUES
                ('evidence-decision-1', 'minute-1', 'meeting-1', 'decision', 'decision-1', 0, 'Caio aprovou a entrega', 'Caio aprovou a entrega final.', 1, 1.0, '2026-05-24T10:00:00Z'),
                ('evidence-action-1', 'minute-1', 'meeting-1', 'action', 'action-1', 0, 'Maria envia o resumo', 'Maria envia o resumo revisado hoje.', 1, 1.0, '2026-05-24T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_decisions
                (id, minute_id, meeting_id, item_index, chunk_index, title, owner, timestamp_sec, evidence, evidence_id, created_at)
             VALUES ('decision-1', 'minute-1', 'meeting-1', 0, 0, 'Aprovar entrega', 'Caio', 4.0, 'Caio aprovou a entrega', 'evidence-decision-1', '2026-05-24T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_actions
                (id, minute_id, meeting_id, item_index, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, created_at)
             VALUES ('action-1', 'minute-1', 'meeting-1', 0, 0, 'Enviar resumo', 'Maria', 'hoje', 7.0, 'Maria envia o resumo', 'evidence-action-1', '2026-05-24T10:00:00Z')",
            [],
        )
        .unwrap();
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
    fn init_db_creates_speaker_map_column_for_new_databases() {
        let dir = temp_app_dir("speaker-map-new");
        let conn = init_db(&dir);

        let columns = transcription_columns(&conn);

        assert!(columns.contains("speaker_map"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_creates_structured_minutes_tables_for_new_databases() {
        let dir = temp_app_dir("structured-minutes-new");
        let conn = init_db(&dir);

        let tables = table_names(&conn);

        assert!(tables.contains("minute_versions"));
        assert!(tables.contains("minute_decisions"));
        assert!(tables.contains("minute_actions"));
        assert!(tables.contains("minute_evidences"));
        assert!(tables.contains("processing_jobs"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_migrates_existing_database_to_structured_minutes_tables() {
        let dir = temp_app_dir("structured-minutes-migration");
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
            );
            CREATE TABLE transcriptions (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                raw_whisper TEXT,
                diarized TEXT,
                speakers TEXT,
                language TEXT DEFAULT 'pt',
                created_at TEXT NOT NULL
            );
            CREATE TABLE minutes (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                html_content TEXT NOT NULL,
                pdf_path TEXT,
                model_used TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE jobs (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                step TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                error_msg TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE processing_chunks (
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
        .unwrap();
        drop(conn);

        let migrated = init_db(&dir);
        let tables = table_names(&migrated);

        assert!(tables.contains("minute_versions"));
        assert!(tables.contains("minute_decisions"));
        assert!(tables.contains("minute_actions"));
        assert!(tables.contains("minute_evidences"));
        assert!(tables.contains("processing_jobs"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn structured_minutes_persistence_writes_items_and_validated_evidence() {
        let dir = temp_app_dir("structured-minutes-persist");
        let mut conn = init_db(&dir);
        conn.execute(
            "INSERT INTO processing_chunks
                (meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, created_at, updated_at)
             VALUES ('meeting-1', 0, 'chunk.flac', 0, 60, 0, 60, 'done', ?1, 'now', 'now')",
            params![serde_json::json!([
                { "id": 0, "start": 0.0, "end": 5.0, "text": "Caio vai revisar o contrato ate sexta." },
                { "id": 1, "start": 5.0, "end": 9.0, "text": "Maria envia o resumo ainda hoje." }
            ])
            .to_string()],
        )
        .unwrap();
        let facts = vec![MeetingChunkInsights {
            chunk_index: 0,
            start_sec: 0.0,
            end_sec: 60.0,
            summary: "Alinhamento de contratos.".to_string(),
            topics: vec!["Contratos".to_string()],
            decisions: vec![MeetingDecision {
                title: "Revisar contrato".to_string(),
                owner: "Caio".to_string(),
                timestamp_sec: 3.0,
                evidence: "Caio vai revisar o contrato".to_string(),
            }],
            actions: vec![MeetingAction {
                task: "Enviar resumo".to_string(),
                owner: "Maria".to_string(),
                deadline: "hoje".to_string(),
                timestamp_sec: 7.0,
                evidence: "Maria envia o resumo".to_string(),
            }],
            questions: vec![],
            risks: vec![],
        }];

        let tx = conn.transaction().unwrap();
        persist_structured_minutes(&tx, "minute-1", "meeting-1", &facts, "now").unwrap();
        tx.commit().unwrap();

        assert_eq!(row_count(&conn, "minute_decisions"), 1);
        assert_eq!(row_count(&conn, "minute_actions"), 1);
        assert_eq!(row_count(&conn, "minute_evidences"), 2);
        let verified: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM minute_evidences WHERE validated = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(verified, 2);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn structured_minutes_reader_returns_latest_minute_items_versions_and_evidences() {
        let dir = temp_app_dir("structured-minutes-reader");
        let conn = init_db(&dir);
        conn.execute(
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, created_at)
             VALUES ('minute-1', 'meeting-1', '<h1>Anterior</h1>', NULL, 'gemini-old', '2026-05-23T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, created_at)
             VALUES ('minute-2', 'meeting-1', '<h1>Atual</h1>', 'ata.pdf', 'gemini-2.5-flash', '2026-05-23T11:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_versions
                (id, minute_id, meeting_id, version_no, html_content, facts_json, diarized_json, participant_names_json, created_at)
             VALUES ('version-old', 'minute-1', 'meeting-1', 1, '<h1>Anterior</h1>', '[]', '{}', NULL, '2026-05-23T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_versions
                (id, minute_id, meeting_id, version_no, html_content, facts_json, diarized_json, participant_names_json, created_at)
             VALUES ('version-1', 'minute-2', 'meeting-1', 1, '<h1>Atual</h1>', '[]', '{}', '[\"Caio\"]', '2026-05-23T11:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_evidences
                (id, minute_id, meeting_id, parent_type, parent_id, chunk_index, quote, transcript_excerpt, validated, validation_score, created_at)
             VALUES
                ('evidence-1', 'minute-2', 'meeting-1', 'decision', 'decision-1', 0, 'Caio aprovou a entrega', 'Caio aprovou a entrega hoje', 1, 0.95, '2026-05-23T11:00:00Z'),
                ('evidence-2', 'minute-2', 'meeting-1', 'action', 'action-1', 1, 'Rafaela revisa o Drive', NULL, 0, 0.34, '2026-05-23T11:01:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_decisions
                (id, minute_id, meeting_id, item_index, chunk_index, title, owner, timestamp_sec, evidence, evidence_id, created_at)
             VALUES ('decision-1', 'minute-2', 'meeting-1', 0, 0, 'Aprovar entrega', 'Caio', 12.5, 'Caio aprovou a entrega', 'evidence-1', '2026-05-23T11:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO minute_actions
                (id, minute_id, meeting_id, item_index, chunk_index, task, owner, deadline, timestamp_sec, evidence, evidence_id, created_at)
             VALUES ('action-1', 'minute-2', 'meeting-1', 0, 1, 'Revisar Drive', 'Rafaela', 'sexta-feira', 74.0, 'Rafaela revisa o Drive', 'evidence-2', '2026-05-23T11:01:00Z')",
            [],
        )
        .unwrap();

        let structured = get_structured_minutes_by_meeting_record(&conn, "meeting-1")
            .unwrap()
            .unwrap();

        assert_eq!(structured["minuteId"], "minute-2");
        assert_eq!(structured["meetingId"], "meeting-1");
        assert_eq!(structured["htmlContent"], "<h1>Atual</h1>");
        assert_eq!(structured["modelUsed"], "gemini-2.5-flash");
        assert_eq!(structured["participantNames"][0], "Caio");
        assert_eq!(structured["decisions"][0]["title"], "Aprovar entrega");
        assert_eq!(structured["decisions"][0]["evidenceId"], "evidence-1");
        assert_eq!(structured["actions"][0]["task"], "Revisar Drive");
        assert_eq!(structured["actions"][0]["deadline"], "sexta-feira");
        assert_eq!(structured["evidences"][0]["validated"], true);
        assert_eq!(structured["evidences"][1]["validationScore"], 0.34);
        assert_eq!(structured["versions"].as_array().unwrap().len(), 1);
        assert_eq!(structured["versions"][0]["versionNo"], 1);
        assert_eq!(structured["versions"][0]["minuteId"], "minute-2");

        let evidences = get_minute_evidences_record(&conn, "meeting-1").unwrap();
        assert_eq!(evidences.len(), 2);
        assert_eq!(evidences[0]["parentType"], "decision");
        assert_eq!(evidences[1]["validated"], false);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn structured_minutes_reader_returns_none_for_legacy_html_only_minutes() {
        let dir = temp_app_dir("structured-minutes-legacy");
        let conn = init_db(&dir);
        conn.execute(
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, created_at)
             VALUES ('minute-legacy', 'meeting-legacy', '<h1>Legado</h1>', NULL, 'gemini-old', '2026-05-23T10:00:00Z')",
            [],
        )
        .unwrap();

        let structured = get_structured_minutes_by_meeting_record(&conn, "meeting-legacy").unwrap();

        assert!(structured.is_none());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_creates_phase_two_review_columns() {
        let dir = temp_app_dir("phase-two-columns");
        let conn = init_db(&dir);

        let minutes = table_columns(&conn, "minutes");
        let versions = table_columns(&conn, "minute_versions");
        let actions = table_columns(&conn, "minute_actions");

        assert!(minutes.contains("user_edited"));
        assert!(minutes.contains("participant_names_json"));
        assert!(versions.contains("change_reason"));
        assert!(versions.contains("snapshot_json"));
        assert!(actions.contains("status"));
        assert!(actions.contains("priority"));
        assert!(actions.contains("completed_at"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn update_minute_action_record_creates_snapshot_and_marks_minute_edited() {
        let dir = temp_app_dir("phase-two-action-update");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);

        update_minute_action_record(
            &mut conn,
            "action-1",
            serde_json::json!({
                "task": "Enviar resumo revisado",
                "owner": "Caio",
                "status": "done",
                "priority": "high",
                "completedAt": "2026-05-24T12:00:00Z"
            }),
            Some("corrigir responsavel".to_string()),
        )
        .unwrap();

        let (task, owner, status, priority, completed_at): (
            String,
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT task, owner, status, priority, completed_at FROM minute_actions WHERE id = 'action-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(task, "Enviar resumo revisado");
        assert_eq!(owner, "Caio");
        assert_eq!(status, "done");
        assert_eq!(priority, "high");
        assert_eq!(completed_at, "2026-05-24T12:00:00Z");

        let user_edited: i64 = conn
            .query_row(
                "SELECT user_edited FROM minutes WHERE id = 'minute-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(user_edited, 1);
        assert_eq!(row_count(&conn, "minute_versions"), 2);
        let (reason, snapshot): (String, String) = conn
            .query_row(
                "SELECT change_reason, snapshot_json FROM minute_versions WHERE version_no = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(reason, "corrigir responsavel");
        assert!(snapshot.contains("Enviar resumo"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn update_minute_decision_record_creates_snapshot_and_revalidates_evidence() {
        let dir = temp_app_dir("phase-two-decision-update");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);

        update_minute_decision_record(
            &mut conn,
            "decision-1",
            serde_json::json!({
                "title": "Aprovar entrega revisada",
                "owner": "Rafaela",
                "evidence": "orcamento internacional aprovado"
            }),
            Some("corrigir decisao".to_string()),
        )
        .unwrap();

        let (title, owner, evidence): (String, String, String) = conn
            .query_row(
                "SELECT title, owner, evidence FROM minute_decisions WHERE id = 'decision-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "Aprovar entrega revisada");
        assert_eq!(owner, "Rafaela");
        assert_eq!(evidence, "orcamento internacional aprovado");

        let (validated, score): (i64, f64) = conn
            .query_row(
                "SELECT validated, validation_score FROM minute_evidences WHERE id = 'evidence-decision-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(validated, 0);
        assert!(score < 0.58);
        assert_eq!(row_count(&conn, "minute_versions"), 2);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn restore_minute_version_record_restores_snapshot_and_versions_current_state() {
        let dir = temp_app_dir("phase-two-restore");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);
        update_minute_action_record(
            &mut conn,
            "action-1",
            serde_json::json!({
                "task": "Enviar resumo alterado",
                "owner": "Caio",
                "status": "done"
            }),
            Some("alterar acao".to_string()),
        )
        .unwrap();

        let version_id: String = conn
            .query_row(
                "SELECT id FROM minute_versions WHERE version_no = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        restore_minute_version_record(&mut conn, &version_id).unwrap();

        let (task, owner, status): (String, String, String) = conn
            .query_row(
                "SELECT task, owner, status FROM minute_actions WHERE id = 'action-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(task, "Enviar resumo");
        assert_eq!(owner, "Maria");
        assert_eq!(status, "pending");
        assert_eq!(row_count(&conn, "minute_versions"), 3);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_minute_revision_record_saves_snapshot_and_marks_minute_edited() {
        let dir = temp_app_dir("phase-two-save-revision");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);

        let version_id = save_minute_revision_record(
            &mut conn,
            "meeting-1",
            Some("revisao aprovada".to_string()),
            None,
        )
        .unwrap();

        let (reason, has_snapshot, minute_id): (String, i64, String) = conn
            .query_row(
                "SELECT change_reason, CASE WHEN snapshot_json IS NULL THEN 0 ELSE 1 END, minute_id
                 FROM minute_versions
                 WHERE id = ?1",
                params![version_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(reason, "revisao aprovada");
        assert_eq!(has_snapshot, 1);
        assert_eq!(minute_id, "minute-1");
        let user_edited: i64 = conn
            .query_row(
                "SELECT user_edited FROM minutes WHERE id = 'minute-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(user_edited, 1);
        assert_eq!(row_count(&conn, "minute_versions"), 2);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn update_minute_participants_record_versions_and_updates_active_names() {
        let dir = temp_app_dir("phase-two-participants");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);

        update_minute_participants_record(
            &mut conn,
            "meeting-1",
            vec![
                " Caio ".to_string(),
                "Rafaela".to_string(),
                "Caio".to_string(),
                "".to_string(),
            ],
            Some("corrigir participantes".to_string()),
        )
        .unwrap();

        let structured = get_structured_minutes_by_meeting_record(&conn, "meeting-1")
            .unwrap()
            .unwrap();
        assert_eq!(
            structured["participantNames"],
            serde_json::json!(["Caio", "Rafaela"])
        );
        assert_eq!(structured["userEdited"], true);
        assert_eq!(row_count(&conn, "minute_versions"), 2);
        let (reason, snapshot): (String, String) = conn
            .query_row(
                "SELECT change_reason, snapshot_json FROM minute_versions WHERE version_no = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(reason, "corrigir participantes");
        assert!(snapshot.contains("Maria"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_minute_action_patch_does_not_create_partial_revision() {
        let dir = temp_app_dir("phase-two-invalid-action");
        let mut conn = init_db(&dir);
        seed_reviewable_minute(&conn);

        let result = update_minute_action_record(
            &mut conn,
            "action-1",
            serde_json::json!({
                "task": "",
                "status": "done"
            }),
            Some("patch invalido".to_string()),
        );

        assert!(result.unwrap_err().contains("task cannot be empty"));
        assert_eq!(row_count(&conn, "minute_versions"), 1);
        let (task, status): (String, String) = conn
            .query_row(
                "SELECT task, status FROM minute_actions WHERE id = 'action-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(task, "Enviar resumo");
        assert_eq!(status, "pending");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn processing_job_upsert_preserves_single_row_per_stage() {
        let dir = temp_app_dir("processing-job-upsert");
        let conn = init_db(&dir);

        upsert_processing_job_record(&conn, "meeting-1", "transcribe", "running", 42, None)
            .unwrap();
        upsert_processing_job_record(&conn, "meeting-1", "transcribe", "done", 100, None).unwrap();

        assert_eq!(row_count(&conn, "processing_jobs"), 1);
        let (status, progress): (String, i64) = conn
            .query_row(
                "SELECT status, progress_pct FROM processing_jobs WHERE meeting_id = 'meeting-1' AND stage = 'transcribe'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "done");
        assert_eq!(progress, 100);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn init_db_migrates_existing_transcriptions_to_speaker_map_column() {
        let dir = temp_app_dir("speaker-map-migration");
        let db_path = dir.join("db.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE transcriptions (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                raw_whisper TEXT,
                diarized TEXT,
                speakers TEXT,
                language TEXT DEFAULT 'pt',
                created_at TEXT NOT NULL
            )",
        )
        .unwrap();
        drop(conn);

        let migrated = init_db(&dir);
        let columns = transcription_columns(&migrated);

        assert!(columns.contains("speaker_map"));

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
