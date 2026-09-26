# Native protocol route fixtures

`native_wire_features.json` contains 32 request/response pairs captured by
`scripts/pi-omp-wire-extended.mjs` from Pi 0.87.1 and OMP 18.3.2 on 2026-09-26.
The source run is `.trellis/.runtime/research/omp-pi/runs/extended-2026-09-26T08-45-32-903Z/summary.json`.

Each client/protocol pair covers tool invocation and its tool-result follow-up,
image input, and thinking. Environment-generated system/developer prompts are
omitted, and workspace path prefixes are replaced with `C:/native-fixture/`.
Feature fields, model IDs, request paths, and SSE response shapes come from the
real SDK capture. Authentication headers are not stored in this fixture.

The router test compares complete request JSON and response SSE bytes with these
pairs, while separately checking source client and attempt count. Live process
execution through generated native configuration is covered by the ignored
`native_real_cli_eight_protocol_streams_through_actual_gateway` test.
