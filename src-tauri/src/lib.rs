pub mod benchmark;
pub mod commands;
pub mod models;

use commands::db::{init_db, DbState};
use tauri::Manager;
use tauri_plugin_store::StoreExt;

pub struct HttpClientState(pub reqwest::Client);

#[tauri::command]
fn get_api_keys(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    let groq = store
        .get("groq_api_key")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let gemini = store
        .get("gemini_api_key")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let expected_speakers = store
        .get("expected_speakers")
        .and_then(|v| v.as_i64())
        .filter(|value| *value > 0)
        .map(|value| value as i32);
    Ok(serde_json::json!({
        "groq": groq,
        "gemini": gemini,
        "expectedSpeakers": expected_speakers
    }))
}

#[tauri::command]
fn set_api_keys(
    app: tauri::AppHandle,
    groq: String,
    gemini: String,
    expected_speakers: Option<i32>,
) -> Result<(), String> {
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("groq_api_key", serde_json::Value::String(groq));
    store.set("gemini_api_key", serde_json::Value::String(gemini));
    if let Some(expected_speakers) = expected_speakers.filter(|value| *value > 0) {
        store.set(
            "expected_speakers",
            serde_json::Value::Number(serde_json::Number::from(expected_speakers)),
        );
    } else {
        store.delete("expected_speakers");
    }
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");
            let conn = init_db(&app_data_dir);
            app.manage(DbState(std::sync::Mutex::new(conn)));
            app.manage(HttpClientState(reqwest::Client::new()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::audio::extract_audio,
            commands::audio::probe_media_metadata,
            commands::audio::chunk_audio,
            commands::audio::detect_silences,
            commands::audio::create_smart_chunks,
            commands::transcribe::transcribe_chunk,
            commands::diarize::align_speaker_turns_to_transcription,
            commands::diarize::diarize_audio_turns_modern_cpu,
            commands::diarize::diarize_audio_turns_modern_cpu_chunked,
            commands::diarize::diarize_audio_turns_pyannote,
            commands::diarize::diarize_transcription_end_to_end,
            commands::diarize::diarize_transcription_fast,
            commands::diarize::diarize_transcription,
            commands::diarize::refine_diarization_selectively,
            commands::generate::generate_ata_html,
            commands::generate::extract_chunk_facts,
            commands::generate::extract_fact_batch,
            commands::generate::generate_ata_from_facts,
            commands::generate::generate_ata_from_facts_streaming,
            commands::db::save_meeting,
            commands::db::get_meetings,
            commands::db::update_meeting_status,
            commands::db::save_processing_chunks,
            commands::db::get_processing_chunks,
            commands::db::update_processing_chunk_result,
            commands::db::update_processing_chunk_facts,
            commands::db::save_transcription,
            commands::db::save_minutes,
            commands::db::get_minutes_by_meeting,
            commands::db::delete_meeting,
            commands::storage::save_pdf,
            commands::storage::save_benchmark_run,
            commands::storage::open_folder,
            get_api_keys,
            set_api_keys,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
