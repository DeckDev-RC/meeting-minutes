use super::*;
use crate::models::transcription::TranscriptionSegment;

#[test]
fn local_diarization_merges_continuations_and_alternates_after_question() {
    let segments = vec![
        TranscriptionSegment {
            id: 0,
            start: 0.0,
            end: 4.0,
            text: "Bom dia pessoal.".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 4.2,
            end: 8.0,
            text: "Hoje vamos revisar o projeto.".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 10.0,
            end: 12.0,
            text: "Podem validar o prazo?".to_string(),
        },
        TranscriptionSegment {
            id: 3,
            start: 12.2,
            end: 15.0,
            text: "Sim, ate sexta.".to_string(),
        },
    ];

    let result = diarize_transcription_locally(&segments);

    assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(result.segments.len(), 3);
    assert_eq!(result.segments[0].speaker, "Falante 1");
    assert_eq!(
        result.segments[0].text,
        "Bom dia pessoal. Hoje vamos revisar o projeto."
    );
    assert_eq!(result.segments[1].speaker, "Falante 1");
    assert_eq!(result.segments[1].text, "Podem validar o prazo?");
    assert_eq!(result.segments[2].speaker, "Falante 2");
    assert_eq!(result.segments[2].text, "Sim, ate sexta.");
}

#[test]
fn local_diarization_uses_inline_speaker_labels_when_present() {
    let segments = vec![
        TranscriptionSegment {
            id: 0,
            start: 0.0,
            end: 2.0,
            text: "Ana: vamos comecar pela pauta.".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 2.5,
            end: 4.0,
            text: "Bruno - combinado, eu atualizo o cronograma.".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 5.0,
            end: 7.0,
            text: "Ana: obrigado.".to_string(),
        },
    ];

    let result = diarize_transcription_locally(&segments);

    assert_eq!(result.speakers, vec!["Ana", "Bruno"]);
    assert_eq!(result.segments.len(), 3);
    assert_eq!(result.segments[0].speaker, "Ana");
    assert_eq!(result.segments[0].text, "vamos comecar pela pauta.");
    assert_eq!(result.segments[1].speaker, "Bruno");
    assert_eq!(
        result.segments[1].text,
        "combinado, eu atualizo o cronograma."
    );
    assert_eq!(result.segments[2].speaker, "Ana");
    assert_eq!(result.segments[2].text, "obrigado.");
}

#[test]
fn audio_turns_are_aligned_to_transcript_by_largest_overlap() {
    let segments = vec![
        TranscriptionSegment {
            id: 0,
            start: 0.0,
            end: 4.0,
            text: "Abrimos a reuniao.".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 4.0,
            end: 8.0,
            text: "Revisamos o cronograma.".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 8.0,
            end: 12.0,
            text: "Eu envio ate sexta.".to_string(),
        },
    ];
    let turns = vec![
        SpeakerTurn {
            start: 0.0,
            end: 7.5,
            speaker_index: 0,
        },
        SpeakerTurn {
            start: 7.5,
            end: 12.5,
            speaker_index: 1,
        },
    ];

    let result = diarize_segments_with_speaker_turns(&segments, &turns);

    assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(result.segments.len(), 2);
    assert_eq!(result.segments[0].speaker, "Falante 1");
    assert_eq!(
        result.segments[0].text,
        "Abrimos a reuniao. Revisamos o cronograma."
    );
    assert_eq!(result.segments[1].speaker, "Falante 2");
    assert_eq!(result.segments[1].text, "Eu envio ate sexta.");
}

#[test]
fn audio_turn_fallback_uses_nearest_speaker_when_segment_has_no_overlap() {
    let segments = vec![TranscriptionSegment {
        id: 0,
        start: 12.0,
        end: 13.0,
        text: "fala entre turnos".to_string(),
    }];
    let turns = vec![
        SpeakerTurn {
            start: 10.0,
            end: 11.0,
            speaker_index: 0,
        },
        SpeakerTurn {
            start: 30.0,
            end: 31.0,
            speaker_index: 1,
        },
    ];

    let result = diarize_segments_with_speaker_turns(&segments, &turns);

    assert_eq!(result.segments[0].speaker, "Falante 1");
}

#[test]
fn modern_cpu_turns_align_to_transcript_text() {
    let modern_result = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 5.0,
                text: String::new(),
            },
            DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 5.0,
                end: 10.0,
                text: String::new(),
            },
        ],
    };
    let transcript = vec![
        TranscriptionSegment {
            id: 0,
            start: 1.0,
            end: 3.0,
            text: "A primeira fala tem texto.".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 6.0,
            end: 9.0,
            text: "A segunda fala tambem.".to_string(),
        },
    ];

    let result = align_modern_cpu_diarization_to_transcript(modern_result, &transcript);

    assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(result.segments.len(), 2);
    assert_eq!(result.segments[0].text, "A primeira fala tem texto.");
    assert_eq!(result.segments[1].speaker, "Falante 2");
    assert_eq!(result.segments[1].text, "A segunda fala tambem.");
}

#[test]
fn speaker_turns_are_json_serializable_for_speculative_pipeline() {
    let turns = vec![SpeakerTurn {
        start: 1.0,
        end: 3.5,
        speaker_index: 2,
    }];

    let json = serde_json::to_string(&turns).unwrap();

    assert_eq!(json, r#"[{"start":1.0,"end":3.5,"speakerIndex":2}]"#);
}

#[test]
fn serialized_speaker_turns_align_to_transcript_text() {
    let segments_json = serde_json::to_string(&vec![
        TranscriptionSegment {
            id: 1,
            start: 0.0,
            end: 2.0,
            text: "primeira fala".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 2.1,
            end: 4.0,
            text: "segunda fala".to_string(),
        },
    ])
    .unwrap();
    let turns_json =
        r#"[{"start":0.0,"end":2.2,"speakerIndex":0},{"start":2.2,"end":4.5,"speakerIndex":1}]"#
            .to_string();

    let result = align_speaker_turns_to_transcription(segments_json, turns_json).unwrap();

    assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(result.segments[0].speaker, "Falante 1");
    assert_eq!(result.segments[1].speaker, "Falante 2");
}

#[test]
fn selective_refinement_selects_chunk_with_weak_turn_coverage() {
    let chunks = vec![
        ExportedChunk {
            index: 0,
            audio_path: "chunk_000.wav".to_string(),
            start_sec: 0.0,
            end_sec: 100.0,
            offset_sec: 0.0,
            duration_sec: 100.0,
        },
        ExportedChunk {
            index: 1,
            audio_path: "chunk_001.wav".to_string(),
            start_sec: 100.0,
            end_sec: 200.0,
            offset_sec: 100.0,
            duration_sec: 100.0,
        },
    ];
    let segments = vec![
        TranscriptionSegment {
            id: 0,
            start: 10.0,
            end: 15.0,
            text: "coberto".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 120.0,
            end: 130.0,
            text: "sem cobertura".to_string(),
        },
    ];
    let turns = vec![SpeakerTurn {
        start: 0.0,
        end: 20.0,
        speaker_index: 0,
    }];

    let selected = select_suspicious_chunks_for_refinement(&segments, &turns, &chunks, 1);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].index, 1);
}

#[test]
fn selective_refinement_windows_are_short_and_padded() {
    let chunks = vec![ExportedChunk {
        index: 1,
        audio_path: "chunk_001.wav".to_string(),
        start_sec: 100.0,
        end_sec: 200.0,
        offset_sec: 100.0,
        duration_sec: 100.0,
    }];
    let segments = vec![TranscriptionSegment {
        id: 1,
        start: 120.0,
        end: 130.0,
        text: "sem cobertura".to_string(),
    }];
    let turns = vec![SpeakerTurn {
        start: 0.0,
        end: 20.0,
        speaker_index: 0,
    }];

    let windows = select_suspicious_refinement_windows(&segments, &turns, &chunks, 1);

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].chunk.index, 1);
    assert_eq!(windows[0].start_sec, 116.0);
    assert_eq!(windows[0].end_sec, 134.0);
    assert!(windows[0].duration_sec() <= 45.0);
}

#[test]
fn selective_refinement_selects_chunk_with_excessive_speaker_switches() {
    let chunks = vec![ExportedChunk {
        index: 0,
        audio_path: "chunk_000.wav".to_string(),
        start_sec: 0.0,
        end_sec: 80.0,
        offset_sec: 0.0,
        duration_sec: 80.0,
    }];
    let segments = (0..8)
        .map(|index| TranscriptionSegment {
            id: index,
            start: index as f64 * 5.0,
            end: index as f64 * 5.0 + 4.0,
            text: format!("fala {index}"),
        })
        .collect::<Vec<_>>();
    let turns = (0..8)
        .map(|index| SpeakerTurn {
            start: index as f64 * 5.0,
            end: index as f64 * 5.0 + 4.0,
            speaker_index: (index % 2) as i32,
        })
        .collect::<Vec<_>>();

    let windows = select_suspicious_refinement_windows(&segments, &turns, &chunks, 1);

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].chunk.index, 0);
}

#[test]
fn selective_refinement_selects_single_speaker_chunk_when_multiple_expected() {
    let chunks = vec![ExportedChunk {
        index: 0,
        audio_path: "chunk_000.wav".to_string(),
        start_sec: 0.0,
        end_sec: 120.0,
        offset_sec: 0.0,
        duration_sec: 120.0,
    }];
    let segments = vec![
        TranscriptionSegment {
            id: 0,
            start: 10.0,
            end: 20.0,
            text: "primeira fala".to_string(),
        },
        TranscriptionSegment {
            id: 1,
            start: 40.0,
            end: 50.0,
            text: "segunda fala".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 80.0,
            end: 90.0,
            text: "terceira fala".to_string(),
        },
    ];
    let turns = vec![SpeakerTurn {
        start: 0.0,
        end: 120.0,
        speaker_index: 0,
    }];

    let windows =
        select_suspicious_refinement_windows_with_context(&segments, &turns, &chunks, 1, Some(3));

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].chunk.index, 0);
}

#[test]
fn selective_refinement_replaces_only_selected_chunk_segments() {
    let base = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 50.0,
                text: "fora".to_string(),
            },
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 120.0,
                end: 130.0,
                text: "trocar".to_string(),
            },
        ],
    };
    let refined = DiarizedResult {
        speakers: vec!["Falante 2".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 2".to_string(),
            start: 120.0,
            end: 130.0,
            text: "trocar".to_string(),
        }],
    };
    let chunks = vec![ExportedChunk {
        index: 1,
        audio_path: "chunk_001.wav".to_string(),
        start_sec: 100.0,
        end_sec: 200.0,
        offset_sec: 100.0,
        duration_sec: 100.0,
    }];

    let merged = merge_selective_refinement(base, refined, &chunks);

    assert_eq!(merged.segments.len(), 2);
    assert_eq!(merged.segments[0].speaker, "Falante 1");
    assert_eq!(merged.segments[1].speaker, "Falante 2");
    assert_eq!(merged.speakers, vec!["Falante 1", "Falante 2"]);
}

#[test]
fn diarization_asset_paths_point_to_expected_local_model_files() {
    let base_dir = std::path::Path::new("C:/app-data");
    let paths = diarization_asset_paths(base_dir);

    assert!(paths
        .segmentation_model
        .ends_with("models/diarization/sherpa-onnx-pyannote-segmentation-3-0-model.int8.onnx"));
    assert!(paths.embedding_model.ends_with(
        "models/diarization/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"
    ));
}

#[test]
fn modern_cpu_backend_paths_point_to_project_tools() {
    let root = std::path::Path::new("C:/project");
    let paths = modern_cpu_backend_paths(root);

    assert!(paths
        .python_exe
        .ends_with(".venv-diarize/Scripts/python.exe"));
    assert!(paths
        .script_path
        .ends_with("scripts/diarize_cpu_backend.py"));
}

#[test]
fn pyannote_backend_paths_point_to_project_tools() {
    let root = std::path::Path::new("C:/project");
    let paths = pyannote_backend_paths(root);

    assert!(paths
        .python_exe
        .ends_with(".venv-pyannote/Scripts/python.exe"));
    assert!(paths
        .script_path
        .ends_with("scripts/pyannote_community_backend.py"));
}

#[test]
fn diarization_mode_parses_supported_values() {
    assert_eq!(
        DiarizationMode::from_option(Some("fast".to_string())),
        DiarizationMode::Fast
    );
    assert_eq!(
        DiarizationMode::from_option(Some("hybrid".to_string())),
        DiarizationMode::Hybrid
    );
    assert_eq!(
        DiarizationMode::from_option(Some("modern-cpu".to_string())),
        DiarizationMode::ModernCpu
    );
    assert_eq!(
        DiarizationMode::from_option(Some("modern-cpu-chunked".to_string())),
        DiarizationMode::ModernCpuChunked
    );
    assert_eq!(
        DiarizationMode::from_option(Some("diarize-cpu".to_string())),
        DiarizationMode::ModernCpu
    );
    assert_eq!(
        DiarizationMode::from_option(Some("precise".to_string())),
        DiarizationMode::Precise
    );
    assert_eq!(
        DiarizationMode::from_option(Some("pyannote".to_string())),
        DiarizationMode::Pyannote
    );
    assert_eq!(
        DiarizationMode::from_option(Some("auto".to_string())),
        DiarizationMode::Auto
    );
    assert_eq!(
        DiarizationMode::from_option(Some("bad".to_string())),
        DiarizationMode::Auto
    );
    assert_eq!(DiarizationMode::from_option(None), DiarizationMode::Auto);
}

#[test]
fn diarization_thread_count_is_bounded() {
    assert_eq!(normalize_diarization_threads(Some(0), 20), 2);
    assert_eq!(normalize_diarization_threads(Some(12), 20), 12);
    assert_eq!(normalize_diarization_threads(Some(64), 20), 16);
    assert_eq!(normalize_diarization_threads(None, 20), 8);
}

#[test]
fn explicit_diarization_modes_do_not_allow_local_fallback() {
    assert!(mode_allows_local_fallback(DiarizationMode::Auto));
    assert!(!mode_allows_local_fallback(DiarizationMode::Fast));
    assert!(!mode_allows_local_fallback(DiarizationMode::Hybrid));
    assert!(!mode_allows_local_fallback(DiarizationMode::ModernCpu));
    assert!(!mode_allows_local_fallback(
        DiarizationMode::ModernCpuChunked
    ));
    assert!(!mode_allows_local_fallback(DiarizationMode::Precise));
    assert!(!mode_allows_local_fallback(DiarizationMode::Pyannote));
}

#[test]
fn diarization_telemetry_records_backend_and_counts() {
    let result = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 0.0,
            end: 1.0,
            text: "ola".to_string(),
        }],
    };

    let telemetry = build_diarization_telemetry(
        DiarizationMode::Precise,
        DiarizationBackend::SherpaPrecise,
        Some("modern CPU backend unavailable".to_string()),
        12.5,
        &result,
    );
    let json = serde_json::to_value(&telemetry).unwrap();

    assert_eq!(json["requestedMode"], "precise");
    assert_eq!(json["backendUsed"], "sherpa-precise");
    assert_eq!(json["fallbackReason"], "modern CPU backend unavailable");
    assert_eq!(json["wallClockSec"], 12.5);
    assert_eq!(json["speakerCount"], 2);
    assert_eq!(json["segmentCount"], 1);
}

#[test]
fn shifted_segments_for_chunk_use_chunk_relative_time() {
    let chunk = ExportedChunk {
        index: 1,
        audio_path: "chunk.flac".to_string(),
        start_sec: 100.0,
        end_sec: 200.0,
        offset_sec: 100.0,
        duration_sec: 100.0,
    };
    let segments = vec![
        TranscriptionSegment {
            id: 1,
            start: 110.0,
            end: 115.0,
            text: "hello".to_string(),
        },
        TranscriptionSegment {
            id: 2,
            start: 220.0,
            end: 225.0,
            text: "outside".to_string(),
        },
    ];

    let shifted = segments_for_chunk(&segments, &chunk);

    assert_eq!(shifted.len(), 1);
    assert_eq!(shifted[0].start, 10.0);
    assert_eq!(shifted[0].end, 15.0);
}

#[test]
fn stitch_chunk_results_maps_overlap_speakers() {
    let first = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 0.0,
            end: 10.0,
            text: "same overlap".to_string(),
        }],
    };
    let second = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 8.0,
            end: 18.0,
            text: "same overlap continues".to_string(),
        }],
    };

    let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

    assert_eq!(stitched.speakers, vec!["Falante 1"]);
    assert_eq!(stitched.segments.len(), 1);
    assert_eq!(stitched.segments[0].speaker, "Falante 1");
    assert_eq!(stitched.segments[0].start, 0.0);
    assert_eq!(stitched.segments[0].end, 18.0);
}

#[test]
fn stitch_chunk_results_maps_different_local_labels_by_overlap() {
    let first = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 0.0,
            end: 10.0,
            text: "overlap speaker".to_string(),
        }],
    };
    let second = DiarizedResult {
        speakers: vec!["Falante 2".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 2".to_string(),
            start: 8.0,
            end: 18.0,
            text: "overlap speaker continues".to_string(),
        }],
    };

    let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

    assert_eq!(stitched.speakers, vec!["Falante 1"]);
    assert_eq!(stitched.segments.len(), 1);
    assert_eq!(stitched.segments[0].speaker, "Falante 1");
    assert_eq!(stitched.segments[0].end, 18.0);
}

#[test]
fn stitch_chunk_results_does_not_collapse_new_local_speakers_near_overlap() {
    let first = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 0.0,
            end: 10.0,
            text: "overlap speaker".to_string(),
        }],
    };
    let second = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 8.0,
                end: 11.0,
                text: "overlap speaker continues".to_string(),
            },
            DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 11.2,
                end: 18.0,
                text: "new speaker near overlap".to_string(),
            },
        ],
    };

    let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

    assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(stitched.segments.len(), 2);
    assert_eq!(stitched.segments[0].speaker, "Falante 1");
    assert_eq!(stitched.segments[1].speaker, "Falante 2");
}

#[test]
fn expected_speaker_stitch_reuses_local_label_ids_across_chunks() {
    let first = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 5.0,
                text: "primeiro falante".to_string(),
            },
            DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 5.0,
                end: 10.0,
                text: "segundo falante".to_string(),
            },
        ],
    };
    let second = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 100.0,
                end: 105.0,
                text: "primeiro falante no chunk dois".to_string(),
            },
            DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 105.0,
                end: 110.0,
                text: "segundo falante no chunk dois".to_string(),
            },
        ],
    };

    let stitched =
        stitch_diarized_chunk_results_with_expected_speakers(vec![first, second], 3.0, Some(2));

    assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(stitched.segments.len(), 4);
    assert_eq!(stitched.segments[2].speaker, "Falante 1");
    assert_eq!(stitched.segments[3].speaker, "Falante 2");
}

#[test]
fn modern_cpu_batch_centroids_map_local_labels_across_chunks() {
    let raw = r#"[
        {
            "index": 0,
            "offsetSec": 0.0,
            "diarized": {
                "speakers": ["Falante 1"],
                "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
            },
            "speakerCentroids": [
                {"speaker": "Falante 1", "embedding": [1.0, 0.0]}
            ]
        },
        {
            "index": 1,
            "offsetSec": 100.0,
            "diarized": {
                "speakers": ["Falante 2"],
                "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
            },
            "speakerCentroids": [
                {"speaker": "Falante 2", "embedding": [0.98, 0.02]}
            ]
        }
    ]"#;

    let stitched = stitch_modern_cpu_batch_outputs(raw, Some(2)).unwrap();

    assert_eq!(stitched.speakers, vec!["Falante 1"]);
    assert_eq!(stitched.segments.len(), 2);
    assert_eq!(stitched.segments[1].speaker, "Falante 1");
    assert_eq!(stitched.segments[1].start, 101.0);
}

#[test]
fn modern_cpu_batch_centroids_respect_expected_speaker_cap() {
    let raw = r#"[
        {
            "index": 0,
            "offsetSec": 0.0,
            "diarized": {
                "speakers": ["Falante 1"],
                "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
            },
            "speakerCentroids": [
                {"speaker": "Falante 1", "embedding": [1.0, 0.0]}
            ]
        },
        {
            "index": 1,
            "offsetSec": 100.0,
            "diarized": {
                "speakers": ["Falante 2"],
                "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
            },
            "speakerCentroids": [
                {"speaker": "Falante 2", "embedding": [0.5, 0.5]}
            ]
        }
    ]"#;

    let stitched = stitch_modern_cpu_batch_outputs(raw, Some(1)).unwrap();

    assert_eq!(stitched.speakers, vec!["Falante 1"]);
    assert_eq!(stitched.segments[1].speaker, "Falante 1");
}

#[test]
fn modern_cpu_batch_outputs_are_shifted_and_stitched() {
    let raw = r#"[
        {
            "index": 0,
            "offsetSec": 0.0,
            "diarized": {
                "speakers": ["Falante 1"],
                "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
            }
        },
        {
            "index": 1,
            "offsetSec": 100.0,
            "diarized": {
                "speakers": ["Falante 2"],
                "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
            }
        }
    ]"#;

    let stitched = stitch_modern_cpu_batch_outputs(raw, Some(2)).unwrap();

    assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
    assert_eq!(stitched.segments[1].speaker, "Falante 2");
    assert_eq!(stitched.segments[1].start, 101.0);
    assert_eq!(stitched.segments[1].end, 106.0);
}

#[test]
fn hybrid_sherpa_requires_wav_chunks() {
    let wav_chunk = ExportedChunk {
        index: 0,
        audio_path: "chunk.wav".to_string(),
        start_sec: 0.0,
        end_sec: 10.0,
        offset_sec: 0.0,
        duration_sec: 10.0,
    };
    let flac_chunk = ExportedChunk {
        audio_path: "chunk.flac".to_string(),
        ..wav_chunk.clone()
    };

    assert!(chunks_can_use_sherpa(&[wav_chunk]));
    assert!(!chunks_can_use_sherpa(&[flac_chunk]));
    assert!(!chunks_can_use_sherpa(&[]));
}
