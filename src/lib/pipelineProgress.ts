export type PipelinePhase =
  | 'prepare_audio'
  | 'detect_speech'
  | 'create_chunks'
  | 'transcribe'
  | 'diarize'
  | 'extract_facts'
  | 'wait_speakers'
  | 'generate'
  | 'complete';

export interface PipelineProgressInput {
  phase: PipelinePhase;
  completedAudioSec: number;
  totalAudioSec: number;
  completedChunks: number;
  totalChunks: number;
  elapsedMs: number;
}

export interface PipelineProgressView {
  percent: number;
  title: string;
  detail: string;
  etaLabel: string;
  speedLabel: string;
}

const PHASE_TITLES: Record<PipelinePhase, string> = {
  prepare_audio: 'Preparando audio',
  detect_speech: 'Detectando fala e pausas',
  create_chunks: 'Criando blocos inteligentes',
  transcribe: 'Transcrevendo em paralelo',
  diarize: 'Identificando falantes em paralelo',
  extract_facts: 'Extraindo decisoes e acoes',
  wait_speakers: 'Aguardando falantes',
  generate: 'Montando ata final',
  complete: 'Processamento concluido',
};

const PHASE_PERCENT: Record<Exclude<PipelinePhase, 'transcribe'>, number> = {
  prepare_audio: 5,
  detect_speech: 10,
  create_chunks: 16,
  diarize: 88,
  extract_facts: 92,
  wait_speakers: 96,
  generate: 97,
  complete: 100,
};

const TRANSCRIBE_START_PERCENT = 18;
const TRANSCRIBE_END_PERCENT = 82;
const EXTRACT_FACTS_START_PERCENT = 89;
const EXTRACT_FACTS_END_PERCENT = 95;
const MIN_SPEED_ELAPSED_MS = 1000;

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function finiteNumber(value: number, fallback = 0) {
  return Number.isFinite(value) ? value : fallback;
}

function finiteNonNegative(value: number) {
  return Math.max(finiteNumber(value), 0);
}

function finiteNonNegativeInteger(value: number) {
  return Math.floor(finiteNonNegative(value));
}

function formatDuration(totalSec: number) {
  const safe = Math.max(0, Math.round(finiteNumber(totalSec)));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const seconds = safe % 60;

  if (hours > 0) {
    return `${hours}h${String(minutes).padStart(2, '0')}m`;
  }

  return `${minutes}m${String(seconds).padStart(2, '0')}s`;
}

function phasePercent(phase: PipelinePhase, ratio: number) {
  if (phase === 'complete') return 100;
  if (phase === 'transcribe') {
    const span = TRANSCRIBE_END_PERCENT - TRANSCRIBE_START_PERCENT;
    return clamp(Math.round(TRANSCRIBE_START_PERCENT + ratio * span), TRANSCRIBE_START_PERCENT, TRANSCRIBE_END_PERCENT);
  }
  if (phase === 'extract_facts') {
    const span = EXTRACT_FACTS_END_PERCENT - EXTRACT_FACTS_START_PERCENT;
    return clamp(Math.round(EXTRACT_FACTS_START_PERCENT + ratio * span), EXTRACT_FACTS_START_PERCENT, EXTRACT_FACTS_END_PERCENT);
  }

  return PHASE_PERCENT[phase];
}

export function derivePipelineProgress(input: PipelineProgressInput): PipelineProgressView {
  const totalAudioSec = finiteNonNegative(input.totalAudioSec);
  const completedAudioSec = clamp(input.completedAudioSec, 0, totalAudioSec);
  const safeCompletedAudioSec = finiteNonNegative(completedAudioSec);
  const ratio = totalAudioSec > 0 ? completedAudioSec / totalAudioSec : 0;
  const safeRatio = Number.isFinite(ratio) ? clamp(ratio, 0, 1) : 0;
  const totalChunks = finiteNonNegativeInteger(input.totalChunks);
  const completedChunks = clamp(finiteNonNegativeInteger(input.completedChunks), 0, totalChunks);
  const chunkRatio = totalChunks > 0 ? completedChunks / totalChunks : 0;
  const safeChunkRatio = Number.isFinite(chunkRatio) ? clamp(chunkRatio, 0, 1) : 0;
  const percent = phasePercent(input.phase, input.phase === 'extract_facts' ? safeChunkRatio : safeRatio);
  const elapsedMs = finiteNonNegative(input.elapsedMs);
  const canEstimateSpeed = elapsedMs >= MIN_SPEED_ELAPSED_MS && safeCompletedAudioSec > 0;
  const speed = canEstimateSpeed ? safeCompletedAudioSec / (elapsedMs / 1000) : 0;
  const remainingAudioSec = Math.max(totalAudioSec - safeCompletedAudioSec, 0);
  const canEstimateEta = canEstimateSpeed && speed > 0;
  const remainingWallSec = canEstimateEta ? remainingAudioSec / speed : 0;
  const processedLabel = `${formatDuration(safeCompletedAudioSec)} de ${formatDuration(totalAudioSec)}`;

  if (input.phase === 'prepare_audio') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Validando chaves e preparando o arquivo para processamento.',
      etaLabel: '',
      speedLabel: '',
    };
  }

  if (input.phase === 'detect_speech') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Analisando pausas para encontrar bons pontos de corte no audio.',
      etaLabel: '',
      speedLabel: '',
    };
  }

  if (input.phase === 'create_chunks') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Criando blocos menores para acelerar a transcricao e permitir retomada.',
      etaLabel: '',
      speedLabel: '',
    };
  }

  if (input.phase === 'diarize') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: `Motor local trabalhando em paralelo. ${completedChunks} de ${totalChunks} blocos ja tem transcricao disponivel.`,
      etaLabel: '',
      speedLabel: '',
    };
  }

  if (input.phase === 'generate') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Fatos extraidos. Deduplicando decisoes e compondo o documento final.',
      etaLabel: '',
      speedLabel: '',
    };
  }

  if (input.phase === 'extract_facts') {
    if (totalChunks > 0 && completedChunks >= totalChunks) {
      return {
        percent,
        title: 'Aguardando falantes',
        detail: `${completedChunks} de ${totalChunks} blocos de insights prontos. Falantes ainda em processamento antes da ata.`,
        etaLabel: 'Insights completos',
        speedLabel: `Tempo total ${formatDuration(elapsedMs / 1000)}`,
      };
    }

    const insightPercent = totalChunks > 0 ? Math.round((completedChunks / totalChunks) * 100) : 0;
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: `Lendo ${completedChunks} de ${totalChunks} blocos para separar decisoes, tarefas, riscos e perguntas.`,
      etaLabel: `${completedChunks} de ${totalChunks} blocos de insights`,
      speedLabel: `${insightPercent}% dos insights`,
    };
  }

  if (input.phase === 'wait_speakers') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Insights prontos. Mantendo a ata em espera ate a identificacao de falantes terminar.',
      etaLabel: 'Falantes em andamento',
      speedLabel: `Tempo total ${formatDuration(elapsedMs / 1000)}`,
    };
  }

  if (input.phase === 'complete') {
    return {
      percent,
      title: PHASE_TITLES[input.phase],
      detail: 'Ata gerada e salva. Abrindo a visualizacao final.',
      etaLabel: 'Concluido',
      speedLabel: '',
    };
  }

  return {
    percent,
    title: PHASE_TITLES[input.phase],
    detail: `${completedChunks} de ${totalChunks} blocos transcritos. ${processedLabel} de audio processados.`,
    etaLabel: canEstimateEta ? `${formatDuration(remainingWallSec)} restantes` : 'Calculando tempo restante',
    speedLabel: canEstimateSpeed ? `${speed.toFixed(1)}x tempo real` : 'Calculando velocidade',
  };
}
