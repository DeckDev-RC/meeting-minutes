use super::*;

pub fn segments_for_chunk(
    segments: &[TranscriptionSegment],
    chunk: &ExportedChunk,
) -> Vec<TranscriptionSegment> {
    let sorted_segments = segments
        .windows(2)
        .all(|pair| pair[0].start <= pair[1].start);
    let first_candidate = if sorted_segments {
        segments.partition_point(|segment| segment.end <= chunk.start_sec)
    } else {
        0
    };

    let candidates = segments[first_candidate..]
        .iter()
        .take_while(|segment| !sorted_segments || segment.start < chunk.end_sec)
        .filter(|segment| segment.end > chunk.start_sec && segment.start < chunk.end_sec);

    candidates
        .filter_map(|segment| {
            let start = (segment.start - chunk.offset_sec).max(0.0);
            let end = (segment.end - chunk.offset_sec).min(chunk.duration_sec);
            if end <= start {
                return None;
            }

            Some(TranscriptionSegment {
                id: segment.id,
                start,
                end,
                text: segment.text.clone(),
            })
        })
        .collect()
}

pub fn shift_diarized_result(mut result: DiarizedResult, offset_sec: f64) -> DiarizedResult {
    for segment in &mut result.segments {
        segment.start += offset_sec;
        segment.end += offset_sec;
    }
    result
}

fn normalized_embedding(embedding: &[f64]) -> Option<Vec<f64>> {
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return None;
    }

    Some(embedding.iter().map(|value| value / norm).collect())
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }

    let left = normalized_embedding(left)?;
    let right = normalized_embedding(right)?;
    Some(
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| left * right)
            .sum(),
    )
}

fn best_centroid_match<'a>(
    local: &SpeakerCentroid,
    global_centroids: &'a [GlobalSpeakerCentroid],
    min_cosine: Option<f64>,
) -> Option<&'a str> {
    global_centroids
        .iter()
        .filter_map(|global| {
            cosine_similarity(&local.embedding, &global.embedding)
                .map(|score| (global.speaker.as_str(), score))
        })
        .filter(|(_, score)| min_cosine.map(|min| *score >= min).unwrap_or(true))
        .max_by(|(_, left), (_, right)| {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(speaker, _)| speaker)
}

fn upsert_global_centroid(
    global_centroids: &mut Vec<GlobalSpeakerCentroid>,
    speaker: &str,
    local_embedding: &[f64],
) {
    let Some(local_embedding) = normalized_embedding(local_embedding) else {
        return;
    };

    if let Some(global) = global_centroids
        .iter_mut()
        .find(|global| global.speaker == speaker)
    {
        if global.embedding.len() != local_embedding.len() {
            return;
        }
        let previous_weight = global.observations.max(1) as f64;
        for (global_value, local_value) in global.embedding.iter_mut().zip(local_embedding) {
            *global_value =
                (*global_value * previous_weight + local_value) / (previous_weight + 1.0);
        }
        if let Some(normalized) = normalized_embedding(&global.embedding) {
            global.embedding = normalized;
        }
        global.observations += 1;
        return;
    }

    global_centroids.push(GlobalSpeakerCentroid {
        speaker: speaker.to_string(),
        embedding: local_embedding,
        observations: 1,
    });
}

fn expected_speaker_name_from_local_label(label: &str, expected_speakers: usize) -> Option<String> {
    if expected_speakers == 0 {
        return None;
    }

    let trimmed = label.trim();
    let index = if let Some(suffix) = trimmed
        .strip_prefix("SPEAKER_")
        .or_else(|| trimmed.strip_prefix("speaker_"))
    {
        suffix.trim_start_matches('0').parse::<usize>().unwrap_or(0) + 1
    } else {
        let digits = trimmed
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        digits.parse::<usize>().ok()?
    };

    if index == 0 || index > expected_speakers {
        return None;
    }

    Some(format!("Falante {index}"))
}

pub fn stitch_diarized_chunk_results(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
) -> DiarizedResult {
    stitch_diarized_chunk_results_internal(chunk_results, overlap_sec, None)
}

pub fn stitch_diarized_chunk_results_with_expected_speakers(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
    expected_speakers: Option<i32>,
) -> DiarizedResult {
    stitch_diarized_chunk_results_internal(
        chunk_results,
        overlap_sec,
        expected_speakers
            .filter(|value| *value > 0)
            .map(|value| value as usize),
    )
}

fn stitch_diarized_chunk_results_internal(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
    expected_speakers: Option<usize>,
) -> DiarizedResult {
    let mut speakers = Vec::new();
    let mut segments: Vec<DiarizedSegment> = Vec::new();

    for result in chunk_results {
        let mut local_to_global: HashMap<String, String> = HashMap::new();
        let mut overlap_scores: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for local in &result.segments {
            let first_candidate = segments.partition_point(|existing| existing.end <= local.start);
            for existing in &segments[first_candidate..] {
                if existing.start >= local.end {
                    break;
                }
                let overlap = overlap_seconds(local.start, local.end, existing.start, existing.end);
                if overlap <= 0.0 {
                    continue;
                }
                *overlap_scores
                    .entry(local.speaker.clone())
                    .or_default()
                    .entry(existing.speaker.clone())
                    .or_insert(0.0) += overlap;
            }
        }

        for (local_speaker, global_scores) in overlap_scores {
            if let Some((global_speaker, _)) = global_scores
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                local_to_global.insert(local_speaker, global_speaker);
            }
        }

        for mut segment in result.segments {
            let original_speaker = segment.speaker.clone();
            let canonical = local_to_global
                .get(&original_speaker)
                .cloned()
                .or_else(|| {
                    expected_speakers.and_then(|expected| {
                        expected_speaker_name_from_local_label(&original_speaker, expected)
                    })
                })
                .unwrap_or_else(|| {
                    let speaker = format!("Falante {}", speakers.len() + 1);
                    local_to_global.insert(original_speaker.clone(), speaker.clone());
                    speaker
                });
            segment.speaker = canonical.clone();

            push_speaker_once(&mut speakers, &canonical);
            if let Some(last) = segments.last_mut() {
                if last.speaker == segment.speaker && segment.start <= last.end + overlap_sec {
                    last.end = last.end.max(segment.end);
                    let text = segment.text.trim();
                    if !text.is_empty() && !last.text.contains(text) {
                        if !last.text.is_empty() {
                            last.text.push(' ');
                        }
                        last.text.push_str(text);
                    }
                    continue;
                }
            }

            segments.push(segment);
        }
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}

pub(super) fn stitch_diarized_chunk_results_with_centroids(
    chunk_results: Vec<DiarizedChunkResult>,
    overlap_sec: f64,
    expected_speakers: Option<i32>,
) -> DiarizedResult {
    let expected_speakers = expected_speakers
        .filter(|value| *value > 0)
        .map(|value| value as usize);
    let mut speakers = Vec::new();
    let mut segments: Vec<DiarizedSegment> = Vec::new();
    let mut global_centroids: Vec<GlobalSpeakerCentroid> = Vec::new();

    for chunk in chunk_results {
        let has_centroids = !chunk.speaker_centroids.is_empty();
        let mut local_to_global: HashMap<String, String> = HashMap::new();
        let mut overlap_scores: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for local in &chunk.diarized.segments {
            let first_candidate = segments.partition_point(|existing| existing.end <= local.start);
            for existing in &segments[first_candidate..] {
                if existing.start >= local.end {
                    break;
                }
                let overlap = overlap_seconds(local.start, local.end, existing.start, existing.end);
                if overlap <= 0.0 {
                    continue;
                }
                *overlap_scores
                    .entry(local.speaker.clone())
                    .or_default()
                    .entry(existing.speaker.clone())
                    .or_insert(0.0) += overlap;
            }
        }

        for (local_speaker, global_scores) in overlap_scores {
            if let Some((global_speaker, _)) = global_scores
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                local_to_global.insert(local_speaker, global_speaker);
            }
        }

        for centroid in &chunk.speaker_centroids {
            if local_to_global.contains_key(&centroid.speaker) {
                continue;
            }

            let speaker_cap_reached = expected_speakers
                .map(|expected| speakers.len() >= expected)
                .unwrap_or(false);
            let min_cosine = if speaker_cap_reached {
                None
            } else {
                Some(0.72)
            };

            if let Some(global_speaker) =
                best_centroid_match(centroid, &global_centroids, min_cosine)
            {
                local_to_global.insert(centroid.speaker.clone(), global_speaker.to_string());
            }
        }

        for mut segment in chunk.diarized.segments {
            let original_speaker = segment.speaker.clone();
            let canonical = local_to_global
                .get(&original_speaker)
                .cloned()
                .or_else(|| {
                    if has_centroids {
                        None
                    } else {
                        expected_speakers.and_then(|expected| {
                            expected_speaker_name_from_local_label(&original_speaker, expected)
                        })
                    }
                })
                .unwrap_or_else(|| {
                    let speaker = format!("Falante {}", speakers.len() + 1);
                    local_to_global.insert(original_speaker.clone(), speaker.clone());
                    speaker
                });
            segment.speaker = canonical.clone();

            push_speaker_once(&mut speakers, &canonical);
            if let Some(last) = segments.last_mut() {
                if last.speaker == segment.speaker && segment.start <= last.end + overlap_sec {
                    last.end = last.end.max(segment.end);
                    let text = segment.text.trim();
                    if !text.is_empty() && !last.text.contains(text) {
                        if !last.text.is_empty() {
                            last.text.push(' ');
                        }
                        last.text.push_str(text);
                    }
                    continue;
                }
            }

            segments.push(segment);
        }

        for centroid in &chunk.speaker_centroids {
            if let Some(global_speaker) = local_to_global.get(&centroid.speaker) {
                upsert_global_centroid(&mut global_centroids, global_speaker, &centroid.embedding);
            }
        }
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}
