export type MeetingStatus = 'pending' | 'processing' | 'done' | 'error';

export type ProcessingProfile = 'turbo' | 'balanced' | 'precision';

export type TranscriptionRoutingProfile =
  | 'smart-low-cost'
  | 'max-quality'
  | 'groq-turbo'
  | 'offline-free'
  | 'manual';

export type JobStep =
  | 'extract_audio'
  | 'transcribe'
  | 'diarize'
  | 'generate'
  | 'pdf';

export interface Meeting {
  id: string;
  title: string | null;
  filePath: string;
  audioPath: string | null;
  participantsHint: string | null;
  processingProfile: ProcessingProfile;
  transcriptionProfile?: TranscriptionRoutingProfile | null;
  status: MeetingStatus;
  createdAt: string;
  updatedAt: string;
}

export interface MeetingMetadata {
  sourcePath?: string | null;
  sourceFileName?: string | null;
  sourceTitle?: string | null;
  embeddedCreatedAt?: string | null;
  fileCreatedAt?: string | null;
  fileModifiedAt?: string | null;
  recordedAt?: string | null;
  recordedAtSource?: string | null;
  durationSec?: number | null;
}

export interface MinutesData {
  id: string;
  meeting_id: string;
  html_content: string;
  pdf_path: string | null;
  model_used: string;
  created_at: string;
}

export interface TranscriptionData {
  id: string;
  meeting_id: string;
  raw_whisper: string | null;
  diarized: string | null;
  speakers: string | null;
  speaker_map: string | null;
  language: string | null;
  created_at: string;
}

export interface TranscriptionSegment {
  id: number;
  start: number;
  end: number;
  text: string;
}

export interface DiarizedSegment {
  speaker: string;
  start: number;
  end: number;
  text: string;
}

export interface DiarizedResult {
  speakers: string[];
  segments: DiarizedSegment[];
  telemetry?: {
    requestedMode: string;
    backendUsed: string;
    fallbackReason?: string;
    wallClockSec: number;
    speakerCount: number;
    segmentCount: number;
  };
}

export interface SpeakerTurn {
  start: number;
  end: number;
  speakerIndex: number;
}

export interface MeetingDecision {
  title: string;
  owner: string;
  timestampSec: number;
  evidence: string;
}

export interface MeetingAction {
  task: string;
  owner: string;
  deadline: string;
  timestampSec: number;
  evidence: string;
}

export interface MeetingChunkInsights {
  chunkIndex: number;
  startSec: number;
  endSec: number;
  summary: string;
  topics: string[];
  decisions: MeetingDecision[];
  actions: MeetingAction[];
  questions: string[];
  risks: string[];
}

export interface SilenceRange {
  startSec: number;
  endSec: number;
}

export interface ExportedChunk {
  index: number;
  audioPath: string;
  startSec: number;
  endSec: number;
  offsetSec: number;
  durationSec: number;
}

export interface LocalTranscriptionChunkResult {
  index: number;
  segments: TranscriptionSegment[];
}

export interface SmartChunkOptions {
  targetSec: number;
  minSec: number;
  maxSec: number;
  overlapSec: number;
  silenceMinDurationSec: number;
  silenceNoiseDb: number;
  outputFormat: 'flac' | 'wav' | 'mp3';
  prepareStrategy?: 'parallel' | 'singlePassSilence';
}

export type DiarizationMode =
  | 'auto'
  | 'fast'
  | 'hybrid'
  | 'modern-cpu'
  | 'modern-cpu-chunked'
  | 'precise'
  | 'pyannote';

export type SpeakerDiarizationRuntime =
  | 'modern-cpu'
  | 'sherpa-onnx-cpu'
  | 'sherpa-onnx-cuda';

export interface DiarizationOptions {
  audioChunks?: ExportedChunk[];
  mode?: DiarizationMode;
  numThreads?: number;
  expectedSpeakers?: number;
}

export interface ProcessingChunkRecord extends ExportedChunk {
  meetingId: string;
  status: 'pending' | 'running' | 'done' | 'error';
  rawSegmentsJson: string | null;
  errorMsg: string | null;
  factsStatus: 'pending' | 'running' | 'done' | 'error';
  factsJson: string | null;
  factsErrorMsg: string | null;
}

export interface PipelineState {
  meetingId: string;
  currentStep: JobStep;
  stepStatus: Record<JobStep, 'pending' | 'running' | 'done' | 'error'>;
  progress: number;
  error: string | null;
}
