#[cfg(test)]
use super::graph::build_minutes_fact_graph;
use super::graph::{contains_ignore_case, display_owner, MinutesFactGraph};
use super::payload::build_final_minutes_request;
use crate::models::meeting::MeetingMetadata;
use crate::models::transcription::{DiarizedResult, MeetingChunkInsights};
use std::fmt::Write as _;
fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn format_duration(total_sec: f64) -> String {
    let total_sec = total_sec.max(0.0).round() as u64;
    let hours = total_sec / 3600;
    let minutes = (total_sec % 3600) / 60;
    let seconds = total_sec % 60;

    if hours > 0 {
        return format!("{hours}h {minutes:02}m");
    }

    if minutes > 0 {
        return format!("{minutes}m {seconds:02}s");
    }

    format!("{seconds}s")
}

fn format_timestamp(timestamp_sec: f64) -> String {
    let total_sec = timestamp_sec.max(0.0).round() as u64;
    let hours = total_sec / 3600;
    let minutes = (total_sec % 3600) / 60;
    let seconds = total_sec % 60;

    if hours > 0 {
        return format!("{hours}h {minutes:02}m {seconds:02}s");
    }

    format!("{minutes}m {seconds:02}s")
}

fn format_metadata_date_time(recorded_at: &str) -> Option<(String, String)> {
    let datetime = chrono::DateTime::parse_from_rfc3339(recorded_at).ok()?;
    Some((
        datetime.format("%d/%m/%Y").to_string(),
        datetime.format("%H:%M").to_string(),
    ))
}

fn infer_minutes_title(topics: &[String]) -> String {
    let topic = topics
        .iter()
        .map(|topic| topic.trim())
        .find(|topic| !topic.is_empty())
        .unwrap_or("Alinhamento da reunião");

    format!("Reunião de Alinhamento: {topic}")
}

fn push_string_list(html: &mut String, class_name: &str, items: &[String], limit: usize) {
    let _ = write!(html, "<ul class=\"{class_name}\">");
    for item in items.iter().take(limit) {
        let _ = write!(html, "<li>{}</li>", html_escape(item));
    }
    html.push_str("</ul>");
}

fn summarize_remaining_items(total: usize, limit: usize) -> String {
    if total <= limit {
        String::new()
    } else {
        format!(
            "Mais {} item(ns) foram consolidados no resumo para evitar repeticao.",
            total - limit
        )
    }
}

#[cfg(test)]
pub(super) fn render_minutes_from_facts_locally(
    diarized: &DiarizedResult,
    insights: &[MeetingChunkInsights],
    participant_names: &[String],
    meeting_metadata: Option<&MeetingMetadata>,
) -> String {
    let graph = build_minutes_fact_graph(diarized, insights, participant_names);
    render_minutes_fact_graph_locally(diarized, &graph, meeting_metadata)
}

pub(super) fn render_minutes_fact_graph_locally(
    diarized: &DiarizedResult,
    graph: &MinutesFactGraph,
    meeting_metadata: Option<&MeetingMetadata>,
) -> String {
    let sorted_insights = &graph.sorted_insights;
    let topics = &graph.topics;
    let decisions = &graph.decisions;
    let actions = &graph.actions;
    let questions = &graph.questions;
    let risks = &graph.risks;
    let summaries = &graph.summaries;
    let participants = &graph.participants;
    let duration_sec = graph.duration_sec;
    let title = infer_minutes_title(&topics);
    let recorded_date_time = meeting_metadata
        .and_then(|metadata| metadata.recorded_at.as_deref())
        .and_then(format_metadata_date_time);
    let source_file_name = meeting_metadata
        .and_then(|metadata| metadata.source_file_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let topic_limit = 8;
    let action_limit = 14;
    let decision_limit = 10;
    let question_limit = 10;
    let risk_limit = 10;

    let mut html = String::new();
    html.push_str("<div class=\"header\"><p class=\"eyebrow\">Ata consolidada automaticamente</p>");
    let _ = write!(html, "<h1>{}</h1>", html_escape(&title));
    if let Some((date, time)) = &recorded_date_time {
        let _ = write!(
            html,
            "<p class=\"meta\">Data da reunião: {} - Hora: {}</p>",
            html_escape(date),
            html_escape(time)
        );
    }
    let _ = write!(
        html,
        "<p class=\"meta\">Duração analisada: {} - Blocos analisados: {} - Participantes: {}</p>",
        format_duration(duration_sec),
        sorted_insights.len(),
        participants.len().max(diarized.speakers.len())
    );
    if let Some(source_file_name) = source_file_name {
        let _ = write!(
            html,
            "<p class=\"meta\">Arquivo de origem: {}</p>",
            html_escape(source_file_name)
        );
    }
    html.push_str("</div>");

    html.push_str("<div class=\"section\"><h2>Participantes</h2><div class=\"participants-list\">");
    if participants.is_empty() {
        html.push_str("<span>A definir</span>");
    } else {
        for participant in participants {
            let _ = write!(html, "<span>{}</span>", html_escape(participant));
        }
    }
    html.push_str("</div></div>");

    html.push_str("<div class=\"section\"><h2>Pauta Consolidada</h2>");
    if topics.is_empty() {
        html.push_str("<p>Os tópicos principais foram consolidados a partir da transcrição.</p>");
    } else {
        push_string_list(&mut html, "agenda-list", &topics, topic_limit);
        let overflow_note = summarize_remaining_items(topics.len(), topic_limit);
        if !overflow_note.is_empty() {
            let _ = write!(
                html,
                "<p class=\"note\">{}</p>",
                html_escape(&overflow_note)
            );
        }
    }
    html.push_str("</div>");

    html.push_str("<div class=\"section\"><h2>Resumo Executivo</h2><div class=\"summary-box\">");
    if summaries.is_empty() {
        html.push_str("<p>A reunião foi processada e os fatos objetivos foram organizados nas seções abaixo.</p>");
    } else {
        for summary in summaries.iter().take(4) {
            let _ = write!(html, "<p>{}</p>", html_escape(summary));
        }
    }
    if !decisions.is_empty() || !actions.is_empty() {
        let _ = write!(
            html,
            "<p>O encontro gerou {} decisão(ões) e {} ação(ões) rastreáveis, preservando responsáveis, prazos e evidências quando apareceram na fala.</p>",
            decisions.len(),
            actions.len()
        );
    }
    html.push_str("</div></div>");

    html.push_str("<div class=\"section\"><h2>Linha do Tempo</h2>");
    if sorted_insights.is_empty() {
        html.push_str("<p>Não houve blocos de fatos disponíveis para montar a linha do tempo.</p>");
    } else {
        html.push_str("<ul class=\"timeline-list\">");
        for item in sorted_insights.iter().take(8) {
            let topics_for_chunk = if item.topics.is_empty() {
                "sem tópico explícito".to_string()
            } else {
                item.topics
                    .iter()
                    .take(4)
                    .map(|topic| topic.trim())
                    .filter(|topic| !topic.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let summary = if item.summary.trim().is_empty() {
                "Trecho processado sem resumo estruturado.".to_string()
            } else {
                item.summary.trim().to_string()
            };
            let _ = write!(
                html,
                "<li><strong>{} a {}</strong><p>{}</p><p>Temas do trecho: {}.</p></li>",
                html_escape(&format_timestamp(item.start_sec)),
                html_escape(&format_timestamp(item.end_sec)),
                html_escape(&summary),
                html_escape(&topics_for_chunk)
            );
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");

    html.push_str("<div class=\"section\"><h2>Decisões Tomadas</h2>");
    if decisions.is_empty() {
        html.push_str("<p>Nenhuma decisão explícita foi identificada nos fatos extraídos.</p>");
    } else {
        html.push_str("<ul class=\"decision-list\">");
        for decision in decisions.iter().take(decision_limit) {
            let owner = display_owner(&decision.owner);
            let timestamp = format_timestamp(decision.timestamp_sec);
            let _ = write!(
                html,
                "<li><span class=\"tag-decision\">DECISAO</span><strong>{}</strong><p>Responsável: {} • Momento: {}</p>",
                html_escape(&decision.title),
                html_escape(&owner),
                html_escape(&timestamp)
            );
            if !decision.evidence.trim().is_empty() {
                let _ = write!(
                    html,
                    "<blockquote>{}</blockquote>",
                    html_escape(&decision.evidence)
                );
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");

    html.push_str("<div class=\"section\"><h2>Ações e Responsáveis</h2>");
    html.push_str("<table class=\"table-actions\"><thead><tr><th>Ação</th><th>Responsável</th><th>Prazo</th><th>Evidência</th></tr></thead><tbody>");
    if actions.is_empty() {
        html.push_str("<tr><td>Nenhuma ação explícita foi identificada.</td><td>A definir</td><td>A definir</td><td>-</td></tr>");
    } else {
        for action in actions.iter().take(action_limit) {
            let owner = display_owner(&action.owner);
            let deadline = if action.deadline.trim().is_empty() {
                "A definir".to_string()
            } else {
                action.deadline.trim().to_string()
            };
            let evidence = if action.evidence.trim().is_empty() {
                format!("Registrada em {}", format_timestamp(action.timestamp_sec))
            } else {
                action.evidence.trim().to_string()
            };
            let _ = write!(
                html,
                "<tr><td><span class=\"tag-action\">ACAO</span>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&action.task),
                html_escape(&owner),
                html_escape(&deadline),
                html_escape(&evidence)
            );
        }
    }
    html.push_str("</tbody></table></div>");

    html.push_str("<div class=\"section\"><h2>Rastreabilidade</h2>");
    html.push_str("<p>Os itens abaixo conectam a ata aos trechos extraídos, para facilitar conferência posterior sem depender de campos genéricos.</p>");
    html.push_str("<ul class=\"trace-list\">");
    if decisions.is_empty() && actions.is_empty() {
        html.push_str("<li>Nenhuma decisão ou ação explícita foi extraída para rastreamento.</li>");
    } else {
        for decision in decisions.iter().take(decision_limit) {
            let evidence = if decision.evidence.trim().is_empty() {
                "Sem evidência textual curta registrada.".to_string()
            } else {
                decision.evidence.trim().to_string()
            };
            let _ = write!(
                html,
                "<li><strong>Decisão:</strong> {} <span>Momento: {}</span><p>{}</p></li>",
                html_escape(&decision.title),
                html_escape(&format_timestamp(decision.timestamp_sec)),
                html_escape(&evidence)
            );
        }
        for action in actions.iter().take(action_limit) {
            let evidence = if action.evidence.trim().is_empty() {
                "Sem evidência textual curta registrada.".to_string()
            } else {
                action.evidence.trim().to_string()
            };
            let _ = write!(
                html,
                "<li><strong>Ação:</strong> {} <span>Momento: {}</span><p>{}</p></li>",
                html_escape(&action.task),
                html_escape(&format_timestamp(action.timestamp_sec)),
                html_escape(&evidence)
            );
        }
    }
    html.push_str("</ul></div>");

    html.push_str("<div class=\"section\"><h2>Próximos Passos</h2>");
    if actions.is_empty() {
        html.push_str("<ol><li>Revisar a transcrição e confirmar próximos encaminhamentos com os participantes.</li></ol>");
    } else {
        html.push_str("<ol>");
        for action in actions.iter().take(8) {
            let owner = display_owner(&action.owner);
            let deadline = if action.deadline.trim().is_empty() {
                "prazo a definir"
            } else {
                action.deadline.trim()
            };
            let _ = write!(
                html,
                "<li>{} - responsável: {}; prazo: {}.</li>",
                html_escape(&action.task),
                html_escape(&owner),
                html_escape(deadline)
            );
        }
        html.push_str("</ol>");
    }
    html.push_str("</div>");

    if !questions.is_empty() {
        html.push_str("<div class=\"section\"><h2>Perguntas e Pendências</h2>");
        push_string_list(&mut html, "question-list", &questions, question_limit);
        html.push_str("</div>");
    }

    if !risks.is_empty() {
        html.push_str("<div class=\"section\"><h2>Riscos e Bloqueios</h2>");
        push_string_list(&mut html, "risk-list", &risks, risk_limit);
        html.push_str("</div>");
    }

    html.push_str("<div class=\"section\"><h2>Observações</h2>");
    let action_overflow = summarize_remaining_items(actions.len(), action_limit);
    let decision_overflow = summarize_remaining_items(decisions.len(), decision_limit);
    if action_overflow.is_empty() && decision_overflow.is_empty() {
        html.push_str("<p>As informações acima foram montadas a partir dos fatos estruturados extraídos da reunião, sem preencher campos ausentes com placeholders.</p>");
    } else {
        for note in [action_overflow, decision_overflow] {
            if !note.is_empty() {
                let _ = write!(html, "<p>{}</p>", html_escape(&note));
            }
        }
    }
    html.push_str("</div>");

    html
}

pub fn render_ata_from_facts_locally(
    diarized_json: &str,
    facts_json: &str,
    participant_names: Option<&[String]>,
    meeting_metadata: Option<&MeetingMetadata>,
) -> Result<String, String> {
    let request = build_final_minutes_request(
        diarized_json,
        facts_json,
        participant_names,
        meeting_metadata,
    )?;

    Ok(render_minutes_fact_graph_locally(
        &request.diarized,
        &request.fact_graph,
        meeting_metadata,
    ))
}

pub(super) fn minutes_html_is_low_quality(html: &str, insights: &[MeetingChunkInsights]) -> bool {
    let trimmed = html.trim();
    if trimmed.len() < 400 {
        return true;
    }

    let placeholder_markers = [
        "[data",
        "[hora",
        "[nome",
        "data da reunião",
        "data da reuniao",
        "hora da reunião",
        "hora da reuniao",
        "dd/mm",
        "hh:mm",
    ];
    if placeholder_markers
        .iter()
        .any(|marker| contains_ignore_case(trimmed, marker))
    {
        return true;
    }

    let action_count = insights
        .iter()
        .flat_map(|item| item.actions.iter())
        .filter(|action| !action.task.trim().is_empty())
        .count();
    let decision_count = insights
        .iter()
        .flat_map(|item| item.decisions.iter())
        .filter(|decision| !decision.title.trim().is_empty())
        .count();
    let topic_count = insights.iter().map(|item| item.topics.len()).sum::<usize>();
    let question_count = insights
        .iter()
        .map(|item| item.questions.len())
        .sum::<usize>();
    let risk_count = insights.iter().map(|item| item.risks.len()).sum::<usize>();
    let summary_count = insights
        .iter()
        .filter(|item| !item.summary.trim().is_empty())
        .count();
    let fact_count =
        action_count + decision_count + topic_count + question_count + risk_count + summary_count;

    if action_count > 0 && !contains_ignore_case(trimmed, "table-actions") {
        return true;
    }

    if decision_count > 0 && !contains_ignore_case(trimmed, "tag-decision") {
        return true;
    }

    if fact_count >= 8 && trimmed.len() < 1_800 {
        return true;
    }

    if (action_count + decision_count) >= 2 && trimmed.len() < 1_200 {
        return true;
    }

    false
}
