use crate::models::meeting::MeetingMetadata;
use crate::models::transcription::MeetingChunkInsights;
use tauri::{command, AppHandle, State};

mod facts;
mod gemini;
mod graph;
mod participants;
mod payload;
mod render;
mod streaming;

use facts::{
    fact_batch_prompt_payload, gemini_chunk_facts_generation_config,
    gemini_single_chunk_generation_config, parse_batch_insights_or_fallback,
    parse_chunk_insights_or_fallback,
};
use gemini::{apply_gemini_thinking_config, extract_gemini_text, send_gemini_request};
#[cfg(test)]
use gemini::{
    drain_complete_sse_lines, extract_gemini_stream_delta, gemini_request_body_bytes,
    gemini_retry_delay, should_retry_gemini_status,
};
use participants::{normalize_participant_names, participant_names_prompt_section};
use payload::build_final_minutes_request;
use render::{minutes_html_is_low_quality, render_minutes_fact_graph_locally};
use streaming::{emit_minutes_stream_delta, stream_gemini_text};

pub use facts::FactBatchChunkInput;
pub use payload::build_minutes_fact_payload;
pub use render::render_ata_from_facts_locally;

#[cfg(test)]
use graph::{build_minutes_fact_graph, prepare_name_aliases, replace_prepared_name_aliases};
#[cfg(test)]
use render::render_minutes_from_facts_locally;
#[command]
pub async fn extract_chunk_facts(
    http: State<'_, crate::HttpClientState>,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
) -> Result<MeetingChunkInsights, String> {
    extract_chunk_facts_with_client(
        &http.0,
        chunk_index,
        start_sec,
        end_sec,
        segments_json,
        gemini_api_key,
        participant_names,
    )
    .await
}

pub async fn extract_chunk_facts_with_client(
    client: &reqwest::Client,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
) -> Result<MeetingChunkInsights, String> {
    let participant_names = normalize_participant_names(participant_names.as_deref());
    let participant_names_section = participant_names_prompt_section(&participant_names);
    let prompt = format!(
        r#"Voce e um analista de reunioes em portugues brasileiro.
Extraia fatos objetivos deste trecho de transcricao. Nao gere ata completa.
{participant_names_section}

REGRAS:
- Nao invente informacoes.
- Use [] quando nao houver decisoes, acoes, perguntas ou riscos.
- "summary" deve ter no maximo 240 caracteres.
- "topics" deve ter no maximo 6 itens.
- Para cada item de "topics", inclua um item correspondente em "topicEvidence" com title igual, timestampSec e evidence literal da transcricao.
- "decisions" deve ter no maximo 5 itens.
- "actions" deve ter no maximo 8 itens.
- "questions" e "risks" devem ter no maximo 6 itens cada.
- "evidence" deve ter no maximo 120 caracteres e deve aparecer na transcricao.
- "timestampSec" deve ser o tempo aproximado em segundos do fato.
- Se houver muitos fatos repetidos, escolha apenas os mais importantes.
- Retorne SOMENTE JSON valido, sem markdown.

FORMATO:
{{
  "chunkIndex": {chunk_index},
  "startSec": {start_sec},
  "endSec": {end_sec},
  "summary": "resumo curto do trecho",
  "topics": ["topico"],
  "topicEvidence": [
    {{
      "title": "topico",
      "timestampSec": 0,
      "evidence": "trecho curto"
    }}
  ],
  "decisions": [
    {{
      "title": "decisao objetiva",
      "owner": "responsavel ou vazio",
      "timestampSec": 0,
      "evidence": "trecho curto"
    }}
  ],
  "actions": [
    {{
      "task": "acao objetiva",
      "owner": "responsavel ou vazio",
      "deadline": "prazo ou vazio",
      "timestampSec": 0,
      "evidence": "trecho curto"
    }}
  ],
  "questions": ["pergunta ou pendencia"],
  "risks": ["risco ou bloqueio"]
}}

TRECHO:
{}"#,
        segments_json
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": gemini_single_chunk_generation_config()
    });

    let result = send_gemini_request(client, &gemini_api_key, &body).await?;
    let text = extract_gemini_text(result)?;
    Ok(parse_chunk_insights_or_fallback(
        &text,
        chunk_index,
        start_sec,
        end_sec,
        &segments_json,
    ))
}

#[command]
pub async fn extract_fact_batch(
    http: State<'_, crate::HttpClientState>,
    chunks: Vec<FactBatchChunkInput>,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
) -> Result<Vec<MeetingChunkInsights>, String> {
    extract_fact_batch_with_client(&http.0, chunks, gemini_api_key, participant_names).await
}

pub async fn extract_fact_batch_with_client(
    client: &reqwest::Client,
    chunks: Vec<FactBatchChunkInput>,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
) -> Result<Vec<MeetingChunkInsights>, String> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let participant_names = normalize_participant_names(participant_names.as_deref());
    let participant_names_section = participant_names_prompt_section(&participant_names);
    let chunk_payload = fact_batch_prompt_payload(&chunks);

    let prompt = format!(
        r#"Voce e um analista de reunioes em portugues brasileiro.
Extraia fatos objetivos dos trechos abaixo. Nao gere ata completa.
{participant_names_section}

REGRAS:
- Nao invente informacoes.
- Retorne um item em "chunks" para cada chunkIndex recebido.
- Use [] quando nao houver decisoes, acoes, perguntas ou riscos.
- "summary" deve ter no maximo 240 caracteres por trecho.
- "topics" deve ter no maximo 6 itens por trecho.
- Para cada item de "topics", inclua um item correspondente em "topicEvidence" com title igual, timestampSec e evidence literal da transcricao.
- "decisions" deve ter no maximo 5 itens por trecho.
- "actions" deve ter no maximo 8 itens por trecho.
- "questions" e "risks" devem ter no maximo 6 itens cada.
- "evidence" deve ter no maximo 120 caracteres e deve aparecer na transcricao.
- "timestampSec" deve ser o tempo aproximado em segundos do fato.
- Se houver muitos fatos repetidos, escolha apenas os mais importantes.

TRECHOS:
{}"#,
        chunk_payload
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": gemini_chunk_facts_generation_config()
    });

    let result = send_gemini_request(client, &gemini_api_key, &body).await?;
    let text = extract_gemini_text(result)?;
    Ok(parse_batch_insights_or_fallback(&text, &chunks))
}

#[command]
pub async fn generate_ata_from_facts(
    http: State<'_, crate::HttpClientState>,
    diarized_json: String,
    facts_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
    meeting_metadata: Option<MeetingMetadata>,
    prefer_local: Option<bool>,
) -> Result<String, String> {
    generate_ata_from_facts_with_client(
        &http.0,
        diarized_json,
        facts_json,
        gemini_api_key,
        participant_names,
        meeting_metadata,
        prefer_local.unwrap_or(false),
    )
    .await
}

#[command]
pub async fn generate_ata_from_facts_streaming(
    app: AppHandle,
    http: State<'_, crate::HttpClientState>,
    meeting_id: String,
    diarized_json: String,
    facts_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
    meeting_metadata: Option<MeetingMetadata>,
    prefer_local: Option<bool>,
) -> Result<String, String> {
    let request = build_final_minutes_request(
        &diarized_json,
        &facts_json,
        participant_names.as_deref(),
        meeting_metadata.as_ref(),
    )?;

    if prefer_local.unwrap_or(false) {
        let html = render_minutes_fact_graph_locally(
            &request.diarized,
            &request.fact_graph,
            meeting_metadata.as_ref(),
        );
        emit_minutes_stream_delta(&app, &meeting_id, html.clone(), true);
        return Ok(html);
    }

    let html = match stream_gemini_text(&http.0, &gemini_api_key, &request.body, &app, &meeting_id)
        .await
    {
        Ok(html) if !html.trim().is_empty() => html,
        _ => {
            let result = send_gemini_request(&http.0, &gemini_api_key, &request.body).await?;
            let html = extract_gemini_text(result)?;
            emit_minutes_stream_delta(&app, &meeting_id, html.clone(), true);
            html
        }
    };

    if minutes_html_is_low_quality(&html, &request.insights) {
        let fallback = render_minutes_fact_graph_locally(
            &request.diarized,
            &request.fact_graph,
            meeting_metadata.as_ref(),
        );
        emit_minutes_stream_delta(&app, &meeting_id, fallback.clone(), true);
        return Ok(fallback);
    }

    Ok(html)
}

pub async fn generate_ata_from_facts_with_client(
    client: &reqwest::Client,
    diarized_json: String,
    facts_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
    meeting_metadata: Option<MeetingMetadata>,
    prefer_local: bool,
) -> Result<String, String> {
    let request = build_final_minutes_request(
        &diarized_json,
        &facts_json,
        participant_names.as_deref(),
        meeting_metadata.as_ref(),
    );
    let request = request?;

    if prefer_local {
        return Ok(render_minutes_fact_graph_locally(
            &request.diarized,
            &request.fact_graph,
            meeting_metadata.as_ref(),
        ));
    }

    let result = send_gemini_request(client, &gemini_api_key, &request.body).await?;
    let html = extract_gemini_text(result)?;
    if minutes_html_is_low_quality(&html, &request.insights) {
        return Ok(render_minutes_fact_graph_locally(
            &request.diarized,
            &request.fact_graph,
            meeting_metadata.as_ref(),
        ));
    }

    Ok(html)
}

#[command]
pub async fn generate_ata_html(
    http: State<'_, crate::HttpClientState>,
    diarized_json: String,
    gemini_api_key: String,
) -> Result<String, String> {
    generate_ata_html_with_client(&http.0, diarized_json, gemini_api_key).await
}

pub async fn generate_ata_html_with_client(
    client: &reqwest::Client,
    diarized_json: String,
    gemini_api_key: String,
) -> Result<String, String> {
    let prompt = format!(
        r#"Voce e especialista em criar atas de reuniao profissionais em portugues brasileiro.

Com base na transcricao diarizada abaixo, gere a ata completa em HTML.

SECOES OBRIGATORIAS:
1. Cabecalho: titulo da reuniao (infira do contexto), data/hora se mencionada
2. Participantes: lista de todos os falantes
3. Pauta: topicos discutidos (extraia do conteudo)
4. Resumo Executivo: 3-5 linhas sobre o que foi decidido
5. Decisoes Tomadas: lista com <span class="tag-decision">DECISAO</span>
6. Acoes e Responsaveis: tabela class="table-actions" com colunas Acao | Responsavel | Prazo
7. Proximos Passos: lista numerada
8. Observacoes: pontos relevantes restantes

CLASSES CSS DISPONIVEIS (use exatamente):
.header, .section, .table-actions, .tag-decision, .tag-action,
.participants-list, .summary-box

TRANSCRICAO DIARIZADA:
{}

Retorne APENAS o HTML do conteudo (sem <!DOCTYPE>, sem <html>, sem <head>, sem <body>)."#,
        diarized_json
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": apply_gemini_thinking_config(serde_json::json!({
            "temperature": 0.3,
            "maxOutputTokens": 8192
        }))
    });

    let result = send_gemini_request(client, &gemini_api_key, &body).await?;
    extract_gemini_text(result)
}

#[cfg(test)]
mod tests;
