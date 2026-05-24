use crate::models::audio::SmartChunkOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChunkingProfile {
    Turbo,
    Balanced,
    Precision,
}

pub fn options_for_profile(profile: AudioChunkingProfile) -> SmartChunkOptions {
    match profile {
        AudioChunkingProfile::Turbo => SmartChunkOptions {
            target_sec: 480.0,
            min_sec: 240.0,
            max_sec: 600.0,
            overlap_sec: 0.0,
            ..SmartChunkOptions::default()
        },
        AudioChunkingProfile::Balanced => SmartChunkOptions::default(),
        AudioChunkingProfile::Precision => SmartChunkOptions {
            target_sec: 240.0,
            min_sec: 120.0,
            max_sec: 360.0,
            overlap_sec: 3.0,
            ..SmartChunkOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turbo_chunking_uses_longer_chunks_without_overlap() {
        let options = options_for_profile(AudioChunkingProfile::Turbo);

        assert_eq!(options.overlap_sec, 0.0);
        assert!(options.target_sec > SmartChunkOptions::default().target_sec);
    }

    #[test]
    fn precision_chunking_uses_shorter_chunks() {
        let options = options_for_profile(AudioChunkingProfile::Precision);

        assert!(options.target_sec < SmartChunkOptions::default().target_sec);
        assert_eq!(options.overlap_sec, 3.0);
    }
}
