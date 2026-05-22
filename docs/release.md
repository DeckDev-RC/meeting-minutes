# Release and installer artifacts

Installer artifacts are intentionally not tracked in normal Git history. The
Windows MSI and EXE files are large, change on every build, and should be
published through GitHub Releases or Git LFS.

## Recommended flow

1. Build the app locally:

   ```powershell
   npm run tauri:build
   ```

2. Copy the generated artifacts from `src-tauri/target/release/bundle/` into a
   local `executaveis/` folder if you want an easy handoff path.

3. Publish the artifacts in a GitHub Release:

   ```powershell
   gh release create v1.0.0 `
     "executaveis/Meeting Minutes AI_1.0.0_x64_en-US.msi" `
     "executaveis/Meeting Minutes AI_1.0.0_x64-setup.exe" `
     "executaveis/meeting-minutes.exe" `
     --title "Meeting Minutes AI v1.0.0" `
     --notes "Build with adaptive transcription routing and quota fallback."
   ```

## Alternative: Git LFS

If releases are not practical for internal distribution, track only installer
extensions with Git LFS:

```powershell
git lfs install
git lfs track "executaveis/*.msi" "executaveis/*.exe"
git add .gitattributes
```

Do not store generated binaries directly in regular Git commits.
