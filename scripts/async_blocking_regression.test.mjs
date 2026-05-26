import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const forbiddenPatterns = [
  {
    file: "src-tauri/src/commands/audio.rs",
    pattern: /(?<!tokio::)fs::create_dir_all\(&output_dir\)/,
    reason: "chunk_audio should not block the async command thread while creating output_dir",
  },
  {
    file: "src-tauri/src/commands/audio.rs",
    pattern: /(?<!tokio::)fs::create_dir_all\(&chunk_output_dir\)/,
    reason: "prepare_audio_and_chunks should not block the async command thread while creating chunk_output_dir",
  },
  {
    file: "src-tauri/src/commands/diarize.rs",
    pattern: "std::fs::create_dir_all(&output_dir)",
    reason: "diarization temp dirs should be created with tokio::fs before spawn_blocking",
  },
  {
    file: "src-tauri/src/commands/diarize.rs",
    pattern: "std::fs::write(&chunks_path",
    reason: "diarization chunks json should be written with tokio::fs before spawn_blocking",
  },
  {
    file: "src-tauri/src/commands/transcribe.rs",
    pattern: "std::fs::create_dir_all(&output_dir)",
    reason: "transcription temp dirs should be created with tokio::fs before spawn_blocking",
  },
  {
    file: "src-tauri/src/commands/transcribe.rs",
    pattern: "std::fs::write(&chunks_path",
    reason: "transcription chunks json should be written with tokio::fs before spawn_blocking",
  },
];

for (const check of forbiddenPatterns) {
  const source = readFileSync(check.file, "utf8");
  assert.equal(
    typeof check.pattern === "string" ? source.includes(check.pattern) : check.pattern.test(source),
    false,
    `${check.file} still contains ${check.pattern}: ${check.reason}`,
  );
}

for (const file of [
  "src-tauri/src/commands/diarize.rs",
  "src-tauri/src/commands/transcribe.rs",
]) {
  const source = readFileSync(file, "utf8");
  assert.match(source, /cleanup_temp_workspace/);
}
