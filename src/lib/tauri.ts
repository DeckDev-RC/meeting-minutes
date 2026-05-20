import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type {
  DiarizedResult,
  DiarizationOptions,
  ExportedChunk,
  Meeting,
  MeetingMetadata,
  MeetingChunkInsights,
  ProcessingChunkRecord,
  SilenceRange,
  SmartChunkOptions,
  SpeakerTurn,
  TranscriptionSegment,
} from './types';
import type { FactBatchItem } from './meetingFactsQueue';

const invoke = <T>(command: string, args?: Record<string, unknown>) => {
  const mock = window.__MEETING_MINUTES_E2E__?.invoke;
  if (mock) {
    return mock<T>(command, args);
  }

  return tauriInvoke<T>(command, args);
};

export const extractAudio = (inputPath: string, outputPath: string) =>
  invoke<number>('extract_audio', { inputPath, outputPath });

export const prepareAudioAndChunks = (
  inputPath: string,
  audioOutputPath: string,
  chunkOutputDir: string,
  options?: SmartChunkOptions
) =>
  invoke<{ durationSec: number; chunks: ExportedChunk[] }>('prepare_audio_and_chunks', {
    inputPath,
    audioOutputPath,
    chunkOutputDir,
    options,
  });

export const probeMediaMetadata = (inputPath: string) =>
  invoke<MeetingMetadata>('probe_media_metadata', { inputPath });

export const chunkAudio = (inputPath: string, outputDir: string, chunkDurationSec = 1200) =>
  invoke<string[]>('chunk_audio', { inputPath, outputDir, chunkDurationSec });

export const detectSilences = (inputPath: string, noiseDb: number, minDurationSec: number) =>
  invoke<SilenceRange[]>('detect_silences', { inputPath, noiseDb, minDurationSec });

export const createSmartChunks = (
  inputPath: string,
  outputDir: string,
  durationSec: number,
  options?: SmartChunkOptions
) =>
  invoke<ExportedChunk[]>('create_smart_chunks', { inputPath, outputDir, durationSec, options });

export const saveProcessingChunks = (meetingId: string, chunks: ExportedChunk[]) =>
  invoke<void>('save_processing_chunks', { meetingId, chunks });

export const getProcessingChunks = (meetingId: string) =>
  invoke<ProcessingChunkRecord[]>('get_processing_chunks', { meetingId });

export const updateProcessingChunkResult = (
  meetingId: string,
  index: number,
  status: ProcessingChunkRecord['status'],
  rawSegmentsJson?: string,
  errorMsg?: string
) =>
  invoke<void>('update_processing_chunk_result', {
    meetingId,
    index,
    status,
    rawSegmentsJson,
    errorMsg,
  });

export const updateProcessingChunkFacts = (
  meetingId: string,
  index: number,
  status: ProcessingChunkRecord['factsStatus'],
  factsJson?: string,
  errorMsg?: string
) =>
  invoke<void>('update_processing_chunk_facts', {
    meetingId,
    index,
    status,
    factsJson,
    errorMsg,
  });

export const transcribeChunk = (audioPath: string, groqApiKey: string, offsetSec: number) =>
  invoke<TranscriptionSegment[]>('transcribe_chunk', { audioPath, groqApiKey, offsetSec });

export const diarizeTranscription = (segmentsJson: string, geminiApiKey: string) =>
  invoke<DiarizedResult>('diarize_transcription', { segmentsJson, geminiApiKey });

export const diarizeTranscriptionFast = (segmentsJson: string) =>
  invoke<DiarizedResult>('diarize_transcription_fast', { segmentsJson });

export const diarizeTranscriptionEndToEnd = (
  audioPath: string,
  segmentsJson: string,
  options: DiarizationOptions = {}
) =>
  invoke<DiarizedResult>('diarize_transcription_end_to_end', {
    audioPath,
    segmentsJson,
    audioChunks: options.audioChunks,
    mode: options.mode,
    numThreads: options.numThreads,
    expectedSpeakers: options.expectedSpeakers,
  });

export const diarizeAudioTurnsModernCpu = (
  audioPath: string,
  expectedSpeakers?: number
) =>
  invoke<SpeakerTurn[]>('diarize_audio_turns_modern_cpu', {
    audioPath,
    expectedSpeakers,
  });

export const diarizeAudioTurnsModernCpuChunked = (
  audioChunks: ExportedChunk[],
  expectedSpeakers?: number,
  numThreads?: number
) =>
  invoke<SpeakerTurn[]>('diarize_audio_turns_modern_cpu_chunked', {
    audioChunks,
    expectedSpeakers,
    numThreads,
  });

export const diarizeAudioTurnsPyannote = (
  audioPath: string,
  expectedSpeakers?: number
) =>
  invoke<SpeakerTurn[]>('diarize_audio_turns_pyannote', {
    audioPath,
    expectedSpeakers,
  });

export const alignSpeakerTurnsToTranscription = (
  segmentsJson: string,
  speakerTurnsJson: string
) =>
  invoke<DiarizedResult>('align_speaker_turns_to_transcription', {
    segmentsJson,
    speakerTurnsJson,
  });

export const refineDiarizationSelectively = (
  segmentsJson: string,
  speakerTurnsJson: string,
  audioChunks: ExportedChunk[],
  options: Pick<DiarizationOptions, 'expectedSpeakers' | 'numThreads'> & {
    maxRefinementChunks?: number;
  } = {}
) =>
  invoke<DiarizedResult>('refine_diarization_selectively', {
    segmentsJson,
    speakerTurnsJson,
    audioChunks,
    expectedSpeakers: options.expectedSpeakers,
    numThreads: options.numThreads,
    maxRefinementChunks: options.maxRefinementChunks,
  });

export const generateAtaHtml = (diarizedJson: string, geminiApiKey: string) =>
  invoke<string>('generate_ata_html', { diarizedJson, geminiApiKey });

export const extractChunkFacts = (
  chunkIndex: number,
  startSec: number,
  endSec: number,
  segmentsJson: string,
  geminiApiKey: string,
  participantNames: string[] = []
) =>
  invoke<MeetingChunkInsights>('extract_chunk_facts', {
    chunkIndex,
    startSec,
    endSec,
    segmentsJson,
    geminiApiKey,
    participantNames,
  });

export const extractFactBatch = (
  batch: FactBatchItem[],
  geminiApiKey: string,
  participantNames: string[] = []
) =>
  invoke<MeetingChunkInsights[]>('extract_fact_batch', {
    chunks: batch.map((item) => ({
      chunkIndex: item.chunk.index,
      startSec: item.chunk.startSec,
      endSec: item.chunk.endSec,
      segmentsJson: JSON.stringify(item.segments),
    })),
    geminiApiKey,
    participantNames,
  });

export const generateAtaFromFacts = (
  diarizedJson: string,
  factsJson: string,
  geminiApiKey: string,
  participantNames: string[] = [],
  meetingMetadata?: MeetingMetadata
) =>
  invoke<string>('generate_ata_from_facts', {
    diarizedJson,
    factsJson,
    geminiApiKey,
    participantNames,
    meetingMetadata,
  });

export const generateAtaFromFactsStreaming = (
  meetingId: string,
  diarizedJson: string,
  factsJson: string,
  geminiApiKey: string,
  participantNames: string[] = [],
  meetingMetadata?: MeetingMetadata
) =>
  invoke<string>('generate_ata_from_facts_streaming', {
    meetingId,
    diarizedJson,
    factsJson,
    geminiApiKey,
    participantNames,
    meetingMetadata,
  });

export const saveMeeting = (meeting: Partial<Meeting>) =>
  invoke<string>('save_meeting', { meeting });

export const getMeetings = () =>
  invoke<Meeting[]>('get_meetings');

export const updateMeetingStatus = (id: string, status: string) =>
  invoke<void>('update_meeting_status', { id, status });

export const saveTranscription = (meetingId: string, rawWhisper: string, diarized: string, speakers: string) =>
  invoke<void>('save_transcription', { meetingId, rawWhisper, diarized, speakers });

export const saveMinutes = (meetingId: string, htmlContent: string, pdfPath?: string) =>
  invoke<void>('save_minutes', { meetingId, htmlContent, pdfPath, modelUsed: 'gemini-2.5-flash' });

export const savePdf = (pdfBytes: number[], suggestedName: string) =>
  invoke<string>('save_pdf', { pdfBytes, suggestedName });

export const saveBenchmarkRun = (path: string, content: string) =>
  invoke<string>('save_benchmark_run', { path, content });

export const openFolder = (path: string) =>
  invoke<void>('open_folder', { path });

export const getApiKeys = () =>
  invoke<{ groq: string; gemini: string; expectedSpeakers?: number }>('get_api_keys');

export const setApiKeys = (groq: string, gemini: string, expectedSpeakers?: number) =>
  invoke<void>('set_api_keys', { groq, gemini, expectedSpeakers });
