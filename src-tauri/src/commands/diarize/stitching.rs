use super::*;

const GLOBAL_CENTROID_MIN_COSINE: f64 = 0.75;

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

fn centroid_group_embedding(
    group: &[usize],
    observations: &[(usize, String, Vec<f64>)],
) -> Option<Vec<f64>> {
    if group.is_empty() {
        return None;
    }

    let first = normalized_embedding(&observations[*group.first()?].2)?;
    let mut centroid = vec![0.0; first.len()];
    for observation_index in group {
        let embedding = normalized_embedding(&observations[*observation_index].2)?;
        if embedding.len() != centroid.len() {
            return None;
        }
        for (target, value) in centroid.iter_mut().zip(embedding) {
            *target += value;
        }
    }
    normalized_embedding(&centroid)
}

fn merge_centroid_groups_by_threshold(
    observations: &[(usize, String, Vec<f64>)],
    min_cosine: f64,
) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = (0..observations.len()).map(|index| vec![index]).collect();
    let mut changed = true;

    while changed {
        changed = false;
        'outer: for left in 0..groups.len() {
            let Some(left_embedding) = centroid_group_embedding(&groups[left], observations) else {
                continue;
            };
            for right in (left + 1)..groups.len() {
                let Some(right_embedding) = centroid_group_embedding(&groups[right], observations)
                else {
                    continue;
                };
                let Some(score) = cosine_similarity(&left_embedding, &right_embedding) else {
                    continue;
                };
                if score >= min_cosine {
                    let merged = groups.remove(right);
                    groups[left].extend(merged);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }

    groups
}

fn cap_centroid_groups(
    mut groups: Vec<Vec<usize>>,
    observations: &[(usize, String, Vec<f64>)],
    expected_speakers: Option<usize>,
) -> Vec<Vec<usize>> {
    let Some(expected_speakers) = expected_speakers.filter(|value| *value > 0) else {
        return groups;
    };

    while groups.len() > expected_speakers {
        let mut best_pair = None;
        let mut best_score = f64::NEG_INFINITY;

        for left in 0..groups.len() {
            let Some(left_embedding) = centroid_group_embedding(&groups[left], observations) else {
                continue;
            };
            for right in (left + 1)..groups.len() {
                let Some(right_embedding) = centroid_group_embedding(&groups[right], observations)
                else {
                    continue;
                };
                let Some(score) = cosine_similarity(&left_embedding, &right_embedding) else {
                    continue;
                };
                if score > best_score {
                    best_pair = Some((left, right));
                    best_score = score;
                }
            }
        }

        let Some((left, right)) = best_pair else {
            break;
        };
        let merged = groups.remove(right);
        groups[left].extend(merged);
    }

    groups
}

pub(super) fn global_centroid_speaker_map(
    chunk_results: &[DiarizedChunkResult],
    expected_speakers: Option<usize>,
) -> HashMap<(usize, String), String> {
    let mut observations = Vec::new();
    for (chunk_index, chunk) in chunk_results.iter().enumerate() {
        for centroid in &chunk.speaker_centroids {
            if normalized_embedding(&centroid.embedding).is_some() {
                observations.push((
                    chunk_index,
                    centroid.speaker.clone(),
                    centroid.embedding.clone(),
                ));
            }
        }
    }
    if observations.is_empty() {
        return HashMap::new();
    }

    let groups = merge_centroid_groups_by_threshold(&observations, GLOBAL_CENTROID_MIN_COSINE);
    let mut groups = cap_centroid_groups(groups, &observations, expected_speakers);
    groups.sort_by_key(|group| group.iter().copied().min().unwrap_or(usize::MAX));

    let mut mapping = HashMap::new();
    for (group_index, group) in groups.iter().enumerate() {
        let speaker = format!("Falante {}", group_index + 1);
        for observation_index in group {
            let (chunk_index, local_speaker, _) = &observations[*observation_index];
            mapping.insert((*chunk_index, local_speaker.clone()), speaker.clone());
        }
    }
    mapping
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
    let centroid_mapping = global_centroid_speaker_map(&chunk_results, expected_speakers);

    for (chunk_index, chunk) in chunk_results.into_iter().enumerate() {
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

        for centroid in &chunk.speaker_centroids {
            if let Some(global_speaker) =
                centroid_mapping.get(&(chunk_index, centroid.speaker.clone()))
            {
                local_to_global.insert(centroid.speaker.clone(), global_speaker.clone());
            }
        }

        for (local_speaker, global_scores) in overlap_scores {
            if local_to_global.contains_key(&local_speaker) {
                continue;
            }
            if let Some((global_speaker, _)) = global_scores
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                local_to_global.insert(local_speaker, global_speaker);
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
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}
