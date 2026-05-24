use rusqlite::Connection;

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

pub(super) fn migrate_processing_chunks_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
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

pub(super) fn migrate_processing_chunk_fact_cache(
    conn: &Connection,
) -> Result<(), rusqlite::Error> {
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

pub(super) fn migrate_meetings_metadata(conn: &Connection) -> Result<(), rusqlite::Error> {
    add_meeting_column_if_missing(conn, "participants_hint", "participants_hint TEXT")?;
    add_meeting_column_if_missing(
        conn,
        "processing_profile",
        "processing_profile TEXT NOT NULL DEFAULT 'balanced'",
    )?;
    add_meeting_column_if_missing(conn, "transcription_profile", "transcription_profile TEXT")?;
    Ok(())
}

pub(super) fn migrate_transcriptions_speaker_map(conn: &Connection) -> Result<(), rusqlite::Error> {
    add_transcription_column_if_missing(conn, "speaker_map", "speaker_map TEXT")?;
    Ok(())
}

pub(super) fn migrate_structured_minutes_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
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
