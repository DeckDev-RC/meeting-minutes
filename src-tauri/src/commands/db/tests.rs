use super::structured_minutes::edit::{
    restore_minute_version_record, save_minute_revision_record, update_minute_action_record,
    update_minute_decision_record, update_minute_participants_record,
};
use super::structured_minutes::{
    get_minute_evidences_record, get_structured_minutes_by_meeting_record,
    persist_structured_minutes,
};
use super::*;
use crate::models::transcription::{MeetingAction, MeetingChunkInsights, MeetingDecision};
use rusqlite::{params, Connection};
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

fn index_names(conn: &Connection) -> HashSet<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
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
fn init_db_enables_wal_for_file_database() {
    let dir = temp_app_dir("wal-mode");
    let conn = init_db(&dir);

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();

    assert_eq!(journal_mode.to_lowercase(), "wal");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn init_db_creates_lookup_indexes_for_meeting_queries() {
    let dir = temp_app_dir("meeting-query-indexes");
    let conn = init_db(&dir);

    let indexes = index_names(&conn);

    assert!(indexes.contains("idx_meetings_created"));
    assert!(indexes.contains("idx_transcriptions_meeting"));
    assert!(indexes.contains("idx_minutes_meeting_created"));
    assert!(indexes.contains("idx_jobs_meeting"));
    assert!(indexes.contains("idx_processing_jobs_meeting"));

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn get_meetings_record_supports_limit_and_offset() {
    let dir = temp_app_dir("meetings-pagination");
    let conn = init_db(&dir);

    for (id, created_at) in [
        ("meeting-1", "2026-05-24T10:00:00Z"),
        ("meeting-2", "2026-05-24T11:00:00Z"),
        ("meeting-3", "2026-05-24T12:00:00Z"),
    ] {
        conn.execute(
            "INSERT INTO meetings
                (id, title, file_path, processing_profile, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'balanced', 'done', ?4, ?4)",
            params![id, id, format!("{id}.wav"), created_at],
        )
        .unwrap();
    }

    let page = meetings::get_meetings_record(&conn, Some(2), Some(1)).unwrap();

    assert_eq!(
        page.into_iter()
            .map(|meeting| meeting.id)
            .collect::<Vec<_>>(),
        vec!["meeting-2".to_string(), "meeting-1".to_string()]
    );

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
        topic_evidence: vec![],
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
fn structured_minutes_persistence_purges_items_with_invalid_evidence() {
    let dir = temp_app_dir("structured-minutes-purge-invalid");
    let mut conn = init_db(&dir);
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
    let facts = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 60.0,
        summary: "Alinhamento final.".to_string(),
        topics: vec!["Entrega".to_string()],
        topic_evidence: vec![],
        decisions: vec![
            MeetingDecision {
                title: "Aprovar entrega".to_string(),
                owner: "Caio".to_string(),
                timestamp_sec: 3.0,
                evidence: "Caio aprovou a entrega final".to_string(),
            },
            MeetingDecision {
                title: "Cortar escopo".to_string(),
                owner: "Equipe".to_string(),
                timestamp_sec: 8.0,
                evidence: "essa frase nunca apareceu na transcricao".to_string(),
            },
        ],
        actions: vec![
            MeetingAction {
                task: "Enviar resumo".to_string(),
                owner: "Maria".to_string(),
                deadline: "hoje".to_string(),
                timestamp_sec: 7.0,
                evidence: "Maria envia o resumo revisado hoje".to_string(),
            },
            MeetingAction {
                task: "Contratar fornecedor".to_string(),
                owner: "Financeiro".to_string(),
                deadline: "sexta".to_string(),
                timestamp_sec: 12.0,
                evidence: "fornecedor externo foi aprovado por todos".to_string(),
            },
        ],
        questions: vec![],
        risks: vec![],
    }];

    let tx = conn.transaction().unwrap();
    persist_structured_minutes(&tx, "minute-1", "meeting-1", &facts, "now").unwrap();
    tx.commit().unwrap();

    assert_eq!(row_count(&conn, "minute_decisions"), 1);
    assert_eq!(row_count(&conn, "minute_actions"), 1);
    assert_eq!(row_count(&conn, "minute_evidences"), 2);
    let title: String = conn
        .query_row("SELECT title FROM minute_decisions", [], |row| row.get(0))
        .unwrap();
    let task: String = conn
        .query_row("SELECT task FROM minute_actions", [], |row| row.get(0))
        .unwrap();
    let weak_evidences: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM minute_evidences WHERE validated = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "Aprovar entrega");
    assert_eq!(task, "Enviar resumo");
    assert_eq!(weak_evidences, 0);

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
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, purge_summary_json, created_at)
             VALUES ('minute-2', 'meeting-1', '<h1>Atual</h1>', 'ata.pdf', 'gemini-2.5-flash', ?1, '2026-05-23T11:00:00Z')",
            params![serde_json::json!({
                "removedTopics": 1,
                "removedDecisions": 1,
                "removedActions": 1,
                "removedTotal": 3
            })
            .to_string()],
        )
        .unwrap();
    conn.execute(
            "INSERT INTO minutes (id, meeting_id, html_content, pdf_path, model_used, created_at)
             VALUES ('minute-3', 'meeting-with-version-purge', '<h1>Atual</h1>', NULL, 'gemini-2.5-flash', '2026-05-23T12:00:00Z')",
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
            "INSERT INTO minute_versions
                (id, minute_id, meeting_id, version_no, html_content, facts_json, diarized_json, purge_summary_json, created_at)
             VALUES ('version-purge', 'minute-3', 'meeting-with-version-purge', 1, '<h1>Atual</h1>', '[]', '{}', ?1, '2026-05-23T12:00:00Z')",
            params![serde_json::json!({
                "removedTopics": 0,
                "removedDecisions": 0,
                "removedActions": 1,
                "removedTotal": 1
            })
            .to_string()],
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
    assert_eq!(structured["purgeSummary"]["removedTotal"], 3);
    assert_eq!(structured["purgeSummary"]["removedTopics"], 1);
    assert_eq!(structured["purgeSummary"]["removedDecisions"], 1);
    assert_eq!(structured["purgeSummary"]["removedActions"], 1);
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

    let version_purge =
        get_structured_minutes_by_meeting_record(&conn, "meeting-with-version-purge")
            .unwrap()
            .unwrap();
    assert_eq!(version_purge["purgeSummary"]["removedTotal"], 1);
    assert_eq!(version_purge["purgeSummary"]["removedTopics"], 0);
    assert_eq!(version_purge["purgeSummary"]["removedActions"], 1);

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
    assert!(minutes.contains("purge_summary_json"));
    assert!(versions.contains("purge_summary_json"));
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

    processing_jobs::upsert_processing_job_record(
        &conn,
        "meeting-1",
        "transcribe",
        "running",
        42,
        None,
    )
    .unwrap();
    processing_jobs::upsert_processing_job_record(
        &conn,
        "meeting-1",
        "transcribe",
        "done",
        100,
        None,
    )
    .unwrap();

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
