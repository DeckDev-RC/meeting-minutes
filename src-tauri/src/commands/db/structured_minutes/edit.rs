use super::{
    encode_participant_names_json, get_structured_minutes_by_meeting_record,
    latest_structured_minute, next_minute_version_no, normalize_participant_names,
};
use crate::commands::db::DbState;
use crate::commands::minutes_validator::validate_evidence_against_segments_json;
use rusqlite::{params, Connection};
use tauri::command;
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

pub(in crate::commands::db) fn update_minute_action_record(
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

pub(in crate::commands::db) fn update_minute_decision_record(
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

pub(in crate::commands::db) fn restore_minute_version_record(
    db: &mut Connection,
    version_id: &str,
) -> Result<(), String> {
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

pub(in crate::commands::db) fn save_minute_revision_record(
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

pub(in crate::commands::db) fn update_minute_participants_record(
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
