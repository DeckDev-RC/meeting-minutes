# Hardening, Performance, and Regression Safety Spec

**Date:** 2026-05-26

## Goal

Reduce the highest-risk security, data-loss, database, async-runtime, frontend-render, and PDF-export issues while adding regression tests, unit tests, and lightweight benchmarks that can be rerun locally.

## Scope

- Sanitize generated meeting minutes HTML before browser insertion.
- Add a React error boundary around the application shell.
- Preserve unsaved structured-minute decision/action drafts across parent rerenders.
- Enable SQLite WAL and add high-value indexes.
- Add paginated meeting reads while preserving the existing default API.
- Replace blocking filesystem setup in async commands with async filesystem calls where practical.
- Clean temporary diarization/transcription work directories after worker completion.
- Reduce global layout rerenders from frequent progress updates by isolating the processing nav item.
- Keep PDF export dynamically imported and expose progress state to the export UI while rendering.

## Non-Goals

- Replace the single SQLite connection with a connection pool in this pass.
- Replace html2canvas with a different renderer in this pass.
- Add full transcript virtualization before measuring the render cost.
- Redesign the processing UI.

## Regression Requirements

- Tests must fail before implementation for new behavior.
- Rust DB tests must verify WAL, indexes, and pagination.
- Frontend unit tests must cover draft merge behavior.
- Browser regression tests must cover HTML sanitization and ErrorBoundary fallback.
- Static regression tests must guard against reintroducing selected blocking filesystem calls in async command setup.
- Lightweight benchmarks must run without external API calls.

