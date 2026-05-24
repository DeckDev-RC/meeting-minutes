use super::gemini::apply_gemini_thinking_config;
use super::graph::{build_minutes_fact_graph, MinutesFactGraph};
use super::participants::{normalize_participant_names, participant_names_prompt_section};
use crate::models::meeting::MeetingMetadata;
use crate::models::transcription::{DiarizedResult, MeetingChunkInsights};
pub fn build_minutes_fact_payload(
    diarized: &DiarizedResult,
    insights: &[MeetingChunkInsights],
    participant_names: Option<&[String]>,
    meeting_metadata: Option<&MeetingMetadata>,
) -> Result<String, String> {
    let participant_names = normalize_participant_names(participant_names);
    let graph = build_minutes_fact_graph(diarized, insights, &participant_names);

    minutes_fact_payload_from_graph(diarized, &graph, meeting_metadata)
}

pub(super) fn minutes_fact_payload_from_graph(
    diarized: &DiarizedResult,
    graph: &MinutesFactGraph,
    meeting_metadata: Option<&MeetingMetadata>,
) -> Result<String, String> {
    let decision_count = graph.decisions.len();
    let action_count = graph.actions.len();

    let payload = serde_json::json!({
        "speakers": &diarized.speakers,
        "participantNames": &graph.participant_names,
        "stats": {
            "chunkCount": graph.sorted_insights.len(),
            "durationSec": graph.duration_sec,
            "decisionCount": decision_count,
            "actionCount": action_count
        },
        "meetingMetadata": meeting_metadata,
        "meetingGraph": {
            "summaryNotes": &graph.summaries,
            "topics": &graph.topics,
            "decisions": &graph.decisions,
            "actions": &graph.actions,
            "questions": &graph.questions,
            "risks": &graph.risks
        }
    });

    serde_json::to_string(&payload).map_err(|e| e.to_string())
}

pub(super) struct FinalMinutesRequest {
    pub(super) diarized: DiarizedResult,
    pub(super) insights: Vec<MeetingChunkInsights>,
    pub(super) fact_graph: MinutesFactGraph,
    pub(super) body: serde_json::Value,
}

pub(super) fn build_final_minutes_request(
    diarized_json: &str,
    facts_json: &str,
    participant_names: Option<&[String]>,
    meeting_metadata: Option<&MeetingMetadata>,
) -> Result<FinalMinutesRequest, String> {
    let diarized = serde_json::from_str::<DiarizedResult>(diarized_json)
        .map_err(|e| format!("Failed to parse diarized JSON: {e}"))?;
    let insights = serde_json::from_str::<Vec<MeetingChunkInsights>>(facts_json)
        .map_err(|e| format!("Failed to parse meeting facts JSON: {e}"))?;
    let participant_names = normalize_participant_names(participant_names);
    let participant_names_section = participant_names_prompt_section(&participant_names);
    let fact_graph = build_minutes_fact_graph(&diarized, &insights, &participant_names);
    let compact_payload =
        minutes_fact_payload_from_graph(&diarized, &fact_graph, meeting_metadata)?;

    let prompt = format!(
        r#"Voce e especialista em criar atas de reuniao profissionais em portugues brasileiro.

Com base nos fatos estruturados abaixo, gere a ata completa em HTML.
Os fatos ja foram extraidos por trecho; deduplique acoes e decisoes repetidas.
Preserve responsaveis, prazos e evidencias quando existirem.
{participant_names_section}

SECOES OBRIGATORIAS:
1. Cabecalho: titulo da reuniao (infira do contexto), data/hora de "meetingMetadata.recordedAt" se disponivel e duracao se disponivel
2. Participantes: lista de todos os participantes; priorize "participantNames" quando fornecido
3. Pauta: topicos discutidos, com no maximo 8 itens agrupados
4. Resumo Executivo: 3-5 linhas reais usando "summaryNotes"
5. Decisoes Tomadas: lista com <span class="tag-decision">DECISAO</span>
6. Acoes e Responsaveis: tabela class="table-actions" com colunas Acao | Responsavel | Prazo
7. Proximos Passos: lista numerada
8. Observacoes: pontos relevantes restantes

REGRAS DE QUALIDADE:
- NUNCA use placeholders como [Data da Reuniao], [Hora da Reuniao], DD/MM/AAAA ou HH:MM.
- Se data ou hora nao estiverem em "meetingMetadata.recordedAt" ou nos fatos, omita esses campos; nao preencha com modelo.
- Se "meetingMetadata.sourceFileName" existir, inclua como arquivo de origem no cabecalho ou observacoes.
- Se houver decisoes no payload, a secao de decisoes e obrigatoria.
- Se houver acoes no payload, a tabela table-actions e obrigatoria.
- Nao transforme a pauta em uma lista longa de palavras soltas; agrupe topicos repetidos.
- Nao invente fatos, nomes, prazos ou decisoes.

CLASSES CSS DISPONIVEIS (use exatamente):
.header, .section, .table-actions, .tag-decision, .tag-action,
.participants-list, .summary-box

FATOS ESTRUTURADOS:
{}

Retorne APENAS o HTML do conteudo (sem <!DOCTYPE>, sem <html>, sem <head>, sem <body>)."#,
        compact_payload
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": apply_gemini_thinking_config(serde_json::json!({
            "temperature": 0.15,
            "maxOutputTokens": 8192
        }))
    });

    Ok(FinalMinutesRequest {
        diarized,
        insights,
        fact_graph,
        body,
    })
}
