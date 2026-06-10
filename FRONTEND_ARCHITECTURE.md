# Frontend Architecture — Production-grade for RiiMail

## Overview
- Goal: fast, robust Tauri front-end that maps cleanly to the Rust backend, supports offline/incremental sync, and follows production best practices.

## Tech Choices
- Language: TypeScript
- Framework: React (functional components). Alternative: Svelte for smaller bundles.
- Bundler: Vite (Tauri-compatible, fast HMR)
- UI primitives: Headless components + Tailwind or Chakra UI for accessibility and consistency

## API / IPC Layer
- Single `api/` module that wraps all Tauri commands (`check_app_status`, `login`, `fetch_emails`, `refresh_emails_handler`, `rater`, `config_setup`, `logout_with_state`).
- Contract-first: keep TypeScript types aligned with backend models (`Email`, `RefreshSummary`, `AppState`). Generate or hand-maintain types to avoid drift.
- Centralized retry / timeout / backoff logic and error normalization for RPC/LLM calls.

## Data Layer & Sync
- Use TanStack Query (React Query) for caching, background refetch, pagination, invalidation, and optimistic updates.
- Drive long-running incremental sync by calling `refresh_emails_handler` on intervals and on resume/visibility; coordinate with backend cancel tokens.
- Use virtualized lists (react-window) and server-side pagination via `fetch_emails` to handle large mailboxes.
- Prefetch likely next data (threads or mailboxes) to improve perceived performance.

## State Management
- Keep ephemeral UI state (selected thread, composer open) in React Context or a lightweight store (Zustand).
- Let Query caches hold authoritative email data; avoid duplicating large arrays in global state.
- Use memoized selectors for derived data (counts, unread, filters).

## UI Architecture
- Component model: atomic components (Button, List, Avatar) + container/page components that handle data fetching.
- Separation of concerns: presentational vs container components; containers call Query hooks.
- Routing: React Router with route-based code-splitting and lazy-loading for heavy pages (composer, settings).
- Accessibility: keyboard navigation, ARIA attributes, focus management for lists and modals.

## Performance
- Lazy-load heavy components (composer, settings, rater UI) and large assets.
- Batch/coalesce UI updates during bulk DB inserts (initial sync) to avoid re-renders.
- Sanitize and cache HTML previews; defer rendering heavy previews until visible.
- Use React.memo, useCallback, and virtualization for long lists.

## Security & Privacy
- Keep credentials out of front-end storage — rely on backend `Apple Keychain` management.
- Sanitize all HTML from backend before rendering; use a secure renderer or sanitized innerHTML.
- IPC least-privilege: expose only minimal Tauri commands; validate inputs server-side.
- Apply CSP and disable risky eval-like functions in web views.

## Offline & Resilience
- Provide a cached read-only UI from SQLite via backend handlers when offline.
- Queue user actions that require network and flush them when connectivity resumes.
- Surface sync conflicts to users when relevant; otherwise use last-write-wins for non-critical edits.

## Testing
- Unit: Jest + Testing Library for components, hooks, and utilities.
- Integration/E2E: Playwright testing against the packaged app or web build for login → sync flow.
- Contract tests: type-checked tests for `api/` wrappers with mocked backend responses.
- Accessibility checks: axe-core in CI.

## CI / CD / Releases
- CI steps: lint → typecheck → unit tests → contract checks → E2E smoke → build.
- Package with Tauri for macOS; sign and notarize macOS builds for production distribution.
- Semantic versioning and automated changelogs (semantic-release).

## Observability
- Error reporting: Sentry or similar, with PII-scrubbing and opt-out.
- Metrics: track sync latencies, refresh counts, and error rates; correlate frontend events with backend traces (OTLP endpoint).
- Structured client logs that can be correlated with backend tracing.

## Developer Experience
- Fast HMR with Vite and mocked backend stubs for UI-only development.
- Strict `tsconfig` and lint rules; pre-commit hooks for formatting and tests.
- API surface documented in `api/` module and a short README for running/dev commands.

## Concrete Next Steps
1. Generate TypeScript types from backend models in `src-tauri` or hand-author matching types in `api/types.ts`.
2. Scaffold Vite + React + TypeScript project and add `api/` bridge, TanStack Query, virtualization, and baseline E2E test.
3. Add CI (lint, typecheck, unit tests, basic Playwright smoke) and Tauri packaging job.

---
If you want, I can scaffold the Vite + React + TypeScript project and create the `api/` wrappers next.
