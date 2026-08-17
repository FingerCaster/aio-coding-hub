# Beta release baseline

- Date: 2026-08-17
- Repository: `FingerCaster/aio-coding-hub` (`origin` only)
- Initial `origin/main`: `d02728cea0990c0fc019c9c8c6bfa67796a0295b`
- Stable source version files: `0.60.40`
- Current Beta pointer: `aio-coding-hub-v0.60.41-beta.8`
- Promotion high-water: `0.60.41-beta.8`
- Pointer source SHA: `c56f589e74115a10cc82392f0cc325a87d5a7158`
- Next free candidate observed during planning: `aio-coding-hub-v0.60.41-beta.9`; its tag and Release were absent.
- Required dispatch inputs: `release_channel=beta`, canonical Beta `release_tag`, and the full 40-hex `origin/main` merge SHA as `target_commitish`.
- Last successful Beta had `draft=false`, `prerelease=true`, and exactly 14 official assets. The next publication must preserve this matrix and leave stable latest/Homebrew unchanged.
- `release-channels` ref observed at `60caaf330fbc93eb75179075493bb95165dad5df`; publication must use the workflow's guarded CAS rather than a direct or forced ref update.
- Revalidate every remote fact immediately before dispatch because another release may advance the high-water while implementation and CI run.
