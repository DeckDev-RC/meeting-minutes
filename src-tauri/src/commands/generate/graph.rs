use super::participants::normalize_participant_names;
use crate::models::transcription::{
    DiarizedResult, MeetingAction, MeetingChunkInsights, MeetingDecision,
};
use aho_corasick::AhoCorasick;
use std::collections::HashSet;
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
    let mut estimated = diarized
        .map(|diarized| {
            diarized
                .segments
                .iter()
                .map(|segment| segment.text.len() + 1)
                .sum::<usize>()
        })
        .unwrap_or(0);
    for insight in insights {
        estimated += insight.summary.len() + 1;
        estimated += insight
            .topics
            .iter()
            .map(|item| item.len() + 1)
            .sum::<usize>();
        estimated += insight
            .decisions
            .iter()
            .map(|item| item.title.len() + item.owner.len() + item.evidence.len() + 3)
            .sum::<usize>();
        estimated += insight
            .actions
            .iter()
            .map(|item| {
                item.task.len() + item.owner.len() + item.deadline.len() + item.evidence.len() + 4
            })
            .sum::<usize>();
        estimated += insight
            .questions
            .iter()
            .map(|item| item.len() + 1)
            .sum::<usize>();
        estimated += insight
            .risks
            .iter()
            .map(|item| item.len() + 1)
            .sum::<usize>();
    }

    let mut text = String::with_capacity(estimated);

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

fn char_eq_ignore_case(left: char, right: char) -> bool {
    left.to_lowercase().eq(right.to_lowercase())
}

pub(super) fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    let needle_chars = needle.chars().collect::<Vec<_>>();
    haystack.char_indices().any(|(start, _)| {
        haystack[start..]
            .chars()
            .zip(needle_chars.iter().copied())
            .take_while(|(left, right)| char_eq_ignore_case(*left, *right))
            .count()
            == needle_chars.len()
    })
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
pub(super) struct PreparedNameAlias {
    alias: String,
    canonical: String,
}

pub(super) struct PreparedNameAliases {
    entries: Vec<PreparedNameAlias>,
    matcher: Option<AhoCorasick>,
}

pub(super) fn prepare_name_aliases(aliases: &[(String, String)]) -> PreparedNameAliases {
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

pub(super) fn replace_prepared_name_aliases(value: &str, aliases: &PreparedNameAliases) -> String {
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
    if aliases.matcher.is_none() {
        return insight;
    }

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

pub(super) fn display_owner(owner: &str) -> String {
    let trimmed = owner.trim();
    let lower = trimmed.to_lowercase();
    if trimmed.is_empty() || lower.starts_with("falante ") || lower == "speaker" {
        "A definir".to_string()
    } else {
        trimmed.to_string()
    }
}

fn fold_latin_lower(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        for lower in ch.to_lowercase() {
            let folded = match lower {
                'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                other => other,
            };
            output.push(folded);
        }
    }

    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn owner_name_is_promotable_participant(owner: &str) -> bool {
    let trimmed = owner.trim();
    if display_owner(trimmed) == "A definir" {
        return false;
    }

    if trimmed.chars().any(|ch| {
        matches!(
            ch,
            '[' | ']' | '{' | '}' | '(' | ')' | '<' | '>' | '@' | '#'
        )
    }) {
        return false;
    }

    let folded = fold_latin_lower(trimmed);
    if folded.is_empty() {
        return false;
    }

    const GENERIC_OWNER_VALUES: &[&str] = &[
        "a definir",
        "indefinido",
        "na",
        "n a",
        "n/a",
        "nao informado",
        "nao informada",
        "nao especificado",
        "nao especificada",
        "sem responsavel",
        "todos",
        "todas",
        "todos os participantes",
        "participantes",
        "participante",
        "responsavel",
        "responsaveis",
        "equipe",
        "time",
        "grupo",
        "implicit",
        "implicito",
        "undefined",
        "unspecified",
    ];
    if GENERIC_OWNER_VALUES.contains(&folded.as_str()) {
        return false;
    }

    let tokens = word_tokens_lower_ordered(&folded);
    if tokens.is_empty() || tokens.iter().any(|token| token.chars().count() < 2) {
        return false;
    }

    const GENERIC_OWNER_TOKENS: &[&str] = &[
        "administrativo",
        "aprovacao",
        "aprovador",
        "area",
        "cliente",
        "clientes",
        "comercial",
        "contabil",
        "contabilidade",
        "departamento",
        "diretoria",
        "empresa",
        "equipe",
        "especificado",
        "financeiro",
        "fornecedor",
        "fornecedores",
        "gestao",
        "grupo",
        "implicit",
        "implicito",
        "juridico",
        "lideranca",
        "marketing",
        "narrador",
        "operacao",
        "operacoes",
        "participante",
        "participantes",
        "responsavel",
        "responsaveis",
        "setor",
        "sistema",
        "suporte",
        "time",
        "vendas",
    ];
    !tokens
        .iter()
        .any(|token| GENERIC_OWNER_TOKENS.contains(&token.as_str()))
}

fn owner_participant_candidates(owner: &str) -> Vec<String> {
    owner
        .split(',')
        .flat_map(|part| part.split(';'))
        .flat_map(|part| part.split('/'))
        .flat_map(|part| part.split('\\'))
        .flat_map(|part| part.split(" e "))
        .flat_map(|part| part.split(" E "))
        .flat_map(|part| part.split(" & "))
        .map(str::trim)
        .filter(|name| owner_name_is_promotable_participant(name))
        .map(str::to_string)
        .collect()
}

fn owner_participant_names(
    decisions: &[MeetingDecision],
    actions: &[MeetingAction],
) -> Vec<String> {
    decisions
        .iter()
        .map(|decision| decision.owner.as_str())
        .chain(actions.iter().map(|action| action.owner.as_str()))
        .flat_map(owner_participant_candidates)
        .collect()
}

#[derive(Debug, Clone)]
pub(super) struct MinutesFactGraph {
    pub(super) participant_names: Vec<String>,
    pub(super) sorted_insights: Vec<MeetingChunkInsights>,
    pub(super) topics: Vec<String>,
    pub(super) decisions: Vec<MeetingDecision>,
    pub(super) actions: Vec<MeetingAction>,
    pub(super) questions: Vec<String>,
    pub(super) risks: Vec<String>,
    pub(super) summaries: Vec<String>,
    pub(super) participants: Vec<String>,
    pub(super) duration_sec: f64,
}

pub(super) fn build_minutes_fact_graph(
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
    let owner_names = owner_participant_names(&decisions, &actions);

    let participants = if participant_names.is_empty() {
        unique_strings(diarized.speakers.clone().into_iter().chain(owner_names))
    } else {
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
