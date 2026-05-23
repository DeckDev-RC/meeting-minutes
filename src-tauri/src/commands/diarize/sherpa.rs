use super::*;
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig, Wave,
};
use std::collections::VecDeque;

struct SherpaDiarizer {
    inner: OfflineSpeakerDiarization,
}

impl SherpaDiarizer {
    fn create(
        assets: &DiarizationAssetPaths,
        expected_speakers: Option<i32>,
        num_threads: i32,
        provider: &str,
    ) -> Result<Self, String> {
        let num_clusters = expected_speakers.filter(|value| *value > 0).unwrap_or(-1);
        let num_threads = num_threads.max(1);
        let provider = provider.to_string();
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(assets.segmentation_model.to_string_lossy().to_string()),
                },
                provider: Some(provider.clone()),
                num_threads,
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(assets.embedding_model.to_string_lossy().to_string()),
                provider: Some(provider),
                num_threads,
                ..Default::default()
            },
            clustering: FastClusteringConfig {
                num_clusters,
                threshold: 0.5,
            },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };

        let inner = OfflineSpeakerDiarization::create(&config)
            .ok_or_else(|| "failed to initialize offline speaker diarization".to_string())?;
        Ok(Self { inner })
    }

    fn process_turns(&self, audio_path: &str) -> Result<Vec<SpeakerTurn>, String> {
        let wave = Wave::read(audio_path).ok_or_else(|| "Failed to read WAV audio".to_string())?;

        if self.inner.sample_rate() != wave.sample_rate() {
            return Err(format!(
                "Unexpected diarization sample rate. Model expects {} Hz, audio has {} Hz",
                self.inner.sample_rate(),
                wave.sample_rate()
            ));
        }

        let result = self
            .inner
            .process(wave.samples())
            .ok_or_else(|| "Offline speaker diarization failed".to_string())?;
        let turns = result
            .sort_by_start_time()
            .into_iter()
            .map(|segment| SpeakerTurn {
                start: segment.start as f64,
                end: segment.end as f64,
                speaker_index: segment.speaker,
            })
            .collect::<Vec<_>>();

        if turns.is_empty() {
            return Err("Offline speaker diarization returned no speaker turns".to_string());
        }

        Ok(turns)
    }
}

fn create_sherpa_diarizer_with_fallback(
    assets: &DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
    provider: Option<String>,
) -> Result<(String, SherpaDiarizer), String> {
    let mut errors = Vec::new();
    for candidate in sherpa_provider_candidates(provider) {
        match SherpaDiarizer::create(assets, expected_speakers, num_threads, &candidate) {
            Ok(diarizer) => return Ok((candidate, diarizer)),
            Err(error) => errors.push(format!("{candidate}: {error}")),
        }
    }

    Err(format!(
        "Failed to initialize sherpa diarization providers: {}",
        errors.join(" | ")
    ))
}

fn speaker_turns_to_diarized_result(turns: Vec<SpeakerTurn>) -> DiarizedResult {
    let mut speakers = Vec::new();
    let mut segments = Vec::new();
    for turn in turns {
        let speaker = format!("Falante {}", turn.speaker_index + 1);
        push_speaker_once(&mut speakers, &speaker);
        segments.push(DiarizedSegment {
            speaker,
            start: turn.start,
            end: turn.end,
            text: String::new(),
        });
    }
    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }
    DiarizedResult { speakers, segments }
}

fn speaker_turns_from_diarized_result(result: DiarizedResult) -> Vec<SpeakerTurn> {
    result
        .segments
        .into_iter()
        .map(|segment| SpeakerTurn {
            start: segment.start,
            end: segment.end,
            speaker_index: speaker_index_from_name(&segment.speaker),
        })
        .collect()
}

#[command]
pub async fn diarize_audio_turns_sherpa_chunked(
    app: tauri::AppHandle,
    audio_chunks: Vec<ExportedChunk>,
    expected_speakers: Option<i32>,
    num_threads: Option<i32>,
    provider: Option<String>,
) -> Result<Vec<SpeakerTurn>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    let assets = ensure_diarization_assets_in_dir(&app_data_dir).await?;
    let available_threads = available_parallelism_count();
    let max_parallel_chunks = num_threads
        .filter(|value| *value > 0)
        .map(|value| value as usize)
        .unwrap_or_else(|| (available_threads / 3).max(1))
        .clamp(1, audio_chunks.len().max(1));
    let threads_per_worker = ((available_threads / max_parallel_chunks).max(1).min(4)) as i32;
    diarize_audio_turns_with_sherpa_chunks(
        audio_chunks,
        assets,
        expected_speakers,
        threads_per_worker,
        max_parallel_chunks,
        requested_sherpa_provider(provider),
    )
    .await
}

pub async fn diarize_audio_by_chunks_with_sherpa(
    audio_chunks: Vec<ExportedChunk>,
    segments: Arc<Vec<TranscriptionSegment>>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
) -> Result<DiarizedResult, String> {
    diarize_audio_by_chunks_with_sherpa_provider(
        audio_chunks,
        segments,
        assets,
        expected_speakers,
        num_threads,
        requested_sherpa_provider(None),
    )
    .await
}

async fn diarize_audio_by_chunks_with_sherpa_provider(
    audio_chunks: Vec<ExportedChunk>,
    segments: Arc<Vec<TranscriptionSegment>>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
    provider: Option<String>,
) -> Result<DiarizedResult, String> {
    if audio_chunks.is_empty() {
        return Ok(diarize_transcription_locally(segments.as_slice()));
    }

    let mut sorted_chunks = audio_chunks;
    sorted_chunks.sort_by(|a, b| {
        a.start_sec
            .partial_cmp(&b.start_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });

    let available_threads = available_parallelism_count();
    let worker_count =
        ((available_threads as i32 / num_threads.max(1)).max(1) as usize).min(sorted_chunks.len());
    let queue = Arc::new(std::sync::Mutex::new(VecDeque::from(sorted_chunks)));
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = queue.clone();
        let segments = segments.clone();
        let assets = assets.clone();
        let provider = provider.clone();
        handles.push(tokio::task::spawn_blocking(move || {
            let mut local_results = Vec::new();
            let mut local_diarizer: Option<SherpaDiarizer> = None;

            loop {
                let chunk = {
                    let mut guard = queue
                        .lock()
                        .map_err(|e| format!("Hybrid diarization queue lock failed: {e}"))?;
                    guard.pop_front()
                };
                let Some(chunk) = chunk else {
                    break;
                };

                let chunk_segments = segments_for_chunk(&segments, &chunk);
                if chunk_segments.is_empty() {
                    continue;
                }

                if local_diarizer.is_none() {
                    let (_provider_used, diarizer) = create_sherpa_diarizer_with_fallback(
                        &assets,
                        expected_speakers,
                        num_threads,
                        provider.clone(),
                    )?;
                    local_diarizer = Some(diarizer);
                }
                let turns = local_diarizer
                    .as_ref()
                    .expect("diarizer initialized")
                    .process_turns(&chunk.audio_path)
                    .map_err(|e| {
                        format!(
                            "Hybrid Sherpa diarization failed for chunk {}: {e}",
                            chunk.index
                        )
                    })?;
                let diarized = diarize_segments_with_speaker_turns(&chunk_segments, &turns);

                local_results.push((
                    chunk.index,
                    shift_diarized_result(diarized, chunk.offset_sec),
                ));
            }

            Ok::<Vec<(usize, DiarizedResult)>, String>(local_results)
        }));
    }

    let mut chunk_results = Vec::new();
    for handle in handles {
        chunk_results.extend(
            handle
                .await
                .map_err(|e| format!("Hybrid diarization join failed: {e}"))??,
        );
    }

    chunk_results.sort_by_key(|(index, _)| *index);
    Ok(stitch_diarized_chunk_results(
        chunk_results
            .into_iter()
            .map(|(_, result)| result)
            .collect(),
        3.0,
    ))
}

async fn diarize_audio_turns_with_sherpa_chunks(
    audio_chunks: Vec<ExportedChunk>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
    max_parallel_chunks: usize,
    provider: Option<String>,
) -> Result<Vec<SpeakerTurn>, String> {
    if audio_chunks.is_empty() {
        return Err("Sherpa chunked diarization requires audio chunks".to_string());
    }
    if !chunks_can_use_sherpa(&audio_chunks) {
        return Err("Sherpa chunked diarization requires WAV chunks".to_string());
    }

    let mut sorted_chunks = audio_chunks;
    sorted_chunks.sort_by(|a, b| {
        a.start_sec
            .partial_cmp(&b.start_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });

    let worker_count = max_parallel_chunks.max(1).min(sorted_chunks.len());
    let queue = Arc::new(std::sync::Mutex::new(VecDeque::from(sorted_chunks)));
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = queue.clone();
        let assets = assets.clone();
        let provider = provider.clone();
        handles.push(tokio::task::spawn_blocking(move || {
            let mut local_results = Vec::new();
            let mut local_diarizer: Option<SherpaDiarizer> = None;

            loop {
                let chunk = {
                    let mut guard = queue
                        .lock()
                        .map_err(|e| format!("Sherpa chunked queue lock failed: {e}"))?;
                    guard.pop_front()
                };
                let Some(chunk) = chunk else {
                    break;
                };

                if local_diarizer.is_none() {
                    let (_provider_used, diarizer) = create_sherpa_diarizer_with_fallback(
                        &assets,
                        expected_speakers,
                        num_threads,
                        provider.clone(),
                    )?;
                    local_diarizer = Some(diarizer);
                }
                let turns = local_diarizer
                    .as_ref()
                    .expect("diarizer initialized")
                    .process_turns(&chunk.audio_path)
                    .map_err(|e| {
                        format!(
                            "Sherpa chunked diarization failed for chunk {}: {e}",
                            chunk.index
                        )
                    })?;
                let diarized = shift_diarized_result(
                    speaker_turns_to_diarized_result(turns),
                    chunk.offset_sec,
                );
                local_results.push((chunk.index, diarized));
            }

            Ok::<Vec<(usize, DiarizedResult)>, String>(local_results)
        }));
    }

    let mut chunk_results = Vec::new();
    for handle in handles {
        chunk_results.extend(
            handle
                .await
                .map_err(|e| format!("Sherpa chunked join failed: {e}"))??,
        );
    }
    chunk_results.sort_by_key(|(index, _)| *index);
    let stitched = stitch_diarized_chunk_results_with_expected_speakers(
        chunk_results
            .into_iter()
            .map(|(_, result)| result)
            .collect(),
        3.0,
        expected_speakers,
    );
    let turns = speaker_turns_from_diarized_result(stitched);
    if turns.is_empty() {
        return Err("Sherpa chunked diarization returned no speaker turns".to_string());
    }

    Ok(turns)
}

pub fn diarize_audio_with_sherpa(
    audio_path: String,
    segments: &[TranscriptionSegment],
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
) -> Result<DiarizedResult, String> {
    let (_provider_used, diarizer) = create_sherpa_diarizer_with_fallback(
        &assets,
        expected_speakers,
        num_threads,
        requested_sherpa_provider(None),
    )?;
    let turns = diarizer.process_turns(&audio_path)?;
    Ok(diarize_segments_with_speaker_turns(segments, &turns))
}
