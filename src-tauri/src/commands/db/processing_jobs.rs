use super::DbState;
use rusqlite::{params, Connection};
use tauri::command;

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

pub(super) fn upsert_processing_job_record(
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

pub(super) fn finalize_processing_jobs_for_meeting_record(
    db: &Connection,
    meeting_id: &str,
    meeting_status: &str,
    now_rfc3339: &str,
) -> Result<usize, String> {
    match meeting_status {
        "done" => db
            .execute(
                "UPDATE processing_jobs
                 SET status = 'done',
                     progress_pct = 100,
                     error_msg = NULL,
                     finished_at = COALESCE(finished_at, ?1),
                     updated_at = ?1
                 WHERE meeting_id = ?2 AND status IN ('pending', 'running')",
                params![now_rfc3339, meeting_id],
            )
            .map_err(|e| e.to_string()),
        "error" => db
            .execute(
                "UPDATE processing_jobs
                 SET status = 'error',
                     progress_pct = CASE WHEN progress_pct >= 100 THEN 99 ELSE progress_pct END,
                     error_msg = COALESCE(error_msg, 'Processamento interrompido; use Retomar para continuar.'),
                     finished_at = COALESCE(finished_at, ?1),
                     updated_at = ?1
                 WHERE meeting_id = ?2 AND status IN ('pending', 'running')",
                params![now_rfc3339, meeting_id],
            )
            .map_err(|e| e.to_string()),
        _ => Ok(0),
    }
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

pub(super) fn reap_stale_processing_jobs_record(
    db: &Connection,
    stale_after_minutes: i64,
    now_rfc3339: &str,
) -> Result<usize, String> {
    let stale_after_minutes = stale_after_minutes.max(5);
    let now = chrono::DateTime::parse_from_rfc3339(now_rfc3339)
        .map_err(|e| format!("invalid reaper timestamp: {e}"))?
        .with_timezone(&chrono::Utc);
    let cutoff = now - chrono::Duration::minutes(stale_after_minutes);
    let cutoff = cutoff.to_rfc3339();
    let now = now.to_rfc3339();

    let done_resolved = db
        .execute(
            "UPDATE processing_jobs
             SET status = 'done',
                 progress_pct = 100,
                 error_msg = NULL,
                 finished_at = COALESCE(finished_at, ?1),
                 updated_at = ?1
             WHERE status IN ('pending', 'running')
               AND meeting_id IN (SELECT id FROM meetings WHERE status = 'done')",
            params![&now],
        )
        .map_err(|e| e.to_string())?;

    let error_resolved = db
        .execute(
            "UPDATE processing_jobs
             SET status = 'error',
                 progress_pct = CASE WHEN progress_pct >= 100 THEN 99 ELSE progress_pct END,
                 error_msg = COALESCE(error_msg, 'Processamento interrompido; use Retomar para continuar.'),
                 finished_at = COALESCE(finished_at, ?1),
                 updated_at = ?1
             WHERE status IN ('pending', 'running')
               AND meeting_id IN (SELECT id FROM meetings WHERE status = 'error')",
            params![&now],
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = db
        .prepare(
            "SELECT DISTINCT meeting_id
             FROM processing_jobs
             WHERE status IN ('pending', 'running')
               AND updated_at < ?1
               AND meeting_id NOT IN (SELECT id FROM meetings WHERE status IN ('done', 'error'))",
        )
        .map_err(|e| e.to_string())?;
    let affected_meetings = stmt
        .query_map(params![&cutoff], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let reaped = db
        .execute(
            "UPDATE processing_jobs
             SET status = 'error',
                 progress_pct = CASE WHEN progress_pct >= 100 THEN 99 ELSE progress_pct END,
                 error_msg = COALESCE(error_msg, 'Processamento interrompido; use Retomar para continuar.'),
                 finished_at = ?1,
                 updated_at = ?1
             WHERE status IN ('pending', 'running') AND updated_at < ?2",
            params![&now, &cutoff],
        )
        .map_err(|e| e.to_string())?;

    for meeting_id in affected_meetings {
        db.execute(
            "UPDATE meetings
             SET status = 'error', updated_at = ?1
             WHERE id = ?2 AND status = 'processing'",
            params![&now, meeting_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(done_resolved + error_resolved + reaped)
}

#[command]
pub fn reap_stale_processing_jobs(
    state: tauri::State<'_, DbState>,
    stale_after_minutes: Option<i64>,
) -> Result<usize, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    reap_stale_processing_jobs_record(
        &db,
        stale_after_minutes.unwrap_or(90),
        &chrono::Utc::now().to_rfc3339(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::db::init_db;
    use rusqlite::params;

    #[test]
    fn reaper_marks_stale_running_jobs_and_meetings_as_error() {
        let dir =
            std::env::temp_dir().join(format!("meeting-minutes-reaper-{}", uuid::Uuid::new_v4()));
        let conn = init_db(&dir);
        conn.execute(
            "INSERT INTO meetings (id, title, file_path, status, created_at, updated_at)
             VALUES ('meeting-1', 'Teste', 'a.mp4', 'processing', '2026-05-25T10:00:00Z', '2026-05-25T10:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO processing_jobs
                (id, meeting_id, stage, status, progress_pct, error_msg, started_at, finished_at, created_at, updated_at)
             VALUES ('job-1', 'meeting-1', 'transcribe', 'running', 45, NULL, NULL, NULL, '2026-05-25T10:00:00Z', ?1)",
            params!["2026-05-25T10:00:00Z"],
        )
        .unwrap();

        let reaped = reap_stale_processing_jobs_record(&conn, 60, "2026-05-25T12:30:00Z").unwrap();

        assert_eq!(reaped, 1);
        let status: String = conn
            .query_row(
                "SELECT status FROM processing_jobs WHERE id = 'job-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "error");
        let meeting_status: String = conn
            .query_row(
                "SELECT status FROM meetings WHERE id = 'meeting-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(meeting_status, "error");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reaper_resolves_running_jobs_for_completed_meetings_as_done() {
        let dir =
            std::env::temp_dir().join(format!("meeting-minutes-reaper-{}", uuid::Uuid::new_v4()));
        let conn = init_db(&dir);
        conn.execute(
            "INSERT INTO meetings (id, title, file_path, status, created_at, updated_at)
             VALUES ('meeting-1', 'Teste', 'a.mp4', 'done', '2026-05-25T10:00:00Z', '2026-05-25T10:20:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO processing_jobs
                (id, meeting_id, stage, status, progress_pct, error_msg, started_at, finished_at, created_at, updated_at)
             VALUES ('job-1', 'meeting-1', 'generate', 'running', 97, NULL, NULL, NULL, '2026-05-25T10:00:00Z', ?1)",
            params!["2026-05-25T10:20:00Z"],
        )
        .unwrap();

        let reaped = reap_stale_processing_jobs_record(&conn, 60, "2026-05-25T10:30:00Z").unwrap();

        assert_eq!(reaped, 1);
        let row: (String, i64, Option<String>) = conn
            .query_row(
                "SELECT status, progress_pct, error_msg FROM processing_jobs WHERE id = 'job-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("done".to_string(), 100, None));

        std::fs::remove_dir_all(dir).ok();
    }
}
