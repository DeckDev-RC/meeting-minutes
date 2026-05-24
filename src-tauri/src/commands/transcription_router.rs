#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionProviderKind {
    Cloudflare,
    Deepgram,
    Groq,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRoutePlan {
    pub primary: TranscriptionProviderKind,
    pub fallbacks: Vec<TranscriptionProviderKind>,
}

pub fn smart_low_cost_route(
    cloudflare_available: bool,
    deepgram_available: bool,
) -> TranscriptionRoutePlan {
    let primary = if cloudflare_available {
        TranscriptionProviderKind::Cloudflare
    } else if deepgram_available {
        TranscriptionProviderKind::Deepgram
    } else {
        TranscriptionProviderKind::Local
    };
    let mut fallbacks = Vec::new();
    if primary != TranscriptionProviderKind::Deepgram && deepgram_available {
        fallbacks.push(TranscriptionProviderKind::Deepgram);
    }
    if primary != TranscriptionProviderKind::Local {
        fallbacks.push(TranscriptionProviderKind::Local);
    }

    TranscriptionRoutePlan { primary, fallbacks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_low_cost_prefers_cloudflare_then_deepgram_then_local() {
        let plan = smart_low_cost_route(true, true);

        assert_eq!(plan.primary, TranscriptionProviderKind::Cloudflare);
        assert_eq!(
            plan.fallbacks,
            vec![
                TranscriptionProviderKind::Deepgram,
                TranscriptionProviderKind::Local
            ]
        );
    }

    #[test]
    fn smart_low_cost_uses_local_when_cloud_providers_are_unavailable() {
        let plan = smart_low_cost_route(false, false);

        assert_eq!(plan.primary, TranscriptionProviderKind::Local);
        assert!(plan.fallbacks.is_empty());
    }
}
