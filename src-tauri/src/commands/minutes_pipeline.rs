#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinutesPipelineStage {
    ExtractFacts,
    WaitSpeakers,
    GenerateMinutes,
    ValidateEvidence,
    PersistStructuredMinutes,
}

pub fn minutes_pipeline_stages() -> &'static [MinutesPipelineStage] {
    &[
        MinutesPipelineStage::ExtractFacts,
        MinutesPipelineStage::WaitSpeakers,
        MinutesPipelineStage::GenerateMinutes,
        MinutesPipelineStage::ValidateEvidence,
        MinutesPipelineStage::PersistStructuredMinutes,
    ]
}

pub fn stage_label(stage: MinutesPipelineStage) -> &'static str {
    match stage {
        MinutesPipelineStage::ExtractFacts => "extract_facts",
        MinutesPipelineStage::WaitSpeakers => "wait_speakers",
        MinutesPipelineStage::GenerateMinutes => "generate_minutes",
        MinutesPipelineStage::ValidateEvidence => "validate_evidence",
        MinutesPipelineStage::PersistStructuredMinutes => "persist_structured_minutes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_pipeline_keeps_validation_before_persistence() {
        let labels = minutes_pipeline_stages()
            .iter()
            .map(|stage| stage_label(*stage))
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "extract_facts",
                "wait_speakers",
                "generate_minutes",
                "validate_evidence",
                "persist_structured_minutes",
            ]
        );
    }
}
