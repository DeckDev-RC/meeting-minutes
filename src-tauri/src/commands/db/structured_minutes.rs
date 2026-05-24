use super::DbState;
use crate::commands::minutes_validator::validate_evidence_against_segments_json;
use crate::models::transcription::{MeetingAction, MeetingChunkInsights, MeetingDecision};
use rusqlite::{params, Connection};
use tauri::command;

pub mod edit;
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

pub(super) fn persist_structured_minutes(
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

pub(super) fn get_structured_minutes_by_meeting_record(
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

pub(super) fn get_minute_evidences_record(
    db: &Connection,
    meeting_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let Some(minute) = latest_structured_minute(db, meeting_id)? else {
        return Ok(Vec::new());
    };

    minute_evidences_for_minute(db, &minute.id)
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
