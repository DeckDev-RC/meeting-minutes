use super::*;
use crate::models::audio::SilenceRange;
use std::sync::mpsc;
use std::time::Duration;

const EPSILON: f64 = 1e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "expected {actual} to be within {EPSILON} of {expected}"
    );
}

fn plan_with_timeout(
    duration_sec: f64,
    silences: Vec<SilenceRange>,
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
) -> Option<Vec<ChunkPlan>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let chunks = plan_smart_chunks(
            duration_sec,
            &silences,
            target_sec,
            min_sec,
            max_sec,
            overlap_sec,
        );
        let _ = tx.send(chunks);
    });

    rx.recv_timeout(Duration::from_millis(200)).ok()
}

#[test]
fn validates_chunk_output_format_whitelist() {
    assert_eq!(validate_chunk_output_format("").unwrap(), "flac");
    assert_eq!(validate_chunk_output_format("FLAC").unwrap(), "flac");
    assert_eq!(validate_chunk_output_format(".wav").unwrap(), "wav");
    assert_eq!(validate_chunk_output_format(" mp3 ").unwrap(), "mp3");

    let err = validate_chunk_output_format("../bad").unwrap_err();
    assert!(err.contains("Formato de audio invalido"));
}

#[test]
fn normalized_audio_args_can_skip_silence_detection_for_parallel_prepare() {
    let args = build_extract_audio_args(
        Path::new("meeting.mp4"),
        Path::new("meeting.wav"),
        "wav",
        None,
    );

    assert!(args.windows(2).any(|pair| pair == ["-vn", "-ar"]));
    assert!(!args.iter().any(|arg| arg.contains("silencedetect")));
    assert!(args.iter().any(|arg| arg == "meeting.mp4"));
    assert!(args.iter().any(|arg| arg == "meeting.wav"));
}

#[test]
fn default_prepare_strategy_uses_single_pass_audio_workspace() {
    let mut opts = SmartChunkOptions::default();

    assert!(!should_use_parallel_prepare(&opts));

    opts.prepare_strategy = Some("parallel".to_string());
    assert!(should_use_parallel_prepare(&opts));

    opts.prepare_strategy = Some("singlePassSilence".to_string());
    assert!(!should_use_parallel_prepare(&opts));
}

#[test]
fn source_audio_fingerprint_is_content_based_across_paths() {
    let dir = std::env::temp_dir().join(format!(
        "meeting-minutes-audio-fingerprint-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("a.raw");
    let second = dir.join("b.raw");
    let different = dir.join("c.raw");
    let payload = vec![7_u8; (FINGERPRINT_SAMPLE_BYTES as usize) + 128];
    std::fs::write(&first, &payload).unwrap();
    std::fs::write(&second, &payload).unwrap();
    std::fs::write(
        &different,
        vec![8_u8; (FINGERPRINT_SAMPLE_BYTES as usize) + 128],
    )
    .unwrap();

    assert_eq!(
        source_audio_fingerprint(first.to_str().unwrap()),
        source_audio_fingerprint(second.to_str().unwrap())
    );
    assert_ne!(
        source_audio_fingerprint(first.to_str().unwrap()),
        source_audio_fingerprint(different.to_str().unwrap())
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn global_silence_cache_file_name_includes_detection_options() {
    let fingerprint = "abc123";

    assert_ne!(
        silence_cache_file_name_for_fingerprint(fingerprint, -35.0, 0.5),
        silence_cache_file_name_for_fingerprint(fingerprint, -40.0, 0.5)
    );
    assert_ne!(
        silence_cache_file_name_for_fingerprint(fingerprint, -35.0, 0.5),
        silence_cache_file_name_for_fingerprint(fingerprint, -35.0, 0.8)
    );
}

#[test]
fn parallel_chunk_export_args_select_audio_stream_only() {
    let plan = ChunkPlan {
        index: 2,
        start_sec: 12.5,
        end_sec: 45.0,
        offset_sec: 12.5,
    };

    let args = build_parallel_chunk_export_args(
        Path::new("meeting.mp4"),
        Path::new("chunks/chunk_002.flac"),
        "flac",
        &plan,
    );

    assert!(args.windows(2).any(|pair| pair == ["-map", "0:a:0"]));
    assert!(args.iter().any(|arg| arg == "-vn"));
    assert!(args.windows(2).any(|pair| pair == ["-ss", "12.500"]));
    assert!(args.windows(2).any(|pair| pair == ["-t", "32.500"]));
    assert_eq!(
        args.last().map(String::as_str),
        Some("chunks/chunk_002.flac")
    );
}

#[test]
fn contiguous_chunk_plans_use_segment_muxer_and_copy_when_format_matches() {
    let plans = vec![
        ChunkPlan {
            index: 0,
            start_sec: 0.0,
            end_sec: 10.0,
            offset_sec: 0.0,
        },
        ChunkPlan {
            index: 1,
            start_sec: 10.0,
            end_sec: 20.0,
            offset_sec: 10.0,
        },
        ChunkPlan {
            index: 2,
            start_sec: 20.0,
            end_sec: 30.0,
            offset_sec: 20.0,
        },
    ];

    let args =
        build_smart_chunk_export_args(Path::new("meeting.wav"), Path::new("chunks"), "wav", &plans);

    assert!(args.windows(2).any(|pair| pair == ["-f", "segment"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["-segment_times", "10.000,20.000"]));
    assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
    assert!(!args.iter().any(|arg| arg == "-filter_complex"));
    assert!(args
        .last()
        .expect("output pattern should exist")
        .ends_with("chunk_%03d.wav"));
}

#[test]
fn short_contiguous_reencode_stays_on_parallel_export() {
    let plans = vec![
        ChunkPlan {
            index: 0,
            start_sec: 0.0,
            end_sec: 10.0,
            offset_sec: 0.0,
        },
        ChunkPlan {
            index: 1,
            start_sec: 10.0,
            end_sec: 20.0,
            offset_sec: 10.0,
        },
        ChunkPlan {
            index: 2,
            start_sec: 20.0,
            end_sec: 30.0,
            offset_sec: 20.0,
        },
        ChunkPlan {
            index: 3,
            start_sec: 30.0,
            end_sec: 40.0,
            offset_sec: 30.0,
        },
    ];

    let strategy = select_smart_chunk_export_strategy(Path::new("meeting.wav"), "flac", &plans);

    assert_eq!(strategy, SmartChunkExportStrategy::ParallelPerChunk);
}

#[test]
fn overlapping_chunk_plans_stay_on_parallel_export_to_preserve_overlap() {
    let plans = vec![
        ChunkPlan {
            index: 0,
            start_sec: 0.0,
            end_sec: 10.0,
            offset_sec: 0.0,
        },
        ChunkPlan {
            index: 1,
            start_sec: 7.0,
            end_sec: 20.0,
            offset_sec: 7.0,
        },
    ];

    let strategy = select_smart_chunk_export_strategy(Path::new("meeting.wav"), "flac", &plans);
    let args = build_smart_chunk_export_args(
        Path::new("meeting.wav"),
        Path::new("chunks"),
        "flac",
        &plans,
    );

    assert_eq!(strategy, SmartChunkExportStrategy::ParallelPerChunk);
    assert!(
        args.is_empty(),
        "overlapping plans cannot use segment muxer without dropping overlap"
    );
}

#[test]
fn parses_duration_from_ffmpeg_progress_line() {
    let stderr = "
            size= 1024kB time=00:01:02.50 bitrate= 128.0kbits/s speed=1x
            size= 2048kB time=00:01:03.25 bitrate= 128.0kbits/s speed=1x
        ";

    assert_close(
        parse_duration_from_ffmpeg_stderr(stderr).expect("duration should parse"),
        63.25,
    );
    assert_eq!(parse_duration_from_ffmpeg_stderr("no duration here"), None);
    assert_eq!(
        parse_duration_from_ffmpeg_stderr("time=00:nope:01.00"),
        None
    );
    assert_eq!(parse_duration_from_ffmpeg_stderr("time=00:00:00.00"), None);
}

#[test]
fn parses_duration_from_ffmpeg_header_without_decoding_progress() {
    let stderr = "
Input #0, wav, from 'meeting.wav':
  Duration: 02:03:04.56, bitrate: 256 kb/s
";

    assert_close(
        parse_duration_from_ffmpeg_stderr(stderr).expect("header duration should parse"),
        7384.56,
    );
}

#[test]
fn parses_duration_from_ffprobe_stdout() {
    assert_close(
        parse_duration_from_ffprobe_stdout("3723.450000\n").expect("ffprobe duration"),
        3723.45,
    );
    assert_eq!(parse_duration_from_ffprobe_stdout("N/A\n"), None);
    assert_eq!(parse_duration_from_ffprobe_stdout("0\n"), None);
}

#[test]
fn parses_ffmpeg_silencedetect_ranges() {
    let stderr = "
            [silencedetect @ 000] silence_start: 12.345
            [silencedetect @ 000] silence_end: 14.890 | silence_duration: 2.545
            [silencedetect @ 000] silence_start: 44
            [silencedetect @ 000] silence_end: 45.25 | silence_duration: 1.25
        ";

    let ranges = parse_silencedetect(stderr);

    assert_eq!(ranges.len(), 2);
    assert_close(ranges[0].start_sec, 12.345);
    assert_close(ranges[0].end_sec, 14.890);
    assert_close(ranges[1].start_sec, 44.0);
    assert_close(ranges[1].end_sec, 45.25);
}

#[test]
fn parses_video_creation_time_from_ffmpeg_metadata() {
    let stderr = r#"
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'reuniao.mp4':
  Duration: 03:00:12.50, start: 0.000000, bitrate: 825 kb/s
  Metadata:
    major_brand     : isom
    creation_time   : 2026-05-08T18:48:29.000000Z
    title           : Reuniao de alinhamento
"#;

    let metadata = parse_ffmpeg_media_metadata(stderr);

    assert_eq!(
        metadata.embedded_created_at.as_deref(),
        Some("2026-05-08T18:48:29+00:00")
    );
    assert_eq!(
        metadata.source_title.as_deref(),
        Some("Reuniao de alinhamento")
    );
    assert_eq!(metadata.duration_sec, Some(10812.5));
}

#[test]
fn chooses_embedded_creation_time_before_filesystem_time() {
    let mut metadata = crate::models::meeting::MeetingMetadata {
        embedded_created_at: Some("2026-05-08T18:48:29+00:00".to_string()),
        file_modified_at: Some("2026-05-19T16:10:00+00:00".to_string()),
        ..Default::default()
    };

    apply_recorded_at_source(&mut metadata);

    assert_eq!(
        metadata.recorded_at.as_deref(),
        Some("2026-05-08T18:48:29+00:00")
    );
    assert_eq!(
        metadata.recorded_at_source.as_deref(),
        Some("embedded_created_at")
    );
}

#[test]
fn malformed_silence_end_keeps_pending_start_for_next_valid_end() {
    let stderr = "
            [silencedetect @ 000] silence_start: 10
            [silencedetect @ 000] silence_end: nope | silence_duration: nope
            [silencedetect @ 000] silence_end: 12.5 | silence_duration: 2.5
            [silencedetect @ 000] silence_start: 20
            [silencedetect @ 000] silence_end: 22 | silence_duration: 2
        ";

    let ranges = parse_silencedetect(stderr);

    assert_eq!(ranges.len(), 2);
    assert_close(ranges[0].start_sec, 10.0);
    assert_close(ranges[0].end_sec, 12.5);
    assert_close(ranges[1].start_sec, 20.0);
    assert_close(ranges[1].end_sec, 22.0);
}

#[test]
fn plans_chunks_near_silence_with_overlap() {
    let silences = vec![
        SilenceRange {
            start_sec: 295.0,
            end_sec: 300.0,
        },
        SilenceRange {
            start_sec: 602.0,
            end_sec: 606.0,
        },
    ];

    let chunks = plan_smart_chunks(900.0, &silences, 300.0, 180.0, 360.0, 3.0);

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].index, 0);
    assert_close(chunks[0].start_sec, 0.0);
    assert_close(chunks[0].end_sec, 297.5);
    assert_close(chunks[0].offset_sec, 0.0);
    assert_close(chunks[1].start_sec, 294.5);
    assert_close(chunks[1].end_sec, 604.0);
    assert_close(chunks[1].offset_sec, 294.5);
    assert_close(chunks[2].start_sec, 601.0);
    assert_close(chunks[2].end_sec, 900.0);
}

#[test]
fn fallback_avoids_tiny_tail_when_remaining_fits_under_max() {
    let chunks = plan_smart_chunks(650.0, &[], 300.0, 180.0, 360.0, 3.0);

    assert_eq!(chunks.len(), 2);
    assert_close(chunks[0].start_sec, 0.0);
    assert_close(chunks[0].end_sec, 300.0);
    assert_close(chunks[1].start_sec, 297.0);
    assert_close(chunks[1].end_sec, 650.0);
}

#[test]
fn invalid_and_hostile_values_terminate_with_sensible_chunks() {
    assert!(plan_smart_chunks(0.0, &[], 300.0, 180.0, 360.0, 3.0).is_empty());
    assert!(plan_smart_chunks(-1.0, &[], 300.0, 180.0, 360.0, 3.0).is_empty());
    assert!(plan_smart_chunks(f64::NAN, &[], 300.0, 180.0, 360.0, 3.0).is_empty());

    let infinite_duration = plan_with_timeout(f64::INFINITY, vec![], 300.0, 180.0, 360.0, 3.0);
    let hostile_options = plan_with_timeout(650.0, vec![], 0.0, -10.0, 0.0, 999.0);

    assert!(
        infinite_duration.as_ref().is_some_and(Vec::is_empty),
        "non-finite duration should terminate with no chunks"
    );

    let chunks = hostile_options.expect("hostile options should terminate");
    assert!(!chunks.is_empty());
    assert!(chunks.len() <= 4);
    assert_close(chunks[0].start_sec, 0.0);
    assert_close(chunks.last().unwrap().end_sec, 650.0);

    for chunk in &chunks {
        assert!(chunk.start_sec.is_finite());
        assert!(chunk.end_sec.is_finite());
        assert!(chunk.end_sec > chunk.start_sec);
        assert!(chunk.end_sec - chunk.start_sec <= 480.0);
    }
}
