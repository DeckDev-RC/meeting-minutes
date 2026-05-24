use crate::models::transcription::TranscriptionSegment;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

const EVIDENCE_THRESHOLD: f64 = 0.58;
const STOPWORDS: &[&str] = &[
    "a", "as", "ate", "com", "da", "das", "de", "do", "dos", "e", "em", "o", "os", "para", "pela",
    "pelo", "por", "que", "um", "uma",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceValidation {
    pub score: f64,
    pub verified: bool,
    pub transcript_excerpt: String,
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
            if folded.is_ascii_alphanumeric() {
                output.push(folded);
            } else {
                output.push(' ');
            }
        }
    }

    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokenize(value: &str) -> Vec<String> {
    let stopwords = STOPWORDS.iter().copied().collect::<HashSet<_>>();
    fold_latin_lower(value)
        .split_whitespace()
        .map(str::trim)
        .filter(|token| token.chars().count() > 1 && !stopwords.contains(*token))
        .map(str::to_string)
        .collect()
}

fn evidence_similarity(source: &str, evidence: &str) -> f64 {
    let normalized_source = fold_latin_lower(source);
    let normalized_evidence = fold_latin_lower(evidence);
    if normalized_source.is_empty() || normalized_evidence.is_empty() {
        return 0.0;
    }
    if normalized_source.contains(&normalized_evidence) {
        return 1.0;
    }

    let source_tokens = tokenize(source);
    let evidence_tokens = tokenize(evidence);
    if source_tokens.is_empty() || evidence_tokens.is_empty() {
        return 0.0;
    }

    let mut source_counts = HashMap::<String, usize>::new();
    for token in &source_tokens {
        *source_counts.entry(token.clone()).or_default() += 1;
    }

    let mut matched = 0usize;
    for token in &evidence_tokens {
        let count = source_counts.get(token).copied().unwrap_or_default();
        if count > 0 {
            matched += 1;
            source_counts.insert(token.clone(), count - 1);
        }
    }

    if matched == evidence_tokens.len() {
        return 1.0;
    }

    let coverage = matched as f64 / evidence_tokens.len() as f64;
    let dice = (2 * matched) as f64 / (source_tokens.len() + evidence_tokens.len()) as f64;
    dice.max(coverage * 0.85)
}

fn parse_segments_text(segments_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<TranscriptionSegment>>(segments_json)
        .map(|segments| {
            segments
                .into_iter()
                .map(|segment| segment.text.trim().to_string())
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn validate_evidence_against_text(evidence: &str, transcript_text: &str) -> EvidenceValidation {
    if evidence.trim().is_empty() || transcript_text.trim().is_empty() {
        return EvidenceValidation {
            score: 0.0,
            verified: false,
            transcript_excerpt: String::new(),
        };
    }

    let score = (evidence_similarity(transcript_text, evidence) * 1000.0).round() / 1000.0;
    EvidenceValidation {
        score,
        verified: score >= EVIDENCE_THRESHOLD,
        transcript_excerpt: transcript_text.chars().take(500).collect(),
    }
}

pub fn validate_evidence_against_segments_json(
    evidence: &str,
    segments_json: Option<&str>,
) -> EvidenceValidation {
    let segments = segments_json.map(parse_segments_text).unwrap_or_default();
    let corpus = segments.join(" ");
    let mut best = validate_evidence_against_text(evidence, &corpus);

    for segment in segments {
        let candidate = validate_evidence_against_text(evidence, &segment);
        if candidate.score > best.score {
            best = candidate;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_exact_evidence() {
        let result = validate_evidence_against_text(
            "Caio vai revisar o contrato",
            "No final, Caio vai revisar o contrato ate sexta.",
        );

        assert!(result.verified);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn validates_normalized_evidence() {
        let result = validate_evidence_against_text(
            "revisar integração",
            "A equipe falou sobre revisar a integracao com o sistema.",
        );

        assert!(result.verified);
        assert!(result.score >= 0.58);
    }

    #[test]
    fn rejects_weak_evidence() {
        let result = validate_evidence_against_text(
            "aprovar budget internacional",
            "Foi discutida a troca de senha do sistema.",
        );

        assert!(!result.verified);
        assert!(result.score < 0.58);
    }

    #[test]
    fn rejects_empty_evidence() {
        let result = validate_evidence_against_text("", "Caio falou sobre contrato.");

        assert!(!result.verified);
        assert_eq!(result.score, 0.0);
    }
}
