# CI Change-Scope Contract

This contract owns the boundary between changed Git paths, the machine-readable
scope policy, conditional GitHub Actions jobs, and the stable final CI check.
The classifier must fail closed: uncertainty selects the complete CI suite.

## Policy Boundary

.github/ci-scope.json is the only configurable allowlist. Its schema version is
1; the top level contains exactly version, processDocumentation, and
checkedDocumentation. Each documentation rule set contains exactly:

- exactPaths: exact repository-relative POSIX paths.
- prefixRules: a prefix ending in a slash plus a non-empty extension allowlist.

Unknown or missing fields, duplicate values, unsafe paths, invalid prefixes, and
invalid extensions are policy errors. A path matching both rule sets is
ambiguous and therefore full.

The allowed tiers are intentionally narrow:

| Tier | Allowed paths |
| --- | --- |
| process-docs | AGENTS.md; Markdown/JSON/JSONL records under .trellis/tasks/; Markdown journals under .trellis/workspace/ |
| checked-docs | README.md, README_EN.md; Markdown under docs/ and .trellis/spec/ |
| full | Every path not explicitly allowed above |

An extension rule is part of the allowlist. It does not make the entire prefix
documentation-only. In particular, machine-readable contracts, source,
configuration, scripts, lockfiles, build/release inputs, images, and other
extensions remain full. CHANGELOG.md, .trellis/workflow.md, and
.trellis/agents/ also remain full because they affect releases or agent runtime
behavior.

CI control-plane ownership is not configurable. The classifier hard-codes
.github/, the classifier/self-test, the gate helper, and the workflow contract
self-test as full, even if the policy attempts to allow them. Any future CI
selection or gate helper must join this hard-coded set.

## Git Diff Boundary

The classifier accepts only repository-relative POSIX paths with no empty,
dot/dot-dot, backslash, control-character, or absolute-path forms. Git output
is read as NUL-delimited name-status data with rename detection and harder copy
detection enabled.

Paths that cannot be checked out safely on every supported desktop platform
also fail closed. Reject Windows-reserved device segments, Windows-invalid
filename characters, trailing dots/spaces, and any changed-path set containing
distinct paths that collide under case-insensitive comparison.

- Ordinary statuses consume one path. A delete is classified by its deleted
  path.
- Rnnn and Cnnn consume and classify both old and new paths.
- Unknown statuses, invalid scores, truncated records, missing terminal NUL, or
  invalid UTF-8 fail closed.

For a pull request, validate base/head object IDs, resolve their merge base,
then diff merge-base to head. For a push, validate and diff the event's before
to head pair. Do not replace either range with HEAD parent comparisons.
actions/checkout must fetch complete history so these objects and the merge
base are available.

All-zero or malformed object IDs, missing objects, Git failures, malformed
output, empty differences, manual dispatch, and unsupported events return
full. The CLI still exits successfully after a runtime classification error so
Actions can consume the safe result and run the complete suite; the classifier
self-test itself is allowed to fail the change-scope job.

## Classification Output

All changed paths must belong to one tier. Any cross-tier mixture fails closed
to full rather than merely selecting the highest documentation tier. A
checked-documentation path keeps docs_checks=true when a mixed diff raises the
aggregate to full, so its targeted contracts still run.

| scope | full_ci | docs_checks |
| --- | --- | --- |
| process-docs | false | false |
| checked-docs | false | true |
| full | true | true when a mixed diff contains checked documentation; otherwise false |

Every result also has a non-empty, single-line reason. Empty diffs and
classification errors use safe full results rather than an empty output.

## Workflow And Gate

change-scope always runs and always executes the dependency-free classifier
self-test before classification. docs-contract runs only when docs_checks is
true and invokes these repository-native, installation-free contracts:

- scripts/check-plugin-system-docs.mjs
- scripts/check-plugin-api-contract.mjs
- scripts/check-spec-links.mjs

support-contract, the complete desktop support matrix, frontend, and rust run
only when full_ci is true. Their full-tier steps remain the current workflow
contract, including the fork's upstream-sync policy self-test and manual-review
policy enforcement.

The job ID and display name ci-gate are stable. It uses an always condition and
directly needs every conditional job. `scripts/ci-gate.mjs` rejects missing or
inconsistent scope outputs, an unexpected PR-title result, and every
conditional result other than the selected contract:

| Selection | Required results |
| --- | --- |
| Pull request | pr-title is success |
| Push | pr-title is skipped |
| docs_checks is true | docs-contract is success |
| docs_checks is false | docs-contract is skipped |
| full_ci is true | support, aggregated desktop matrix, frontend, and rust are all success |
| full_ci is false | support, aggregated desktop matrix, frontend, and rust are all skipped |

This result check is deliberate: a selected job that was accidentally skipped
must fail the gate, and an unselected heavy job that unexpectedly ran must also
fail it.

## Change Checklist

1. Keep policy edits narrow and preserve the hard-coded control plane.
2. Add classifier self-tests for every new path rule, event, status, and output.
3. Update the gate whenever a conditional job or output is added.
4. Preserve all existing full-tier steps; a new tier may only skip work outside
   full.
5. Run the classifier and workflow contract self-tests, the three documentation
   contracts, actionlint when available, and git diff --check. The workflow
   self-test must structurally validate outputs, needs/if, complete gate
   dependencies/environment wiring, and execute the gate's fail-closed result
   matrix against the same helper used by Actions. It must also pin the current
   full-tier command inventory and forbid dependency installation, Cargo, or
   nonexistent release/TUI checks in docs-contract.
