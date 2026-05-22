pub mod benchmark;
pub mod commands;
pub mod models;

use commands::db::{init_db, DbState};
use tauri::Manager;
use tauri_plugin_store::StoreExt;

pub struct HttpClientState(pub reqwest::Client);

fn stored_string_or_env(
    store: &tauri_plugin_store::Store<tauri::Wry>,
    store_key: &str,
    env_key: &str,
) -> String {
    store
        .get(store_key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var(env_key).ok())
        .unwrap_or_default()
}

fn build_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(300))
        .build()
}

#[tauri::command]
fn get_api_keys(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    let groq = stored_string_or_env(&store, "groq_api_key", "GROQ_API_KEY");
    let gemini = stored_string_or_env(&store, "gemini_api_key", "GEMINI_API_KEY");
    let cloudflare_account_id =
        stored_string_or_env(&store, "cloudflare_account_id", "CLOUDFLARE_ACCOUNT_ID");
    let cloudflare_api_token =
        stored_string_or_env(&store, "cloudflare_api_token", "CLOUDFLARE_API_TOKEN");
    let deepgram_api_key = stored_string_or_env(&store, "deepgram_api_key", "DEEPGRAM_API_KEY");
    let transcription_profile = store
        .get("transcription_profile")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "smart-low-cost".to_string());
    let manual_transcription_provider = store
        .get("manual_transcription_provider")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "groq".to_string());
    let expected_speakers = store
        .get("expected_speakers")
        .and_then(|v| v.as_i64())
        .filter(|value| *value > 0)
        .map(|value| value as i32);
    Ok(serde_json::json!({
        "groq": groq,
        "gemini": gemini,
        "cloudflareAccountId": cloudflare_account_id,
        "cloudflareApiToken": cloudflare_api_token,
        "deepgramApiKey": deepgram_api_key,
        "transcriptionProfile": transcription_profile,
        "manualTranscriptionProvider": manual_transcription_provider,
        "expectedSpeakers": expected_speakers
    }))
}

#[tauri::command]
fn set_api_keys(
    app: tauri::AppHandle,
    groq: String,
    gemini: String,
    cloudflare_account_id: String,
    cloudflare_api_token: String,
    deepgram_api_key: String,
    transcription_profile: Option<String>,
    manual_transcription_provider: Option<String>,
    expected_speakers: Option<i32>,
) -> Result<(), String> {
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("groq_api_key", serde_json::Value::String(groq));
    store.set("gemini_api_key", serde_json::Value::String(gemini));
    store.set(
        "cloudflare_account_id",
        serde_json::Value::String(cloudflare_account_id),
    );
    store.set(
        "cloudflare_api_token",
        serde_json::Value::String(cloudflare_api_token),
    );
    store.set(
        "deepgram_api_key",
        serde_json::Value::String(deepgram_api_key),
    );
    store.set(
        "transcription_profile",
        serde_json::Value::String(
            transcription_profile.unwrap_or_else(|| "smart-low-cost".to_string()),
        ),
    );
    store.set(
        "manual_transcription_provider",
        serde_json::Value::String(
            manual_transcription_provider.unwrap_or_else(|| "groq".to_string()),
        ),
    );
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
            app.manage(HttpClientState(
                build_http_client().map_err(|e| e.to_string())?,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::audio::extract_audio,
            commands::audio::prepare_audio_and_chunks,
            commands::audio::probe_media_metadata,
            commands::audio::chunk_audio,
            commands::audio::detect_silences,
            commands::audio::create_smart_chunks,
            commands::transcribe::transcribe_chunk,
            commands::transcribe::transcribe_chunk_cloudflare,
            commands::transcribe::transcribe_chunk_deepgram,
            commands::transcribe::transcribe_chunk_local,
            commands::transcribe::parakeet::transcribe_chunks_parakeet_local,
            commands::transcribe::transcribe_chunks_local,
            commands::transcribe::check_local_transcription_backends,
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
            commands::storage::resolve_processing_work_dir,
            commands::storage::save_pdf,
            commands::storage::save_benchmark_run,
            commands::storage::open_folder,
            get_api_keys,
            set_api_keys,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tuned_http_client() {
        assert!(build_http_client().is_ok());
    }
}
