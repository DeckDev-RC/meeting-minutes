# Release and installer artifacts

Installer artifacts are intentionally not tracked in normal Git history. The
Windows MSI and EXE files are large, change on every build, and should be
published through GitHub Releases or Git LFS.

## Recommended flow: GitHub Releases

Release artifacts are published by `.github/workflows/release.yml`.

Create and push a version tag:

```powershell
git tag v1.0.2
git push origin v1.0.2
```

The workflow builds the Windows MSI/NSIS installers on `windows-2025`, prepares
the FFmpeg sidecar, stages the optimized diarization runtime, creates a GitHub
Release, and uploads the generated installers as release assets.

You can also run the workflow manually from GitHub Actions and provide a
`release_tag`.

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
   C:\tmp\cargo-target\release\bundle\msi\Meeting Minutes AI_1.0.2_x64_en-US.msi
   C:\tmp\cargo-target\release\bundle\nsis\Meeting Minutes AI_1.0.2_x64-setup.exe
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
npm run runtime:diarize:install -- -PackPath "C:\path\meeting-minutes-diarize-runtime-1.0.2-windows-x64.zip"
```

The app resolves the bundled runtime first through the Tauri resource directory,
and still supports `MEETING_MINUTES_DIARIZE_ROOT` for development and repair.

## Alternative: Git LFS

If releases are not practical for internal distribution, track only installer
extensions with Git LFS. The LFS patterns are already declared in
`.gitattributes`; install LFS once and force-add the ignored files when needed:

```powershell
git lfs install
git add -f "executaveis/Meeting Minutes AI_1.0.2_x64_en-US.msi"
git add -f "executaveis/Meeting Minutes AI_1.0.2_x64-setup.exe"
```

Do not store generated binaries directly in regular Git commits.

