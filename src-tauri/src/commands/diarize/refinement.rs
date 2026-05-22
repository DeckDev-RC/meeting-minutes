use super::*;

fn chunk_path_is_wav(chunk: &ExportedChunk) -> bool {
    Path::new(&chunk.audio_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

pub(super) fn chunks_can_use_sherpa(chunks: &[ExportedChunk]) -> bool {
    !chunks.is_empty() && chunks.iter().all(chunk_path_is_wav)
}

fn chunk_for_segment<'a>(
    segment: &TranscriptionSegment,
    chunks: &'a [ExportedChunk],
) -> Option<&'a ExportedChunk> {
    let midpoint = (segment.start + segment.end) / 2.0;
    chunks
        .iter()
        .find(|chunk| midpoint >= chunk.start_sec && midpoint <= chunk.end_sec)
        .or_else(|| {
            chunks
                .iter()
                .find(|chunk| segment.end > chunk.start_sec && segment.start < chunk.end_sec)
        })
}

fn segment_overlap_stats(segment: &TranscriptionSegment, turns: &[SpeakerTurn]) -> (f64, f64) {
    let mut best = 0.0;
    let mut second = 0.0;
    let first_candidate = turns.partition_point(|turn| turn.end <= segment.start);

    for turn in &turns[first_candidate..] {
        if turn.start >= segment.end {
            break;
        }
        let overlap = overlap_seconds(segment.start, segment.end, turn.start, turn.end);
        if overlap > best {
            second = best;
            best = overlap;
        } else if overlap > second {
            second = overlap;
        }
    }

    (best, second)
}

fn suspicion_score_for_segment(segment: &TranscriptionSegment, turns: &[SpeakerTurn]) -> f64 {
    let duration = (segment.end - segment.start).max(0.0);
    if duration <= 0.0 {
        return 0.0;
    }

    let (best_overlap, second_overlap) = segment_overlap_stats(segment, turns);
    let coverage_ratio = best_overlap / duration;
    let ambiguity_ratio = second_overlap / duration;
    let mut score = 0.0;

    if coverage_ratio < 0.45 {
        score += 3.0;
    } else if coverage_ratio < 0.7 {
        score += 1.5;
    }

    if ambiguity_ratio >= 0.25 {
        score += 2.0;
    }

    if duration >= 18.0 && segment.text.contains('?') {
        score += 0.75;
    }

    score
}

fn segments_for_refinement_chunk<'a>(
    segments: &'a [TranscriptionSegment],
    chunk: &ExportedChunk,
) -> Vec<&'a TranscriptionSegment> {
    let first_candidate = segments.partition_point(|segment| segment.end <= chunk.start_sec);
    segments[first_candidate..]
        .iter()
        .take_while(|segment| segment.start < chunk.end_sec)
        .filter(|segment| segment.end > chunk.start_sec)
        .collect()
}

fn chunk_context_suspicion_score(
    chunk_segments: &[&TranscriptionSegment],
    turns: &[SpeakerTurn],
    expected_speakers: Option<i32>,
) -> f64 {
    if chunk_segments.len() < 3 || turns.is_empty() {
        return 0.0;
    }

    let assigned_speakers = best_speakers_for_segment_refs_sweep(chunk_segments, turns);
    let unique_speakers = assigned_speakers.iter().copied().collect::<BTreeSet<_>>();
    let switches = assigned_speakers
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count();
    let switch_ratio = switches as f64 / (assigned_speakers.len().saturating_sub(1).max(1) as f64);
    let mut score = 0.0;

    if chunk_segments.len() >= 6 && switch_ratio >= 0.55 {
        score += 3.0 + switch_ratio;
    }

    if expected_speakers.unwrap_or(0) >= 2 && unique_speakers.len() <= 1 {
        let speech_span = chunk_segments.last().map(|last| last.end).unwrap_or(0.0)
            - chunk_segments
                .first()
                .map(|first| first.start)
                .unwrap_or(0.0);
        if speech_span >= 30.0 {
            score += 2.75;
        }
    }

    score
}

fn refinement_window_for_segments(
    chunk: &ExportedChunk,
    segments: &[&TranscriptionSegment],
) -> Option<RefinementWindow> {
    const PADDING_SEC: f64 = 4.0;
    const MAX_WINDOW_SEC: f64 = 45.0;

    let first = segments.first()?;
    let last = segments.last()?;
    let mut start_sec = (first.start - PADDING_SEC).max(chunk.start_sec);
    let mut end_sec = (last.end + PADDING_SEC).min(chunk.end_sec);
    if end_sec <= start_sec {
        return None;
    }

    if end_sec - start_sec > MAX_WINDOW_SEC {
        let midpoint = (start_sec + end_sec) / 2.0;
        start_sec = (midpoint - MAX_WINDOW_SEC / 2.0).max(chunk.start_sec);
        end_sec = (start_sec + MAX_WINDOW_SEC).min(chunk.end_sec);
        start_sec = (end_sec - MAX_WINDOW_SEC).max(chunk.start_sec);
    }

    Some(RefinementWindow {
        chunk: chunk.clone(),
        start_sec,
        end_sec,
    })
}

pub fn select_suspicious_chunks_for_refinement(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_chunks: usize,
) -> Vec<ExportedChunk> {
    if segments.is_empty() || chunks.is_empty() || max_chunks == 0 || !chunks_can_use_sherpa(chunks)
    {
        return Vec::new();
    }

    let mut scores: HashMap<usize, f64> = HashMap::new();
    for segment in segments {
        let score = suspicion_score_for_segment(segment, turns);
        if score <= 0.0 {
            continue;
        }

        if let Some(chunk) = chunk_for_segment(segment, chunks) {
            *scores.entry(chunk.index).or_insert(0.0) += score;
        }
    }

    let mut ranked = chunks
        .iter()
        .filter_map(|chunk| {
            scores
                .get(&chunk.index)
                .copied()
                .filter(|score| *score > 0.0)
                .map(|score| (score, chunk.clone()))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(a_score, a_chunk), (b_score, b_chunk)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a_chunk.index.cmp(&b_chunk.index))
    });

    ranked
        .into_iter()
        .take(max_chunks)
        .map(|(_, chunk)| chunk)
        .collect()
}

pub fn select_suspicious_refinement_windows(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_windows: usize,
) -> Vec<RefinementWindow> {
    select_suspicious_refinement_windows_with_context(segments, turns, chunks, max_windows, None)
}

pub fn select_suspicious_refinement_windows_with_context(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_windows: usize,
    expected_speakers: Option<i32>,
) -> Vec<RefinementWindow> {
    const PADDING_SEC: f64 = 4.0;
    const MAX_WINDOW_SEC: f64 = 45.0;

    if segments.is_empty()
        || chunks.is_empty()
        || max_windows == 0
        || !chunks_can_use_sherpa(chunks)
    {
        return Vec::new();
    }

    let mut ranked = Vec::new();
    for chunk in chunks {
        let chunk_segments = segments_for_refinement_chunk(segments, chunk);
        let score = chunk_context_suspicion_score(&chunk_segments, turns, expected_speakers);
        if score <= 0.0 {
            continue;
        }
        if let Some(window) = refinement_window_for_segments(chunk, &chunk_segments) {
            ranked.push((score, window));
        }
    }

    for segment in segments {
        let score = suspicion_score_for_segment(segment, turns);
        if score <= 0.0 {
            continue;
        }

        let Some(chunk) = chunk_for_segment(segment, chunks) else {
            continue;
        };

        let mut start_sec = (segment.start - PADDING_SEC).max(chunk.start_sec);
        let mut end_sec = (segment.end + PADDING_SEC).min(chunk.end_sec);
        if end_sec <= start_sec {
            continue;
        }

        if end_sec - start_sec > MAX_WINDOW_SEC {
            let midpoint = (segment.start + segment.end) / 2.0;
            start_sec = (midpoint - MAX_WINDOW_SEC / 2.0).max(chunk.start_sec);
            end_sec = (start_sec + MAX_WINDOW_SEC).min(chunk.end_sec);
            start_sec = (end_sec - MAX_WINDOW_SEC).max(chunk.start_sec);
        }

        ranked.push((
            score,
            RefinementWindow {
                chunk: chunk.clone(),
                start_sec,
                end_sec,
            },
        ));
    }

    ranked.sort_by(|(a_score, a_window), (b_score, b_window)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a_window
                    .start_sec
                    .partial_cmp(&b_window.start_sec)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut windows: Vec<RefinementWindow> = Vec::new();
    for (_, window) in ranked {
        if windows.iter().any(|existing| {
            existing.chunk.index == window.chunk.index
                && window.start_sec < existing.end_sec
                && window.end_sec > existing.start_sec
        }) {
            continue;
        }
        windows.push(window);
        if windows.len() >= max_windows {
            break;
        }
    }

    windows
}

fn segment_overlaps_refined_chunks(segment: &DiarizedSegment, chunks: &[ExportedChunk]) -> bool {
    chunks
        .iter()
        .any(|chunk| segment.end > chunk.start_sec && segment.start < chunk.end_sec)
}

pub fn merge_selective_refinement(
    base: DiarizedResult,
    refined: DiarizedResult,
    refined_chunks: &[ExportedChunk],
) -> DiarizedResult {
    if refined_chunks.is_empty() || refined.segments.is_empty() {
        return base;
    }

    let mut segments = base
        .segments
        .into_iter()
        .filter(|segment| !segment_overlaps_refined_chunks(segment, refined_chunks))
        .collect::<Vec<_>>();
    segments.extend(refined.segments);
    segments.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.end
                    .partial_cmp(&b.end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut speakers = Vec::new();
    for segment in &segments {
        push_speaker_once(&mut speakers, &segment.speaker);
    }
    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}
