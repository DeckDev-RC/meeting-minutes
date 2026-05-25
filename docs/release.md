# Release and installer artifacts

Installer artifacts are intentionally not tracked in normal Git history. The
Windows MSI and EXE files are large, change on every build, and should be
published through GitHub Releases or Git LFS.

## Recommended flow: GitHub Releases

Release artifacts are published by `.github/workflows/release.yml`.

Create and push a version tag:

```powershell
git tag v1.0.3
git push origin v1.0.3
```

The workflow builds the Windows MSI/NSIS installers on `windows-2025`, prepares
the FFmpeg sidecar, stages the optimized diarization runtime, creates a GitHub
Release, and uploads the generated installers as release assets.

You can also run the workflow manually from GitHub Actions and provide a
`release_tag`.

To publish the downloadable offline transcription runtime, run the workflow
manually with `build_offline_runtime=true`. It uploads
`meeting-minutes-transcribe-runtime-windows-x64.zip` and the versioned ZIP to
the same GitHub Release. The app can install this package from
`Configuracoes > Modo offline`.

## Local build and handoff

1. Prepare the local diarization runtime when it is not available yet:

   ```powershell
   npm run setup:diarize-cpu
   ```

2. Build the app locally:

   ```powershell
   npm run tauri:build
   ```

   The Tauri build runs `npm run runtime:diarize:stage` automatically and embeds
   the runtime into the installer resources.

3. Copy the generated artifacts into a local `executaveis/` folder if you want
   an easy handoff path. This folder is ignored by Git.

   With the local target dir used by this repo, the generated files are usually:

   ```powershell
   C:\tmp\cargo-target\release\bundle\msi\Meeting Minutes AI_1.0.3_x64_en-US.msi
   C:\tmp\cargo-target\release\bundle\nsis\Meeting Minutes AI_1.0.3_x64-setup.exe
   ```

## Bundled diarization runtime

The normal Windows installer is a complete bundle for non-technical users. It
includes:

- the app;
- FFmpeg sidecar;
- the optimized `.venv-diarize` Python runtime;
- `scripts/diarize_cpu_backend.py`.

The runtime is staged into `src-tauri/resources/diarize/` during build. That
folder is generated and ignored by Git.

The standalone runtime pack scripts are kept only for diagnostics, repair, or
machines that need the runtime refreshed without reinstalling the whole app:

```powershell
npm run runtime:diarize:pack
npm run runtime:diarize:install -- -PackPath "C:\path\meeting-minutes-diarize-runtime-1.0.3-windows-x64.zip"
```

The app resolves the bundled runtime first through the Tauri resource directory,
and still supports `MEETING_MINUTES_DIARIZE_ROOT` for development and repair.

## Offline transcription runtime

The normal installer does not embed local ASR by default, because the
transcription runtime and model are large. The product supports two delivery
paths:

- in-app download: publish the runtime ZIP with the release workflow and install
  it from `Configuracoes > Modo offline`;
- full offline installer: build locally with `npm run tauri:build:offline`.

The in-app runtime is installed under AppData:

```powershell
%APPDATA%\com.agregar.meeting-minutes\runtime\transcribe
```

Generate the downloadable ZIP locally when needed:

```powershell
npm run setup:transcribe-local
npm run runtime:transcribe:pack
```

Generate a full offline bundle for clients that need one installer with local
transcription included:

```powershell
npm run setup:diarize-cpu
npm run setup:transcribe-local
npm run tauri:build:offline
```

The offline bundle script stages `src-tauri/resources/transcribe/` only for that
build, copies the generated installers to `dist/offline-bundle/` with an
`-offline` suffix, and cleans the temporary resource directory before exiting.

## Alternative: Git LFS

If releases are not practical for internal distribution, track only installer
extensions with Git LFS. The LFS patterns are already declared in
`.gitattributes`; install LFS once and force-add the ignored files when needed:

```powershell
git lfs install
git add -f "executaveis/Meeting Minutes AI_1.0.3_x64_en-US.msi"
git add -f "executaveis/Meeting Minutes AI_1.0.3_x64-setup.exe"
```

Do not store generated binaries directly in regular Git commits.

