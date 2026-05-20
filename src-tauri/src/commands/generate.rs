use crate::models::meeting::MeetingMetadata;
use crate::models::transcription::{
    DiarizedResult, MeetingAction, MeetingChunkInsights, MeetingDecision,
};
use aho_corasick::AhoCorasick;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::time::Duration;
use tauri::{command, AppHandle, Emitter, State};

const GEMINI_MODEL: &str = "gemini-2.5-flash";
const MINUTES_STREAM_EVENT: &str = "meeting-minutes://minutes-stream";

fn gemini_generate_url(gemini_api_key: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent?key={}",
        gemini_api_key
    )
}

fn gemini_stream_url(gemini_api_key: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:streamGenerateContent?alt=sse&key={}",
        gemini_api_key
    )
}

fn should_retry_gemini_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn gemini_retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt).max(1))
}

async fn send_gemini_request(
    client: &reqwest::Client,
    gemini_api_key: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let max_attempts = 3u32;

    for attempt in 0..max_attempts {
        let response = client
            .post(gemini_generate_url(gemini_api_key))
            .json(body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.json().await.map_err(|e| e.to_string());
                }

                let err = resp.text().await.unwrap_or_default();
                if should_retry_gemini_status(status) && attempt < max_attempts - 1 {
                    tokio::time::sleep(gemini_retry_delay(attempt)).await;
                    continue;
                }

                return Err(format!("Gemini API error {}: {}", status, err));
            }
            Err(err) => {
                if attempt < max_attempts - 1 && (err.is_connect() || err.is_timeout()) {
                    tokio::time::sleep(gemini_retry_delay(attempt)).await;
                    continue;
                }

                return Err(err.to_string());
            }
        }
    }

    Err("Gemini API error: max retries exceeded".to_string())
}

fn extract_gemini_text(result: serde_json::Value) -> Result<String, String> {
    result["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(strip_markdown_code_fence)
        .ok_or("Empty response from Gemini".to_string())
}

fn extract_gemini_stream_delta(line: &str) -> Result<Option<String>, String> {
    let Some(raw) = line.trim().strip_prefix("data:") else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "[DONE]" {
        return Ok(None);
    }

    let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|e| e.to_string())?;
    Ok(value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(ToString::to_string))
}

fn strip_markdown_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let Some(first_newline) = trimmed.find('\n') else {
        return trimmed.to_string();
    };
    let body = &trimmed[first_newline + 1..];
    let body = body.strip_suffix("```").unwrap_or(body).trim();

    body.to_string()
}

fn normalize_insights(
    mut insights: MeetingChunkInsights,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
) -> MeetingChunkInsights {
    insights.chunk_index = chunk_index;
    insights.start_sec = start_sec;
    insights.end_sec = end_sec;
    insights
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    output.trim().to_string()
}

fn extract_segment_text(segments_json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(segments_json) else {
        return String::new();
    };
    let Some(items) = value.as_array() else {
        return String::new();
    };

    items
        .iter()
        .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn fallback_chunk_insights(
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: &str,
) -> MeetingChunkInsights {
    let text = extract_segment_text(segments_json);
    let summary = if text.is_empty() {
        "Trecho processado sem fatos estruturados extraidos.".to_string()
    } else {
        truncate_chars(&text, 420)
    };

    MeetingChunkInsights {
        chunk_index,
        start_sec,
        end_sec,
        summary,
        topics: vec!["Trecho da reuniao".to_string()],
        decisions: Vec::new(),
        actions: Vec::new(),
        questions: Vec::new(),
        risks: Vec::new(),
    }
}

fn parse_chunk_insights_or_fallback(
    text: &str,
    chunk_index: usize,
    start_sec: f64,
    end_sec: f64,
    segments_json: &str,
) -> MeetingChunkInsights {
    serde_json::from_str::<MeetingChunkInsights>(text)
        .map(|insights| normalize_insights(insights, chunk_index, start_sec, end_sec))
        .unwrap_or_else(|_| fallback_chunk_insights(chunk_index, start_sec, end_sec, segments_json))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactBatchChunkInput {
    pub chunk_index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub segments_json: String,
}

#[derive(Debug, Deserialize)]
struct MeetingFactBatchResponse {
    #[serde(default)]
    chunks: Vec<MeetingChunkInsights>,
}

fn parse_batch_insights_or_fallback(
    text: &str,
    chunks: &[FactBatchChunkInput],
) -> Vec<MeetingChunkInsights> {
    let parsed = serde_json::from_str::<MeetingFactBatchResponse>(text)
        .map(|response| response.chunks)
        .or_else(|_| serde_json::from_str::<Vec<MeetingChunkInsights>>(text));

    let mut parsed = match parsed {
        Ok(items) => items,
        Err(_) => Vec::new(),
    };
    let source_by_chunk = chunks
        .iter()
        .map(|chunk| (chunk.chunk_index, chunk))
        .collect::<HashMap<_, _>>();

    for item in &mut parsed {
        if let Some(source) = source_by_chunk.get(&item.chunk_index) {
            item.start_sec = source.start_sec;
            item.end_sec = source.end_sec;
        }
    }

    let parsed_by_chunk = parsed
        .into_iter()
        .map(|item| (item.chunk_index, item))
        .collect::<HashMap<_, _>>();

    chunks
        .iter()
        .map(|chunk| {
            parsed_by_chunk
                .get(&chunk.chunk_index)
                .cloned()
                .unwrap_or_else(|| {
                    fallback_chunk_insights(
                        chunk.chunk_index,
                        chunk.start_sec,
                        chunk.end_sec,
                        &chunk.segments_json,
                    )
                })
        })
        .collect()
}

fn normalize_participant_names(participant_names: Option<&[String]>) -> Vec<String> {
    let mut names = Vec::new();

    for name in participant_names.unwrap_or(&[]) {
        let trimmed = name.trim();
        if trimmed.is_empty() || names.iter().any(|existing| existing == trimmed) {
            continue;
        }
        names.push(trimmed.to_string());
    }

    names
}

fn participant_names_prompt_section(participant_names: &[String]) -> String {
    if participant_names.is_empty() {
        return String::new();
    }

    format!(
        "\nNOMES INFORMADOS PELO USUARIO:\n{}\n\nREGRAS PARA NOMES:\n- Use esta lista como fonte preferencial para nomes de participantes e responsaveis.\n- Corrija variacoes foneticas ou de transcricao quando forem claramente compativeis com a lista.\n- Nao invente nomes fora da lista. Se nao houver confianca, mantenha o campo vazio ou o rotulo Falante N.\n",
        participant_names.join(", ")
    )
}

fn normalize_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn unique_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for value in values {
        let trimmed = value.trim();
        let key = normalize_key(trimmed);
        if trimmed.is_empty() || !seen.insert(key) {
            continue;
        }
        output.push(trimmed.to_string());
    }

    output
}

fn unique_decisions(values: impl IntoIterator<Item = MeetingDecision>) -> Vec<MeetingDecision> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for decision in values {
        let key = normalize_key(&decision.title);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        output.push(decision);
    }

    output
}

fn unique_actions(values: impl IntoIterator<Item = MeetingAction>) -> Vec<MeetingAction> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for action in values {
        let key = format!(
            "{}|{}|{}",
            normalize_key(&action.task),
            normalize_key(&action.owner),
            normalize_key(&action.deadline)
        );
        if normalize_key(&action.task).is_empty() || !seen.insert(key) {
            continue;
        }
        output.push(action);
    }

    output
}

fn push_name_alias(aliases: &mut Vec<(String, String)>, alias: String, canonical: String) {
    if alias == canonical
        || alias.trim().is_empty()
        || canonical.trim().is_empty()
        || aliases
            .iter()
            .any(|(existing_alias, _)| normalize_key(existing_alias) == normalize_key(&alias))
    {
        return;
    }

    aliases.push((alias, canonical));
}

fn word_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_alphabetic() {
            current.push(ch);
        } else if !current.is_empty() {
            let token = std::mem::take(&mut current);
            if seen.insert(normalize_key(&token)) {
                tokens.push(token);
            }
        }
    }

    if !current.is_empty() && seen.insert(normalize_key(&current)) {
        tokens.push(current);
    }

    tokens
}

fn word_tokens_lower_ordered(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_alphabetic() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn word_context_ngrams(value: &str) -> HashSet<String> {
    let tokens = word_tokens_lower_ordered(value);
    let mut contexts = HashSet::with_capacity(tokens.len().saturating_mul(2));

    for window in tokens.windows(2) {
        contexts.insert(format!("{} {}", window[0], window[1]));
    }
    for window in tokens.windows(3) {
        contexts.insert(format!("{} {} {}", window[0], window[1], window[2]));
    }

    contexts
}

fn collect_insight_text(
    insights: &[MeetingChunkInsights],
    diarized: Option<&DiarizedResult>,
) -> String {
    let mut text = String::new();

    if let Some(diarized) = diarized {
        for segment in &diarized.segments {
            text.push_str(&segment.text);
            text.push('\n');
        }
    }

    for insight in insights {
        text.push_str(&insight.summary);
        text.push('\n');
        for topic in &insight.topics {
            text.push_str(topic);
            text.push('\n');
        }
        for decision in &insight.decisions {
            text.push_str(&decision.title);
            text.push('\n');
            text.push_str(&decision.owner);
            text.push('\n');
            text.push_str(&decision.evidence);
            text.push('\n');
        }
        for action in &insight.actions {
            text.push_str(&action.task);
            text.push('\n');
            text.push_str(&action.owner);
            text.push('\n');
            text.push_str(&action.deadline);
            text.push('\n');
            text.push_str(&action.evidence);
            text.push('\n');
        }
        for question in &insight.questions {
            text.push_str(question);
            text.push('\n');
        }
        for risk in &insight.risks {
            text.push_str(risk);
            text.push('\n');
        }
    }

    text
}

fn build_name_aliases(
    participant_names: &[String],
    insights: &[MeetingChunkInsights],
    diarized: Option<&DiarizedResult>,
) -> Vec<(String, String)> {
    let mut aliases = Vec::new();
    let explicit_names = participant_names
        .iter()
        .map(|name| normalize_key(name))
        .collect::<HashSet<_>>();

    for name in participant_names {
        let trimmed = name.trim();
        let lower = trimmed.to_lowercase();
        if lower.ends_with('a') && trimmed.chars().count() > 4 {
            let mut alias = trimmed.to_string();
            alias.pop();
            push_name_alias(&mut aliases, alias, trimmed.to_string());
        }
    }

    let corpus = collect_insight_text(insights, diarized);
    let corpus_contexts = word_context_ngrams(&corpus);
    let tokens = word_tokens(&corpus);
    let token_set = tokens
        .iter()
        .map(|token| normalize_key(token))
        .collect::<HashSet<_>>();

    for token in tokens {
        let lower = token.to_lowercase();
        if !lower.ends_with('a') || token.chars().count() <= 4 {
            continue;
        }

        let mut masculine = token.clone();
        masculine.pop();
        masculine.push('o');
        if token_set.contains(&normalize_key(&masculine))
            && !explicit_names.contains(&normalize_key(&masculine))
            && !explicit_names.contains(&normalize_key(&token))
        {
            let masculine_lower = masculine.to_lowercase();
            let has_masculine_context = [
                format!("o {masculine_lower}"),
                format!("do {masculine_lower}"),
                format!("para o {masculine_lower}"),
                format!("com o {masculine_lower}"),
            ]
            .iter()
            .any(|pattern| corpus_contexts.contains(pattern));

            let has_feminine_context = [
                format!("a {lower}"),
                format!("da {lower}"),
                format!("para a {lower}"),
                format!("com a {lower}"),
            ]
            .iter()
            .any(|pattern| corpus_contexts.contains(pattern));

            if has_masculine_context && !has_feminine_context {
                push_name_alias(&mut aliases, token.clone(), masculine);
            }
        }

        let mut alias = token.clone();
        alias.pop();
        if !token_set.contains(&normalize_key(&alias))
            || explicit_names.contains(&normalize_key(&alias))
            || explicit_names.contains(&normalize_key(&token))
        {
            continue;
        }

        let canonical_lower = token.to_lowercase();
        let has_contextual_article = [
            format!("a {canonical_lower}"),
            format!("da {canonical_lower}"),
            format!("para a {canonical_lower}"),
            format!("com a {canonical_lower}"),
        ]
        .iter()
        .any(|pattern| corpus_contexts.contains(pattern));

        if has_contextual_article {
            push_name_alias(&mut aliases, alias, token.clone());
        }
    }

    aliases
}

fn is_name_boundary(ch: Option<char>) -> bool {
    ch.map(|value| !value.is_alphabetic()).unwrap_or(true)
}

#[derive(Debug)]
struct PreparedNameAlias {
    alias: String,
    canonical: String,
}

struct PreparedNameAliases {
    entries: Vec<PreparedNameAlias>,
    matcher: Option<AhoCorasick>,
}

fn prepare_name_aliases(aliases: &[(String, String)]) -> PreparedNameAliases {
    let entries = aliases
        .iter()
        .filter(|(alias, _)| !alias.is_empty())
        .map(|(alias, canonical)| PreparedNameAlias {
            alias: alias.clone(),
            canonical: canonical.clone(),
        })
        .collect::<Vec<_>>();
    let matcher = if entries.is_empty() {
        None
    } else {
        let patterns = entries
            .iter()
            .map(|entry| entry.alias.as_str())
            .collect::<Vec<_>>();
        Some(AhoCorasick::new(patterns).expect("valid name alias patterns"))
    };

    PreparedNameAliases { entries, matcher }
}

fn char_before(value: &str, byte_index: usize) -> Option<char> {
    value[..byte_index].chars().next_back()
}

fn char_after(value: &str, byte_index: usize) -> Option<char> {
    value[byte_index..].chars().next()
}

fn replace_prepared_name_aliases(value: &str, aliases: &PreparedNameAliases) -> String {
    let Some(matcher) = aliases.matcher.as_ref() else {
        return value.to_string();
    };
    if value.is_empty() {
        return value.to_string();
    }

    let mut output = String::with_capacity(value.len());
    let mut last_end = 0usize;

    for matched in matcher.find_iter(value) {
        if matched.start() < last_end {
            continue;
        }
        if !is_name_boundary(char_before(value, matched.start()))
            || !is_name_boundary(char_after(value, matched.end()))
        {
            continue;
        }

        output.push_str(&value[last_end..matched.start()]);
        output.push_str(&aliases.entries[matched.pattern().as_usize()].canonical);
        last_end = matched.end();
    }

    output.push_str(&value[last_end..]);
    output
}

fn normalize_decision_names(
    mut decision: MeetingDecision,
    aliases: &PreparedNameAliases,
) -> MeetingDecision {
    decision.title = replace_prepared_name_aliases(&decision.title, aliases);
    decision.owner = replace_prepared_name_aliases(&decision.owner, aliases);
    decision.evidence = replace_prepared_name_aliases(&decision.evidence, aliases);
    decision
}

fn normalize_action_names(
    mut action: MeetingAction,
    aliases: &PreparedNameAliases,
) -> MeetingAction {
    action.task = replace_prepared_name_aliases(&action.task, aliases);
    action.owner = replace_prepared_name_aliases(&action.owner, aliases);
    action.deadline = replace_prepared_name_aliases(&action.deadline, aliases);
    action.evidence = replace_prepared_name_aliases(&action.evidence, aliases);
    action
}

fn normalize_insight_names(
    mut insight: MeetingChunkInsights,
    aliases: &PreparedNameAliases,
) -> MeetingChunkInsights {
    insight.summary = replace_prepared_name_aliases(&insight.summary, aliases);
    insight.topics = insight
        .topics
        .into_iter()
        .map(|topic| replace_prepared_name_aliases(&topic, aliases))
        .collect();
    insight.decisions = insight
        .decisions
        .into_iter()
        .map(|decision| normalize_decision_names(decision, aliases))
        .collect();
    insight.actions = insight
        .actions
        .into_iter()
        .map(|action| normalize_action_names(action, aliases))
        .collect();
    insight.questions = insight
        .questions
        .into_iter()
        .map(|question| replace_prepared_name_aliases(&question, aliases))
        .collect();
    insight.risks = insight
        .risks
        .into_iter()
        .map(|risk| replace_prepared_name_aliases(&risk, aliases))
        .collect();
    insight
}

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
    let datetime = datetime.with_timezone(&chrono::Local);
    Some((
        datetime.format("%d/%m/%Y").to_string(),
        datetime.format("%H:%M").to_string(),
    ))
}

fn display_owner(owner: &str) -> String {
    let trimmed = owner.trim();
    let lower = trimmed.to_lowercase();
    if trimmed.is_empty() || lower.starts_with("falante ") || lower == "speaker" {
        "A definir".to_string()
    } else {
        trimmed.to_string()
    }
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

#[derive(Debug, Clone)]
struct MinutesFactGraph {
    participant_names: Vec<String>,
    sorted_insights: Vec<MeetingChunkInsights>,
    topics: Vec<String>,
    decisions: Vec<MeetingDecision>,
    actions: Vec<MeetingAction>,
    questions: Vec<String>,
    risks: Vec<String>,
    summaries: Vec<String>,
    participants: Vec<String>,
    duration_sec: f64,
}

fn build_minutes_fact_graph(
    diarized: &DiarizedResult,
    insights: &[MeetingChunkInsights],
    participant_names: &[String],
) -> MinutesFactGraph {
    let participant_names = normalize_participant_names(Some(participant_names));
    let aliases = build_name_aliases(&participant_names, insights, Some(diarized));
    let aliases = prepare_name_aliases(&aliases);
    let mut sorted_insights = insights
        .iter()
        .cloned()
        .map(|insight| normalize_insight_names(insight, &aliases))
        .collect::<Vec<_>>();
    sorted_insights.sort_by_key(|item| item.chunk_index);

    let mut topic_values = Vec::new();
    let mut decision_values = Vec::new();
    let mut action_values = Vec::new();
    let mut question_values = Vec::new();
    let mut risk_values = Vec::new();
    let mut summary_values = Vec::new();
    let mut duration_sec = diarized
        .segments
        .iter()
        .map(|segment| segment.end)
        .fold(0.0_f64, f64::max);

    for item in &sorted_insights {
        duration_sec = duration_sec.max(item.end_sec);
        topic_values.extend(item.topics.iter().cloned());
        decision_values.extend(item.decisions.iter().cloned());
        action_values.extend(item.actions.iter().cloned());
        question_values.extend(item.questions.iter().cloned());
        risk_values.extend(item.risks.iter().cloned());

        let summary = item.summary.trim();
        if !summary.is_empty() {
            summary_values.push(summary.to_string());
        }
    }

    let topics = unique_strings(topic_values);
    let decisions = unique_decisions(decision_values);
    let actions = unique_actions(action_values);
    let questions = unique_strings(question_values);
    let risks = unique_strings(risk_values);
    let summaries = unique_strings(summary_values);

    let participants = if participant_names.is_empty() {
        let owner_names = decisions
            .iter()
            .map(|decision| decision.owner.clone())
            .chain(actions.iter().map(|action| action.owner.clone()))
            .flat_map(|owner| {
                owner
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && display_owner(name) != "A definir")
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            });
        unique_strings(diarized.speakers.clone().into_iter().chain(owner_names))
    } else {
        let owner_names = decisions
            .iter()
            .map(|decision| decision.owner.clone())
            .chain(actions.iter().map(|action| action.owner.clone()))
            .flat_map(|owner| {
                owner
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && display_owner(name) != "A definir")
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            });
        unique_strings(participant_names.clone().into_iter().chain(owner_names))
    };
    MinutesFactGraph {
        participant_names,
        sorted_insights,
        topics,
        decisions,
        actions,
        questions,
        risks,
        summaries,
        participants,
        duration_sec,
    }
}

#[cfg(test)]
fn render_minutes_from_facts_locally(
    diarized: &DiarizedResult,
    insights: &[MeetingChunkInsights],
    participant_names: &[String],
    meeting_metadata: Option<&MeetingMetadata>,
) -> String {
    let graph = build_minutes_fact_graph(diarized, insights, participant_names);
    render_minutes_fact_graph_locally(diarized, &graph, meeting_metadata)
}

fn render_minutes_fact_graph_locally(
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

fn minutes_html_is_low_quality(html: &str, insights: &[MeetingChunkInsights]) -> bool {
    let trimmed = html.trim();
    if trimmed.len() < 400 {
        return true;
    }

    let lower = trimmed.to_lowercase();
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
        .any(|marker| lower.contains(marker))
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

    if action_count > 0 && !lower.contains("table-actions") {
        return true;
    }

    if decision_count > 0 && !lower.contains("tag-decision") {
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

fn chunk_fact_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "chunkIndex": { "type": "integer" },
            "startSec": { "type": "number" },
            "endSec": { "type": "number" },
            "summary": { "type": "string" },
            "topics": { "type": "array", "items": { "type": "string" } },
            "decisions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "owner": { "type": "string" },
                        "timestampSec": { "type": "number" },
                        "evidence": { "type": "string" }
                    },
                    "required": ["title", "owner", "timestampSec", "evidence"],
                    "additionalProperties": false
                }
            },
            "actions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "task": { "type": "string" },
                        "owner": { "type": "string" },
                        "deadline": { "type": "string" },
                        "timestampSec": { "type": "number" },
                        "evidence": { "type": "string" }
                    },
                    "required": ["task", "owner", "deadline", "timestampSec", "evidence"],
                    "additionalProperties": false
                }
            },
            "questions": { "type": "array", "items": { "type": "string" } },
            "risks": { "type": "array", "items": { "type": "string" } }
        },
        "required": [
            "chunkIndex",
            "startSec",
            "endSec",
            "summary",
            "topics",
            "decisions",
            "actions",
            "questions",
            "risks"
        ],
        "additionalProperties": false
    })
}

pub fn gemini_chunk_facts_generation_config() -> serde_json::Value {
    serde_json::json!({
        "temperature": 0.1,
        "maxOutputTokens": 8192,
        "responseMimeType": "application/json",
        "responseJsonSchema": {
            "type": "object",
            "properties": {
                "chunks": {
                    "type": "array",
                    "items": chunk_fact_schema()
                }
            },
            "required": ["chunks"],
            "additionalProperties": false
        }
    })
}

fn gemini_single_chunk_generation_config() -> serde_json::Value {
    serde_json::json!({
        "temperature": 0.1,
        "maxOutputTokens": 8192,
        "responseMimeType": "application/json",
        "responseJsonSchema": chunk_fact_schema()
    })
}

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

fn minutes_fact_payload_from_graph(
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
- "decisions" deve ter no maximo 5 itens.
- "actions" deve ter no maximo 8 itens.
- "questions" e "risks" devem ter no maximo 6 itens cada.
- "evidence" deve ter no maximo 120 caracteres.
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
- "decisions" deve ter no maximo 5 itens por trecho.
- "actions" deve ter no maximo 8 itens por trecho.
- "questions" e "risks" devem ter no maximo 6 itens cada.
- "evidence" deve ter no maximo 120 caracteres.
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

fn json_array_or_empty(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed
    } else {
        "[]"
    }
}

fn fact_batch_prompt_payload(chunks: &[FactBatchChunkInput]) -> String {
    let mut payload = String::with_capacity(chunks.len().saturating_mul(512));
    payload.push('[');
    for (index, chunk) in chunks.iter().enumerate() {
        if index > 0 {
            payload.push(',');
        }
        let _ = write!(
            payload,
            "{{\"chunkIndex\":{},\"startSec\":{},\"endSec\":{},\"segments\":{}}}",
            chunk.chunk_index,
            chunk.start_sec,
            chunk.end_sec,
            json_array_or_empty(&chunk.segments_json)
        );
    }
    payload.push(']');
    payload
}

struct FinalMinutesRequest {
    diarized: DiarizedResult,
    insights: Vec<MeetingChunkInsights>,
    fact_graph: MinutesFactGraph,
    body: serde_json::Value,
}

fn build_final_minutes_request(
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
        "generationConfig": {
            "temperature": 0.15,
            "maxOutputTokens": 8192
        }
    });

    Ok(FinalMinutesRequest {
        diarized,
        insights,
        fact_graph,
        body,
    })
}

#[command]
pub async fn generate_ata_from_facts(
    http: State<'_, crate::HttpClientState>,
    diarized_json: String,
    facts_json: String,
    gemini_api_key: String,
    participant_names: Option<Vec<String>>,
    meeting_metadata: Option<MeetingMetadata>,
) -> Result<String, String> {
    generate_ata_from_facts_with_client(
        &http.0,
        diarized_json,
        facts_json,
        gemini_api_key,
        participant_names,
        meeting_metadata,
    )
    .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MinutesStreamPayload {
    meeting_id: String,
    delta: String,
    done: bool,
}

fn emit_minutes_stream_delta(app: &AppHandle, meeting_id: &str, delta: String, done: bool) {
    let _ = app.emit(
        MINUTES_STREAM_EVENT,
        MinutesStreamPayload {
            meeting_id: meeting_id.to_string(),
            delta,
            done,
        },
    );
}

async fn stream_gemini_text(
    client: &reqwest::Client,
    gemini_api_key: &str,
    body: &serde_json::Value,
    app: &AppHandle,
    meeting_id: &str,
) -> Result<String, String> {
    let response = client
        .post(gemini_stream_url(gemini_api_key))
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, err));
    }

    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut full_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        pending.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = pending.find('\n') {
            let line = pending[..line_end].trim_end_matches('\r').to_string();
            pending = pending[line_end + 1..].to_string();
            if let Some(delta) = extract_gemini_stream_delta(&line)? {
                full_text.push_str(&delta);
                emit_minutes_stream_delta(app, meeting_id, delta, false);
            }
        }
    }

    if !pending.trim().is_empty() {
        if let Some(delta) = extract_gemini_stream_delta(&pending)? {
            full_text.push_str(&delta);
            emit_minutes_stream_delta(app, meeting_id, delta, false);
        }
    }

    let html = strip_markdown_code_fence(&full_text);
    emit_minutes_stream_delta(app, meeting_id, String::new(), true);
    Ok(html)
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
) -> Result<String, String> {
    let request = build_final_minutes_request(
        &diarized_json,
        &facts_json,
        participant_names.as_deref(),
        meeting_metadata.as_ref(),
    )?;

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
) -> Result<String, String> {
    let request = build_final_minutes_request(
        &diarized_json,
        &facts_json,
        participant_names.as_deref(),
        meeting_metadata.as_ref(),
    );
    let request = request?;

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
        "generationConfig": {
            "temperature": 0.3,
            "maxOutputTokens": 8192
        }
    });

    let result = send_gemini_request(client, &gemini_api_key, &body).await?;
    extract_gemini_text(result)
}

#[cfg(test)]
mod tests {
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
            summary: "Emanuella apresentou problemas do leitor de documentos e Caio alinhou proximos passos.".to_string(),
            topics: vec!["Sistema leitor".to_string(), "Processamento de PDF".to_string()],
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

        let html =
            render_minutes_from_facts_locally(&diarized, &insights, &participant_names, None);

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

        let payload =
            build_minutes_fact_payload(&diarized, &insights, None, Some(&metadata)).unwrap();
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
            summary:
                "Uma nova colaboradora, Rafael, sera treinada. Caio citou a Rafaela como contato."
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

        let insights =
            parse_chunk_insights_or_fallback(truncated, 0, 0.0, 364.534438, &segments_json);

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
}
