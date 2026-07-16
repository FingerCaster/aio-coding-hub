# Implementation plan

- [x] Delete managed gateway Rust infrastructure, app service, commands, and
      integration tests.
- [x] Restore ordinary CLI proxy, Codex config, Provider Sync, startup, cleanup,
      CSP, and release workflow behavior.
- [x] Remove managed gateway settings and replace schema 49 with the legacy
      guard-removal migration.
- [x] Delete frontend manager/details/query/service/hook/event/mock surfaces.
- [x] Add the recommendation-only Codex card and exact repository URL test.
- [x] Regenerate TypeScript bindings.
- [x] Run focused Codex card and settings migration tests.
- [x] Run formatting, lint, typecheck, generated-binding, and negative-search
      checks.
- [x] Run full pre-commit and pre-push gates.
- [x] Build the Windows x64 MSI for user validation.
- [x] Commit the branch changes after the batch is confirmed.
