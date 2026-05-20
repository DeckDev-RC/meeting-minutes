export type ProcessingStage =
  | "preparing"
  | "extract_audio"
  | "chunk_audio"
  | "transcribe"
  | "diarize"
  | "save_transcription"
  | "generate"
  | "save_minutes"
  | "complete";

export interface ProcessingProgress {
  percent: number;
  title: string;
  detail: string;
}

interface TranscriptionProgress {
  completed?: number;
  total?: number;
}

const STAGE_PROGRESS: Record<Exclude<ProcessingStage, "transcribe">, ProcessingProgress> = {
  preparing: {
    percent: 3,
    title: "Preparando processamento",
    detail: "Validando chaves de API e carregando os dados da reuniao.",
  },
  extract_audio: {
    percent: 10,
    title: "Extraindo audio",
    detail: "Convertendo o arquivo original para MP3 mono em 16 kHz.",
  },
  chunk_audio: {
    percent: 20,
    title: "Dividindo audio",
    detail: "Separando o audio em trechos menores para transcricao.",
  },
  diarize: {
    percent: 75,
    title: "Identificando falantes",
    detail: "Organizando a transcricao por participantes e sequencia da conversa.",
  },
  save_transcription: {
    percent: 84,
    title: "Salvando transcricao",
    detail: "Gravando o texto bruto e a versao com falantes no historico.",
  },
  generate: {
    percent: 90,
    title: "Gerando ata",
    detail: "Transformando a transcricao em uma ata estruturada.",
  },
  save_minutes: {
    percent: 98,
    title: "Finalizando",
    detail: "Salvando a ata e atualizando o status da reuniao.",
  },
  complete: {
    percent: 100,
    title: "Processamento concluido",
    detail: "Ata gerada e salva. Abrindo a visualizacao.",
  },
};

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

export function getProcessingProgress(
  stage: ProcessingStage,
  progress: TranscriptionProgress = {},
): ProcessingProgress {
  if (stage !== "transcribe") {
    return STAGE_PROGRESS[stage];
  }

  const total = progress.total ?? 0;
  const completed = progress.completed ?? 0;
  const ratio = total > 0 ? clamp(completed / total, 0, 1) : 0;
  const percent = Math.round(25 + ratio * 45);

  return {
    percent,
    title: "Transcrevendo audio",
    detail:
      total > 0
        ? `Enviando trecho ${clamp(completed, 1, total)} de ${total} para transcricao.`
        : "Enviando os trechos de audio para transcricao.",
  };
}
