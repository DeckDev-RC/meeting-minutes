import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import type {
  DiarizedResult,
  DiarizationOptions,
  EvidencePurgeSummary,
  ExportedChunk,
  LocalTranscriptionChunkResult,
  Meeting,
  MeetingMetadata,
  MinutesData,
  MeetingChunkInsights,
  ProcessingJob,
  ProcessingChunkRecord,
  SilenceRange,
  SmartChunkOptions,
  SpeakerTurn,
  StructuredActionPatch,
  StructuredDecisionPatch,
  StructuredEvidence,
  StructuredMinutesData,
  TranscriptionData,
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

export const transcribeChunkCloudflare = (
  audioPath: string,
  cloudflareAccountId: string,
  cloudflareApiToken: string,
  offsetSec: number
) =>
  invoke<TranscriptionSegment[]>('transcribe_chunk_cloudflare', {
    audioPath,
    cloudflareAccountId,
    cloudflareApiToken,
    offsetSec,
  });

export const transcribeChunkDeepgram = (
  audioPath: string,
  deepgramApiKey: string,
  offsetSec: number
) =>
  invoke<TranscriptionSegment[]>('transcribe_chunk_deepgram', {
    audioPath,
    deepgramApiKey,
    offsetSec,
  });

export const transcribeChunkLocal = (audioPath: string, offsetSec: number, model = 'turbo') =>
  invoke<TranscriptionSegment[]>('transcribe_chunk_local', { audioPath, offsetSec, model });

export const transcribeChunksLocal = (audioChunks: ExportedChunk[], model = 'turbo') =>
  invoke<LocalTranscriptionChunkResult[]>('transcribe_chunks_local', { audioChunks, model });

export const transcribeChunksParakeetLocal = (
  audioChunks: ExportedChunk[],
  model = 'nvidia/parakeet-tdt-0.6b-v3',
) =>
  invoke<LocalTranscriptionChunkResult[]>('transcribe_chunks_parakeet_local', {
    audioChunks,
    model,
  });

export const checkLocalTranscriptionBackends = () =>
  invoke<{
    fasterWhisperAvailable: boolean;
    parakeetAvailable: boolean;
  }>('check_local_transcription_backends');

export interface OfflineTranscriptionRuntimeStatus {
  installed: boolean;
  fasterWhisperAvailable: boolean;
  parakeetAvailable: boolean;
  rootPath: string;
  source: string;
  version?: string | null;
  sizeBytes: number;
}

export const getOfflineTranscriptionRuntimeStatus = () =>
  invoke<OfflineTranscriptionRuntimeStatus>('get_offline_transcription_runtime_status');

export const installOfflineTranscriptionRuntime = (
  source: string,
  expectedSha256?: string | null
) =>
  invoke<OfflineTranscriptionRuntimeStatus>('install_offline_transcription_runtime', {
    source,
    expectedSha256,
  });

export const removeOfflineTranscriptionRuntime = () =>
  invoke<OfflineTranscriptionRuntimeStatus>('remove_offline_transcription_runtime');

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

export const diarizeAudioTurnsSherpaChunked = (
  audioChunks: ExportedChunk[],
  expectedSpeakers?: number,
  numThreads?: number,
  provider?: 'cpu' | 'cuda' | 'coreml'
) =>
  invoke<SpeakerTurn[]>('diarize_audio_turns_sherpa_chunked', {
    audioChunks,
    expectedSpeakers,
    numThreads,
    provider,
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
  meetingMetadata?: MeetingMetadata,
  preferLocal = false
) =>
  invoke<string>('generate_ata_from_facts', {
    diarizedJson,
    factsJson,
    geminiApiKey,
    participantNames,
    meetingMetadata,
    preferLocal,
  });

export const generateAtaFromFactsStreaming = (
  meetingId: string,
  diarizedJson: string,
  factsJson: string,
  geminiApiKey: string,
  participantNames: string[] = [],
  meetingMetadata?: MeetingMetadata,
  preferLocal = false
) =>
  invoke<string>('generate_ata_from_facts_streaming', {
    meetingId,
    diarizedJson,
    factsJson,
    geminiApiKey,
    participantNames,
    meetingMetadata,
    preferLocal,
  });

export const saveMeeting = (meeting: Partial<Meeting>) =>
  invoke<string>('save_meeting', { meeting });

export const getMeetings = (limit = 200, offset = 0) =>
  invoke<Meeting[]>('get_meetings', { limit, offset });

export const updateMeetingStatus = (id: string, status: string) =>
  invoke<void>('update_meeting_status', { id, status });

export const upsertProcessingJob = (
  meetingId: string,
  stage: string,
  status: ProcessingJob['status'],
  progressPct: number,
  errorMsg?: string | null
) =>
  invoke<void>('upsert_processing_job', {
    meetingId,
    stage,
    status,
    progressPct,
    errorMsg,
  });

export const getProcessingJobs = (meetingId: string) =>
  invoke<ProcessingJob[]>('get_processing_jobs', { meetingId });

export const saveTranscription = (meetingId: string, rawWhisper: string, diarized: string, speakers: string) =>
  invoke<void>('save_transcription', { meetingId, rawWhisper, diarized, speakers });

export const getTranscriptionByMeeting = (meetingId: string) =>
  invoke<TranscriptionData | null>('get_transcription_by_meeting', { meetingId });

export const saveSpeakerMap = (meetingId: string, speakerMap: Record<string, string>) =>
  invoke<void>('save_speaker_map', { meetingId, speakerMap: JSON.stringify(speakerMap) });

export const saveMinutes = (
  meetingId: string,
  htmlContent: string,
  pdfPath?: string,
  factsJson?: string,
  diarizedJson?: string,
  participantNames?: string[],
  modelUsed = 'meeting-minutes-local-v1',
  purgeSummary?: EvidencePurgeSummary
) =>
  invoke<void>('save_minutes', {
    meetingId,
    htmlContent,
    pdfPath,
    modelUsed,
    factsJson,
    diarizedJson,
    participantNames,
    purgeSummary,
  });

export const getMinutesByMeeting = (meetingId: string) =>
  invoke<MinutesData | null>('get_minutes_by_meeting', { meetingId });

export const getStructuredMinutesByMeeting = (meetingId: string) =>
  invoke<StructuredMinutesData | null>('get_structured_minutes_by_meeting', { meetingId });

export const getMinuteEvidences = (meetingId: string) =>
  invoke<StructuredEvidence[]>('get_minute_evidences', { meetingId });

export const updateMinuteAction = (
  actionId: string,
  patch: StructuredActionPatch,
  reason?: string
) =>
  invoke<void>('update_minute_action', { actionId, patch, reason });

export const updateMinuteDecision = (
  decisionId: string,
  patch: StructuredDecisionPatch,
  reason?: string
) =>
  invoke<void>('update_minute_decision', { decisionId, patch, reason });

export const saveMinuteRevision = (
  meetingId: string,
  reason?: string,
  structuredPayload?: StructuredMinutesData
) =>
  invoke<string>('save_minute_revision', {
    meetingId,
    reason,
    structuredPayload,
  });

export const updateMinuteParticipants = (
  meetingId: string,
  participantNames: string[],
  reason?: string
) =>
  invoke<void>('update_minute_participants', { meetingId, participantNames, reason });

export const restoreMinuteVersion = (versionId: string) =>
  invoke<void>('restore_minute_version', { versionId });

export const savePdf = (pdfBytes: number[], suggestedName: string) =>
  invoke<string>('save_pdf', { pdfBytes, suggestedName });

export const saveHtml = (htmlContent: string, suggestedName: string) =>
  invoke<string>('save_html', { htmlContent, suggestedName });

export const exportDiagnostics = (meetingId?: string | null) =>
  invoke<string>('export_diagnostics', { meetingId });

export const saveBenchmarkRun = (path: string, content: string) =>
  invoke<string>('save_benchmark_run', { path, content });

export const resolveProcessingWorkDir = (meetingId: string) =>
  invoke<string>('resolve_processing_work_dir', { meetingId });

export const openFolder = (path: string) =>
  invoke<void>('open_folder', { path });

export const reapStaleProcessingJobs = (staleAfterMinutes = 90) =>
  invoke<number>('reap_stale_processing_jobs', { staleAfterMinutes });

export interface ApiValidationResult {
  provider: 'groq' | 'gemini' | 'cloudflare' | 'deepgram';
  status: 'valid' | 'missing' | 'invalid' | 'error';
  message: string;
}

export const validateApiKeys = (input: {
  groq: string;
  gemini: string;
  cloudflareAccountId: string;
  cloudflareApiToken: string;
  deepgramApiKey: string;
}) =>
  invoke<ApiValidationResult[]>('validate_api_keys', { input });

export const getApiKeys = () =>
  invoke<{
    groq: string;
    gemini: string;
    cloudflareAccountId: string;
    cloudflareApiToken: string;
    deepgramApiKey: string;
    transcriptionProfile?: import('./types').TranscriptionRoutingProfile;
    manualTranscriptionProvider?: import('./transcriptionProvider').TranscriptionBackend;
    speakerDiarizationRuntime?: import('./types').SpeakerDiarizationRuntime;
    expectedSpeakers?: number;
  }>('get_api_keys');

export const setApiKeys = (
  groq: string,
  gemini: string,
  cloudflareAccountId: string,
  cloudflareApiToken: string,
  deepgramApiKey: string,
  transcriptionProfile?: import('./types').TranscriptionRoutingProfile,
  manualTranscriptionProvider?: import('./transcriptionProvider').TranscriptionBackend,
  speakerDiarizationRuntime?: import('./types').SpeakerDiarizationRuntime,
  expectedSpeakers?: number
) =>
  invoke<void>('set_api_keys', {
    groq,
    gemini,
    cloudflareAccountId,
    cloudflareApiToken,
    deepgramApiKey,
    transcriptionProfile,
    manualTranscriptionProvider,
    speakerDiarizationRuntime,
    expectedSpeakers,
  });
