# Release and installer artifacts

Installer artifacts are intentionally not tracked in normal Git history. The
Windows MSI and EXE files are large, change on every build, and should be
published through GitHub Releases or Git LFS.

## Recommended flow: GitHub Releases

Release artifacts are published by `.github/workflows/release.yml`.

Create and push a version tag:

```powershell
git tag v1.0.1
git push origin v1.0.1
```

The workflow builds the Windows MSI/NSIS installers on `windows-2025`, prepares
the FFmpeg sidecar, creates a GitHub Release, and uploads the generated
installers as release assets.

You can also run the workflow manually from GitHub Actions and provide a
`release_tag`.

## Local build and handoff

1. Build the app locally:

   ```powershell
   npm run tauri:build
   ```

2. Copy the generated artifacts into a local `executaveis/` folder if you want
   an easy handoff path. This folder is ignored by Git.

   With the local target dir used by this repo, the generated files are usually:

   ```powershell
   C:\tmp\cargo-target\release\bundle\msi\Meeting Minutes AI_1.0.1_x64_en-US.msi
   C:\tmp\cargo-target\release\bundle\nsis\Meeting Minutes AI_1.0.1_x64-setup.exe
   ```

## Alternative: Git LFS

If releases are not practical for internal distribution, track only installer
extensions with Git LFS. The LFS patterns are already declared in
`.gitattributes`; install LFS once and force-add the ignored files when needed:

```powershell
git lfs install
git add -f "executaveis/Meeting Minutes AI_1.0.1_x64_en-US.msi"
git add -f "executaveis/Meeting Minutes AI_1.0.1_x64-setup.exe"
```

Do not store generated binaries directly in regular Git commits.
