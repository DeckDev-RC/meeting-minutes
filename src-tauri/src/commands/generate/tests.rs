use super::*;
use crate::models::meeting::MeetingMetadata;
use crate::models::transcription::{
    DiarizedResult, DiarizedSegment, MeetingAction, MeetingChunkInsights, MeetingDecision,
};

#[test]
fn gemini_text_extraction_strips_markdown_fenced_html() {
    let result = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "text": "```html\n<div class=\"header\">Ata</div>\n```"
                }]
            }
        }]
    });

    assert_eq!(
        extract_gemini_text(result).unwrap(),
        "<div class=\"header\">Ata</div>"
    );
}

#[test]
fn gemini_stream_delta_extracts_text_from_sse_data_line() {
    let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"<div>parcial"}]}}]}"#;

    assert_eq!(
        extract_gemini_stream_delta(line).unwrap().as_deref(),
        Some("<div>parcial")
    );
    assert_eq!(extract_gemini_stream_delta("event: message").unwrap(), None);
    assert_eq!(extract_gemini_stream_delta("data: [DONE]").unwrap(), None);
}

#[test]
fn sse_pending_buffer_drains_complete_lines_without_losing_tail() {
    let mut pending = String::from("data: one\r\ndata: two\npartial");
    let lines = drain_complete_sse_lines(&mut pending);

    assert_eq!(lines, vec!["data: one", "data: two"]);
    assert_eq!(pending, "partial");
}

#[test]
fn gemini_retry_policy_retries_rate_limits_and_server_errors_only() {
    assert!(should_retry_gemini_status(
        reqwest::StatusCode::TOO_MANY_REQUESTS
    ));
    assert!(should_retry_gemini_status(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    ));
    assert!(should_retry_gemini_status(
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    ));

    assert!(!should_retry_gemini_status(
        reqwest::StatusCode::BAD_REQUEST
    ));
    assert!(!should_retry_gemini_status(
        reqwest::StatusCode::UNAUTHORIZED
    ));
}

#[test]
fn gemini_retry_delay_uses_exponential_backoff() {
    assert_eq!(gemini_retry_delay(0), std::time::Duration::from_secs(1));
    assert_eq!(gemini_retry_delay(1), std::time::Duration::from_secs(2));
    assert_eq!(gemini_retry_delay(2), std::time::Duration::from_secs(4));
}

#[test]
fn gemini_request_body_bytes_round_trip_json() {
    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": "ola" }]
        }]
    });

    let bytes = gemini_request_body_bytes(&body).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(parsed, body);
}

#[test]
fn meeting_chunk_insights_round_trips_from_structured_json() {
    let json = serde_json::json!({
        "chunkIndex": 2,
        "startSec": 600.0,
        "endSec": 960.0,
        "summary": "Equipe alinhou o prazo da entrega.",
        "topics": ["Entrega", "Riscos"],
        "decisions": [{
            "title": "Manter escopo da sprint",
            "owner": "Falante 1",
            "timestampSec": 620.5,
            "evidence": "vamos manter o escopo atual"
        }],
        "actions": [{
            "task": "Enviar proposta revisada",
            "owner": "Ana",
            "deadline": "sexta-feira",
            "timestampSec": 700.0,
            "evidence": "Ana envia ate sexta"
        }],
        "questions": ["Validar limite do budget"],
        "risks": ["Dependencia de aprovacao externa"]
    });

    let insights: MeetingChunkInsights = serde_json::from_value(json).unwrap();

    assert_eq!(insights.chunk_index, 2);
    assert_eq!(insights.actions[0].owner, "Ana");
    assert_eq!(insights.decisions[0].title, "Manter escopo da sprint");
}

#[test]
fn prepared_name_aliases_replace_aliases_with_word_boundaries() {
    let aliases = prepare_name_aliases(&[("Rafael".to_string(), "Rafaela".to_string())]);

    assert_eq!(
        replace_prepared_name_aliases("Rafael vai acionar Rafael.", &aliases),
        "Rafaela vai acionar Rafaela."
    );
    assert_eq!(
        replace_prepared_name_aliases("Rafaelar nao deve mudar.", &aliases),
        "Rafaelar nao deve mudar."
    );
}

#[test]
fn final_minutes_payload_uses_compact_facts_instead_of_raw_transcript() {
    let diarized = DiarizedResult {
        speakers: vec!["Ana".to_string(), "Bruno".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Ana".to_string(),
            start: 0.0,
            end: 6.0,
            text: "Texto bruto longo que nao deve entrar no payload compacto".to_string(),
        }],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 360.0,
        summary: "Discussao sobre o cronograma.".to_string(),
        topics: vec!["Cronograma".to_string()],
        decisions: vec![],
        actions: vec![MeetingAction {
            task: "Enviar cronograma revisado".to_string(),
            owner: "Ana".to_string(),
            deadline: "sexta-feira".to_string(),
            timestamp_sec: 120.0,
            evidence: "Ana vai enviar sexta".to_string(),
        }],
        questions: vec![],
        risks: vec![],
    }];

    let payload = build_minutes_fact_payload(&diarized, &insights, None, None).unwrap();

    assert!(payload.contains("Enviar cronograma revisado"));
    assert!(payload.contains("\"speakers\""));
    assert!(!payload.contains("Texto bruto longo"));
}

#[test]
fn final_minutes_payload_includes_user_supplied_participant_names() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 60.0,
        summary: "Alinhamento rapido.".to_string(),
        topics: vec!["Projeto".to_string()],
        decisions: vec![],
        actions: vec![],
        questions: vec![],
        risks: vec![],
    }];
    let participant_names = vec![
        "Caio".to_string(),
        "Emanuella".to_string(),
        "Caio".to_string(),
    ];

    let payload = build_minutes_fact_payload(
        &diarized,
        &insights,
        Some(participant_names.as_slice()),
        None,
    )
    .unwrap();

    assert!(payload.contains("\"participantNames\":[\"Caio\",\"Emanuella\"]"));
}

#[test]
fn final_minutes_payload_uses_deduplicated_meeting_graph() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![],
    };
    let insights = vec![
        MeetingChunkInsights {
            chunk_index: 0,
            start_sec: 0.0,
            end_sec: 60.0,
            summary: "Equipe decidiu publicar o MVP.".to_string(),
            topics: vec!["MVP".to_string(), "Produto".to_string()],
            decisions: vec![MeetingDecision {
                title: "Publicar MVP".to_string(),
                owner: "Caio".to_string(),
                timestamp_sec: 12.0,
                evidence: "vamos publicar o MVP".to_string(),
            }],
            actions: vec![MeetingAction {
                task: "Enviar proposta revisada".to_string(),
                owner: "Emanuella".to_string(),
                deadline: "sexta".to_string(),
                timestamp_sec: 30.0,
                evidence: "Emanuella envia sexta".to_string(),
            }],
            questions: vec!["Quem valida?".to_string()],
            risks: vec!["Prazo curto".to_string()],
        },
        MeetingChunkInsights {
            chunk_index: 1,
            start_sec: 60.0,
            end_sec: 120.0,
            summary: "Repetiram a acao da proposta.".to_string(),
            topics: vec!["mvp".to_string()],
            decisions: vec![MeetingDecision {
                title: "publicar mvp".to_string(),
                owner: "Caio".to_string(),
                timestamp_sec: 70.0,
                evidence: "confirmado publicar mvp".to_string(),
            }],
            actions: vec![MeetingAction {
                task: "Enviar proposta revisada".to_string(),
                owner: "Emanuella".to_string(),
                deadline: "sexta".to_string(),
                timestamp_sec: 90.0,
                evidence: "reforco da proposta".to_string(),
            }],
            questions: vec!["quem valida?".to_string()],
            risks: vec!["Prazo curto".to_string()],
        },
    ];

    let payload = build_minutes_fact_payload(&diarized, &insights, None, None).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let graph = &parsed["meetingGraph"];

    assert!(parsed.get("chunks").is_none());
    assert_eq!(graph["topics"].as_array().unwrap().len(), 2);
    assert_eq!(graph["decisions"].as_array().unwrap().len(), 1);
    assert_eq!(graph["actions"].as_array().unwrap().len(), 1);
    assert_eq!(graph["questions"].as_array().unwrap().len(), 1);
    assert_eq!(graph["risks"].as_array().unwrap().len(), 1);
}

#[test]
fn meeting_graph_does_not_promote_generic_owners_to_participants() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 60.0,
        summary: "Equipe revisou a operacao e distribuiu pendencias.".to_string(),
        topics: vec!["Operacao".to_string()],
        decisions: vec![
            MeetingDecision {
                title: "Priorizar cobranca".to_string(),
                owner: "Não especificado".to_string(),
                timestamp_sec: 10.0,
                evidence: "precisamos priorizar isso".to_string(),
            },
            MeetingDecision {
                title: "Caio valida fluxo".to_string(),
                owner: "Caio".to_string(),
                timestamp_sec: 20.0,
                evidence: "Caio vai validar".to_string(),
            },
        ],
        actions: vec![
            MeetingAction {
                task: "Acionar vendas".to_string(),
                owner: "Equipe de Marketing/Vendas".to_string(),
                deadline: "sexta".to_string(),
                timestamp_sec: 30.0,
                evidence: "marketing e vendas cuidam".to_string(),
            },
            MeetingAction {
                task: "Revisar conciliacao".to_string(),
                owner: "Financeiro".to_string(),
                deadline: "segunda".to_string(),
                timestamp_sec: 40.0,
                evidence: "financeiro revisa".to_string(),
            },
            MeetingAction {
                task: "Enviar resumo".to_string(),
                owner: "Maria Eduarda, Gabriel".to_string(),
                deadline: "hoje".to_string(),
                timestamp_sec: 50.0,
                evidence: "Maria Eduarda e Gabriel enviam".to_string(),
            },
            MeetingAction {
                task: "Confirmar aprovacao".to_string(),
                owner: "[Implicit], Aprovador, N/A".to_string(),
                deadline: "amanha".to_string(),
                timestamp_sec: 55.0,
                evidence: "aguardar aprovacao".to_string(),
            },
        ],
        questions: vec![],
        risks: vec![],
    }];

    let graph = build_minutes_fact_graph(&diarized, &insights, &[]);

    assert_eq!(
        graph.participants,
        vec!["Falante 1", "Falante 2", "Caio", "Maria Eduarda", "Gabriel"]
    );
}

#[test]
fn low_quality_minutes_detection_rejects_placeholders_and_missing_actions() {
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 60.0,
        summary: "Equipe alinhou pendencias importantes.".to_string(),
        topics: vec!["Sistema leitor".to_string()],
        decisions: vec![MeetingDecision {
            title: "Resolver drivers desatualizados".to_string(),
            owner: "Caio".to_string(),
            timestamp_sec: 10.0,
            evidence: "precisa funcionar todo dia".to_string(),
        }],
        actions: vec![MeetingAction {
            task: "Corrigir processamento de PDFs".to_string(),
            owner: "Caio".to_string(),
            deadline: "semana que vem".to_string(),
            timestamp_sec: 20.0,
            evidence: "PDF nao esta processando".to_string(),
        }],
        questions: vec![],
        risks: vec![],
    }];
    let bad_html = r#"<div class="header"><p>Data: [Data da Reunião]</p></div><div class="section"><h2>Resumo Executivo</h2><div class="summary-box">Curto.</div></div>"#;

    assert!(minutes_html_is_low_quality(bad_html, &insights));
}

#[test]
fn local_minutes_renderer_uses_meeting_graph_without_placeholders() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 992.9,
        summary:
            "Emanuella apresentou problemas do leitor de documentos e Caio alinhou proximos passos."
                .to_string(),
        topics: vec![
            "Sistema leitor".to_string(),
            "Processamento de PDF".to_string(),
        ],
        decisions: vec![MeetingDecision {
            title: "Rafaela sera ponto de contato para problemas do leitor".to_string(),
            owner: "Caio".to_string(),
            timestamp_sec: 886.0,
            evidence: "vou botar a Rafaela como ponto de contato".to_string(),
        }],
        actions: vec![MeetingAction {
            task: "Resolver os problemas da ferramenta".to_string(),
            owner: "Caio".to_string(),
            deadline: "ate semana que vem".to_string(),
            timestamp_sec: 801.0,
            evidence: "ate semana que vem a gente consiga matar isso tudo".to_string(),
        }],
        questions: vec!["Por que o PDF falha?".to_string()],
        risks: vec!["Sistema parado atrapalha a operacao.".to_string()],
    }];
    let participant_names = vec!["Caio".to_string(), "Emanuella".to_string()];

    let html = render_minutes_from_facts_locally(&diarized, &insights, &participant_names, None);

    assert!(!html.contains("[Data"));
    assert!(!html.contains("[Hora"));
    assert!(html.contains("Rafaela sera ponto de contato"));
    assert!(html.contains("Resolver os problemas da ferramenta"));
    assert!(html.contains("table-actions"));
    assert!(html.contains("tag-decision"));
    assert!(html.len() > 2500);
}

#[test]
fn final_minutes_payload_includes_meeting_metadata() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 60.0,
        summary: "Reuniao sobre operacao.".to_string(),
        topics: vec!["Operacao".to_string()],
        decisions: vec![],
        actions: vec![],
        questions: vec![],
        risks: vec![],
    }];
    let metadata = MeetingMetadata {
        source_file_name: Some("reuniao.mp4".to_string()),
        recorded_at: Some("2026-05-08T15:48:29-03:00".to_string()),
        recorded_at_source: Some("embedded_created_at".to_string()),
        ..Default::default()
    };

    let payload = build_minutes_fact_payload(&diarized, &insights, None, Some(&metadata)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();

    assert_eq!(
        parsed["meetingMetadata"]["recordedAt"],
        serde_json::json!("2026-05-08T15:48:29-03:00")
    );
    assert_eq!(
        parsed["meetingMetadata"]["sourceFileName"],
        serde_json::json!("reuniao.mp4")
    );
}

#[test]
fn local_minutes_renderer_uses_metadata_date_and_time() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 120.0,
        summary: "Equipe alinhou plano de entrega.".to_string(),
        topics: vec!["Entrega".to_string()],
        decisions: vec![],
        actions: vec![],
        questions: vec![],
        risks: vec![],
    }];
    let metadata = MeetingMetadata {
        recorded_at: Some("2026-05-08T15:48:29-03:00".to_string()),
        recorded_at_source: Some("embedded_created_at".to_string()),
        ..Default::default()
    };

    let html = render_minutes_from_facts_locally(&diarized, &insights, &[], Some(&metadata));

    assert!(html.contains("Data da reunião: 08/05/2026"));
    assert!(html.contains("Hora: 15:48"));
    assert!(!html.contains("[Data"));
    assert!(!html.contains("[Hora"));
}

#[test]
fn meeting_graph_normalizes_clear_name_variants_before_minutes() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 120.0,
        summary: "Uma nova colaboradora, Rafael, sera treinada. Caio citou a Rafaela como contato."
            .to_string(),
        topics: vec!["Treinamento".to_string()],
        decisions: vec![MeetingDecision {
            title: "Rafael sera ponto de contato".to_string(),
            owner: "Caio".to_string(),
            timestamp_sec: 80.0,
            evidence: "vou botar a Rafaela como ponto de contato".to_string(),
        }],
        actions: vec![MeetingAction {
            task: "Dar treinamento para Rafael sobre a ferramenta".to_string(),
            owner: "Rafael".to_string(),
            deadline: "terça-feira".to_string(),
            timestamp_sec: 90.0,
            evidence: "a Rafaela vai entrar na sala".to_string(),
        }],
        questions: vec!["Qual horário para Rafael?".to_string()],
        risks: vec![],
    }];

    let payload = build_minutes_fact_payload(&diarized, &insights, None, None).unwrap();
    let html = render_minutes_from_facts_locally(&diarized, &insights, &[], None);

    assert!(payload.contains("Rafaela sera ponto de contato"));
    assert!(payload.contains("Dar treinamento para Rafaela"));
    assert!(payload.contains("\"owner\":\"Rafaela\""));
    assert!(!payload.contains("Rafael sera ponto"));
    assert!(!payload.contains("para Rafael sobre"));
    assert!(html.contains("Dar treinamento para Rafaela"));
    assert!(html.contains("<span>Rafaela</span>"));
}

#[test]
fn meeting_graph_uses_diarized_transcript_to_resolve_gendered_name_variants() {
    let diarized = DiarizedResult {
        speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
        segments: vec![
            DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 12.0,
                end: 16.0,
                text: "O Renato ele esta disponivel no momento.".to_string(),
            },
            DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 660.0,
                end: 670.0,
                text: "Eu pedi para o Renato entender melhor o problema.".to_string(),
            },
        ],
    };
    let insights = vec![MeetingChunkInsights {
        chunk_index: 0,
        start_sec: 0.0,
        end_sec: 120.0,
        summary: "Caio vai sentar com Renata para alinhar a resolucao.".to_string(),
        topics: vec!["Suporte".to_string()],
        decisions: vec![],
        actions: vec![MeetingAction {
            task: "Anotar os problemas e alinhar com Renata para resolucao".to_string(),
            owner: "Caio".to_string(),
            deadline: "semana que vem".to_string(),
            timestamp_sec: 80.0,
            evidence: "sentar com Renata e alinhar isso tudo".to_string(),
        }],
        questions: vec![],
        risks: vec![],
    }];

    let payload = build_minutes_fact_payload(&diarized, &insights, None, None).unwrap();
    let html = render_minutes_from_facts_locally(&diarized, &insights, &[], None);

    assert!(payload.contains("alinhar com Renato"));
    assert!(!payload.contains("Renata"));
    assert!(html.contains("alinhar com Renato"));
    assert!(!html.contains("Renata"));
}

#[test]
fn chunk_fact_generation_config_uses_json_schema_response_format() {
    let config = gemini_chunk_facts_generation_config();

    assert_eq!(
        config["responseMimeType"],
        serde_json::json!("application/json")
    );
    assert_eq!(
        config["responseJsonSchema"]["properties"]["chunks"]["type"],
        serde_json::json!("array")
    );
}

#[test]
fn truncated_chunk_facts_response_falls_back_to_valid_insights() {
    let truncated = r#"{
        "chunkIndex": 0,
        "startSec": 0,
        "endSec": 364.534438,
        "summary": "A reuniao coletou requisitos do projeto.",
        "topics": ["Requisitos"],
        "decisions": [{
            "title": "O sistema precisa funcionar todo dia.",
            "owner": "",
            "timestampSec":
    "#;
    let segments_json = serde_json::json!([
        {
            "speaker": "Falante 1",
            "start": 0.0,
            "end": 10.0,
            "text": "Precisamos que o leitor funcione todos os dias e gere relatorios."
        },
        {
            "speaker": "Falante 2",
            "start": 10.0,
            "end": 20.0,
            "text": "O PDF de alguns clientes falha no processamento."
        }
    ])
    .to_string();

    let insights = parse_chunk_insights_or_fallback(truncated, 0, 0.0, 364.534438, &segments_json);

    assert_eq!(insights.chunk_index, 0);
    assert_eq!(insights.start_sec, 0.0);
    assert_eq!(insights.end_sec, 364.534438);
    assert!(insights
        .summary
        .contains("Precisamos que o leitor funcione"));
    assert!(insights.topics.contains(&"Trecho da reuniao".to_string()));
    assert!(insights.decisions.is_empty());
    assert!(insights.actions.is_empty());
}

#[test]
fn valid_chunk_facts_response_still_uses_model_output() {
    let valid = serde_json::json!({
        "chunkIndex": 99,
        "startSec": 999,
        "endSec": 1000,
        "summary": "Modelo encontrou uma decisao.",
        "topics": ["Produto"],
        "decisions": [{
            "title": "Priorizar leitura de PDF",
            "owner": "",
            "timestampSec": 12.0,
            "evidence": "vamos priorizar PDF"
        }],
        "actions": [],
        "questions": [],
        "risks": []
    })
    .to_string();

    let insights = parse_chunk_insights_or_fallback(&valid, 0, 0.0, 30.0, "[]");

    assert_eq!(insights.chunk_index, 0);
    assert_eq!(insights.start_sec, 0.0);
    assert_eq!(insights.end_sec, 30.0);
    assert_eq!(insights.summary, "Modelo encontrou uma decisao.");
    assert_eq!(insights.decisions[0].title, "Priorizar leitura de PDF");
}
