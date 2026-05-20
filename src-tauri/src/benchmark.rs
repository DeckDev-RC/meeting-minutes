use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSpeed {
    pub realtime_factor: f64,
    pub speed_x: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkStage {
    pub name: String,
    pub duration_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReport {
    pub id: String,
    pub source: String,
    pub audio_path: String,
    pub audio_duration_sec: f64,
    pub wall_clock_sec: f64,
    pub realtime_factor: f64,
    pub speed_x: f64,
    pub chunk_count: usize,
    pub transcript_segment_count: usize,
    pub diarized_segment_count: usize,
    pub speaker_count: usize,
    pub fact_chunk_count: usize,
    pub decision_count: usize,
    pub action_count: usize,
    pub diarization_mode: String,
    pub diarization_threads: i32,
    pub stages: Vec<BenchmarkStage>,
    pub output_files: Vec<String>,
    pub notes: Vec<String>,
}

pub fn compute_benchmark_speed(
    audio_duration_sec: f64,
    wall_clock_sec: f64,
) -> Result<BenchmarkSpeed, String> {
    if !audio_duration_sec.is_finite() || audio_duration_sec <= 0.0 {
        return Err("audio_duration_sec must be positive".to_string());
    }
    if !wall_clock_sec.is_finite() || wall_clock_sec <= 0.0 {
        return Err("wall_clock_sec must be positive".to_string());
    }

    Ok(BenchmarkSpeed {
        realtime_factor: wall_clock_sec / audio_duration_sec,
        speed_x: audio_duration_sec / wall_clock_sec,
    })
}

pub fn render_benchmark_markdown(report: &BenchmarkReport) -> String {
    let mut output = String::new();

    output.push_str(&format!("# Benchmark E2E: {}\n\n", report.id));
    output.push_str(&format!("- Fonte: {}\n", report.source));
    output.push_str(&format!("- Audio: `{}`\n", report.audio_path));
    output.push_str(&format!(
        "- Duracao do audio: {:.2}s\n",
        report.audio_duration_sec
    ));
    output.push_str(&format!(
        "- Tempo total medido: {:.2}s\n",
        report.wall_clock_sec
    ));
    output.push_str(&format!("- RTF: {:.3}\n", report.realtime_factor));
    output.push_str(&format!("- Velocidade: {:.2}x\n\n", report.speed_x));
    output.push_str(&format!(
        "- Modo de diarizacao: `{}`\n",
        report.diarization_mode
    ));
    output.push_str(&format!(
        "- Threads de diarizacao: `{}`\n\n",
        report.diarization_threads
    ));

    output.push_str("## Volume processado\n\n");
    output.push_str(&format!("- Chunks: {}\n", report.chunk_count));
    output.push_str(&format!(
        "- Segmentos transcritos: {}\n",
        report.transcript_segment_count
    ));
    output.push_str(&format!(
        "- Segmentos diarizados: {}\n",
        report.diarized_segment_count
    ));
    output.push_str(&format!(
        "- Falantes detectados: {}\n",
        report.speaker_count
    ));
    output.push_str(&format!(
        "- Trechos com fatos: {}\n",
        report.fact_chunk_count
    ));
    output.push_str(&format!("- Decisoes: {}\n", report.decision_count));
    output.push_str(&format!("- Acoes: {}\n\n", report.action_count));

    output.push_str("## Tempos por etapa\n\n");
    output.push_str("| Etapa | Tempo |\n");
    output.push_str("| --- | ---: |\n");
    for stage in &report.stages {
        output.push_str(&format!(
            "| {} | {:.2}s |\n",
            stage.name, stage.duration_sec
        ));
    }

    if !report.output_files.is_empty() {
        output.push_str("\n## Arquivos\n\n");
        for file in &report.output_files {
            output.push_str(&format!("- `{}`\n", file));
        }
    }

    if !report.notes.is_empty() {
        output.push_str("\n## Observacoes\n\n");
        for note in &report.notes {
            output.push_str(&format!("- {}\n", note));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_realtime_factor_and_speed_from_wall_clock() {
        let speed = compute_benchmark_speed(1272.64, 318.16).expect("speed should compute");

        assert!((speed.realtime_factor - 0.25).abs() < 0.0001);
        assert!((speed.speed_x - 4.0).abs() < 0.0001);
    }

    #[test]
    fn renders_markdown_with_key_e2e_metrics() {
        let report = BenchmarkReport {
            id: "ami-es2002a".to_string(),
            source: "AMI ES2002a".to_string(),
            audio_path: "benchmarks/data/ami/ES2002a.Mix-Headset.wav".to_string(),
            audio_duration_sec: 1272.64,
            wall_clock_sec: 318.16,
            realtime_factor: 0.25,
            speed_x: 4.0,
            chunk_count: 4,
            transcript_segment_count: 120,
            diarized_segment_count: 32,
            speaker_count: 4,
            fact_chunk_count: 4,
            decision_count: 3,
            action_count: 7,
            diarization_mode: "hybrid".to_string(),
            diarization_threads: 8,
            stages: vec![BenchmarkStage {
                name: "transcribe".to_string(),
                duration_sec: 250.0,
            }],
            output_files: vec!["minutes.html".to_string()],
            notes: vec!["Sem gabarito manual: qualidade factual fica n/a.".to_string()],
        };

        let markdown = render_benchmark_markdown(&report);

        assert!(markdown.contains("# Benchmark E2E: ami-es2002a"));
        assert!(markdown.contains("RTF: 0.250"));
        assert!(markdown.contains("Velocidade: 4.00x"));
        assert!(markdown.contains("Modo de diarizacao: `hybrid`"));
        assert!(markdown.contains("Chunks: 4"));
        assert!(markdown.contains("| transcribe | 250.00s |"));
    }
}
