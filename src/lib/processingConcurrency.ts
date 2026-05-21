import type { ProcessingProfile } from "./types";

export const transcriptionConcurrencyForProfile = (profile: ProcessingProfile) => {
  if (profile === "turbo") return 6;
  if (profile === "precision") return 3;
  return 4;
};

export const factConcurrencyForProfile = (profile: ProcessingProfile) => {
  if (profile === "turbo") return 4;
  if (profile === "precision") return 3;
  return 3;
};

export const overlappingFactConcurrencyForProfile = (profile: ProcessingProfile) => {
  if (profile === "turbo") return 2;
  return 1;
};

export const factConcurrencyForPhase = (
  profile: ProcessingProfile,
  transcriptionClosed: boolean,
) => transcriptionClosed
  ? factConcurrencyForProfile(profile)
  : overlappingFactConcurrencyForProfile(profile);
