import { create } from 'zustand';
import type { JobStep, Meeting } from '../lib/types';

interface MeetingStore {
  currentMeetingId: string | null;
  currentStep: JobStep | null;
  stepStatus: Record<JobStep, 'pending' | 'running' | 'done' | 'error'>;
  progress: number;
  progressTitle: string;
  progressDetail: string;
  progressEta: string;
  progressSpeed: string;
  processingNote: string;
  error: string | null;
  meetings: Meeting[];
  setCurrentMeeting: (id: string) => void;
  setStep: (step: JobStep) => void;
  setStepStatus: (step: JobStep, status: 'pending' | 'running' | 'done' | 'error') => void;
  setProgress: (p: number) => void;
  setProgressInfo: (progress: number, title: string, detail: string) => void;
  setProgressStats: (progress: number, title: string, detail: string, eta: string, speed: string) => void;
  setProcessingNote: (note: string) => void;
  setError: (e: string | null) => void;
  setMeetings: (meetings: Meeting[]) => void;
  reset: () => void;
}

const initialStepStatus: Record<JobStep, 'pending' | 'running' | 'done' | 'error'> = {
  extract_audio: 'pending',
  transcribe: 'pending',
  diarize: 'pending',
  generate: 'pending',
  pdf: 'pending',
};

export const useMeetingStore = create<MeetingStore>((set) => ({
  currentMeetingId: null,
  currentStep: null,
  stepStatus: { ...initialStepStatus },
  progress: 0,
  progressTitle: 'Aguardando inicio',
  progressDetail: 'Selecione uma reuniao para iniciar o processamento.',
  progressEta: '',
  progressSpeed: '',
  processingNote: '',
  error: null,
  meetings: [],
  setCurrentMeeting: (id) => set({ currentMeetingId: id }),
  setStep: (step) => set({ currentStep: step }),
  setStepStatus: (step, status) =>
    set((state) => ({ stepStatus: { ...state.stepStatus, [step]: status } })),
  setProgress: (progress) => set({ progress }),
  setProgressInfo: (progress, progressTitle, progressDetail) =>
    set({ progress, progressTitle, progressDetail, progressEta: '', progressSpeed: '' }),
  setProgressStats: (progress, progressTitle, progressDetail, progressEta, progressSpeed) =>
    set({ progress, progressTitle, progressDetail, progressEta, progressSpeed }),
  setProcessingNote: (processingNote) => set({ processingNote }),
  setError: (error) => set({ error }),
  setMeetings: (meetings) => set({ meetings }),
  reset: () => set({
    currentMeetingId: null,
    currentStep: null,
    stepStatus: { ...initialStepStatus },
    progress: 0,
    progressTitle: 'Aguardando inicio',
    progressDetail: 'Selecione uma reuniao para iniciar o processamento.',
    progressEta: '',
    progressSpeed: '',
    processingNote: '',
    error: null,
  }),
}));
