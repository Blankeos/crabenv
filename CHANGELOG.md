# Changelog

All notable changes to this project will be documented in this file.

## [0.0.4] - 2026-07-02

### Bug Fixes

- Retain enum defaults when updating variables by @Blankeos
- Preserve quote style and ignore inline comments in env parsing by @Blankeos
- Format env schema files after mutations and preserve trailing comments by @Blankeos

### Features

- Add comment-aware optionals, json output, fixes on add/update/remove/cp by @Blankeos
- Support partial updates for existing env variables by @Blankeos

### Refactor

- Remove redundant list detail formatter by @Blankeos
- Better message before applying update or add by @Blankeos

## [0.0.3] - 2026-07-02

### Bug Fixes

- Preserve local-only env entries and mark them as non-actionable by @Blankeos
- Preserve trailing unkeyed schema entries when formatting by @Blankeos

### Features

- Add interactive env list UX with explicit plain output mode by @Blankeos
- Add format issue detection and format fix execution by @Blankeos

## [0.0.2] - 2026-06-24

### Documentation

- Revamp onboarding docs and sync guide content by @Blankeos

### Features

- Surface managed sink coverage in env listings by @Blankeos
- Add managed GitHub Actions sink sync support by @Blankeos
- Initialize missing env surfaces in `init` command by @Blankeos
- Add expandable output for shared owners and enum values by @Blankeos
- Allow --shared to target specific app owners by @Blankeos

## [0.0.1] - 2026-06-18

### Bug Fixes

- Allow empty input when value prompt is optional by @Blankeos
- Sort inventory rows by owner rank before env name by @Blankeos
- Ignore comments when parsing env schema objects (format works now!) by @Blankeos
- Strip block comments when parsing env schema objects by @Blankeos

### Chores

- Add cargo-dist release workflow and npm distribution tooling by @Blankeos
- Used fmt on monorepo by @Blankeos

### Documentation

- Unify crabenv messaging across metadata, docs, and CLI help by @Blankeos

### Features

- Add crabenv agent skill docs and generation tooling by @Blankeos
- Add Rust env adapter with multi-language fixture coverage by @Blankeos
- Add env var descriptions to mutation and schema generation by @Blankeos
- Support single env.ts schema mode by @Blankeos
- Surface environment variable descriptions from schema comments by @Blankeos
- Added formatting (not working properly yet) by @Blankeos
- Add interactive prompts for env mutation commands by @Blankeos
- Keep monorepo root `.env.example` in sync by @Blankeos
- Add env var surface inventory output to doctor by @Blankeos
- Scaffold crabenv with adapter-based env management core by @Blankeos

### Refactor

- Drop inferred Docker/Compose/Wrangler sink handling by @Blankeos

### Doc

- Readme by @Blankeos
- Doc corrections by @Blankeos
- More readme tweaks by @Blankeos
- Added python by @Blankeos
- Added important documentation for humans to solidify the standard by @Blankeos


### New Contributors

- @Blankeos made their first contribution

