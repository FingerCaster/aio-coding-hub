# Implementation Plan

1. Add a shared, case-insensitive capacity-signal predicate suitable for both
   stream recognition and client sanitization without weakening terminal-event
   gating.
2. Harden `sanitize_attempts_for_client` so client attempts contain neither raw
   capacity evidence nor capacity-bearing replacement text.
3. Add focused unit and route coverage proving client/internal separation.
4. Clean Rust and TypeScript new-user retry defaults and update exact-default
   tests and fixtures.
5. Update the upstream-error-handling contract to require removal of all
   capacity aliases from client diagnostics and document the cleaned defaults.
6. Run focused tests, Rust formatting and Clippy, TypeScript checks, generated
   binding verification, and the broader relevant test suites.
