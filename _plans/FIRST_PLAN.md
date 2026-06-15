# First Plan

## Goal

Build `crabenv` as a Rust CLI for typed, validated, documented, and drift-resistant environment variable management across multi-project repos.

The core principle is adapter-first: the core must not know about `.env`, `.env.example`, `env.private.ts`, `docker-compose.yml`, Flutter config files, Rust config files, or any other concrete project file. The core works with a normalized env graph. Adapters discover, read, diff, and write existing files.

`crabenv` should not require a new config file. It should improve and synchronize files the project already has.

## Product Decisions

### 1. No New Config File By Default

For v1, do not introduce `crabenv.yml`, `.crabenvrc`, or another required checked-in config file.

Instead, build an in-memory `EnvGraph` from adapter observations:

- `@t3-oss/env-core` schema files are the primary source for validation contract fields: type, required/optional/default, public/private boundary, docs, and environment-specific rules.
- `.env.example` is the primary source for bootstrap example values and copy templates.
- `.env` is the primary source for local secret/runtime values, but should not define schema.
- `package.json` is the primary source for workspace discovery and env-loading scripts.
- `docker-compose.yml`, Dockerfiles, Cloudflare files, and deployment files are consumers/sinks that should be checked for drift and updated through explicit fix plans.

This is not a global source-of-truth hierarchy. It is field-level authority. When files disagree, `doctor` should report a drift item and create a confirmation plan before writing.

Examples:

- Variable exists in `env.private.ts` but not `.env.example`: offer to backfill `.env.example`.
- Variable exists in `.env.example` but not `env.private.ts`: offer to add it to the schema, keep it as example-only, or remove it.
- Variable exists in Docker but not the schema: offer to add to schema or remove from Docker.
- Variable exists in `.env` only: warn that it is local-only and ask whether to add it to schema/example or ignore.

Future option: if enough information cannot be round-tripped through existing files, add an opt-in generated metadata file later. This should be a deliberate v2 decision, not the default architecture.

### 2. Read And Write From Day One

Every v1 adapter must be capable of reading and writing, even if some write operations start conservative.

Writes must go through a plan:

1. Detect current files and build the env graph.
2. Compute proposed edits per adapter.
3. Show a human-readable summary.
4. Require confirmation before applying.
5. Write atomically and preserve formatting as much as practical.

`doctor --fix` should never silently overwrite a user's chosen convention.

### 3. Strong TypeScript Opinion

Strongly favor `@t3-oss/env-core` for TypeScript projects.

Do not spend v1 supporting many unrelated TypeScript env libraries. The TS adapter can be strict and opinionated:

- `src/env.private.ts` for private/server vars.
- `src/env.public.ts` for public/build-time vars.
- `runtimeEnv: process.env` for private vars.
- `runtimeEnvStrict` for public vars.
- `emptyStringAsUndefined: true`.
- app/process workspaces own env; shared packages receive validated config through injection.

### 4. Monorepos From Day One

Project discovery must support monorepos immediately.

Initial discovery sources:

- `package.json` workspaces.
- `pnpm-workspace.yaml`.
- Cargo workspace metadata where useful for Rust projects.
- Flutter app roots discovered by `pubspec.yaml`.

Ownership rule:

- apps/processes can own env vars.
- shared packages should not own process env.
- root `.env` is the local development file for a monorepo unless an adapter identifies a stronger existing convention.

### 5. Template Values In Examples

`.env.example` values may include templates like:

```env
JWT_SECRET="$(openssl rand -base64 32)"
```

`crabenv copy` must parse these and populate concrete values in `.env`.

Safety rules:

- Treat template commands as executable code.
- Show the command plan before running.
- Execute only during explicit commands like `crabenv copy`, not during `list` or `doctor`.
- Prefer direct process execution using parsed argv, not shell execution.
- Require a stricter opt-in flag for shell-only forms such as pipes, redirects, command chaining, or variable expansion.
- Capture stdout, trim one trailing newline, and write the result as the env value.

## Core Model

The normalized model should represent more than the lowest common denominator.

Start with:

```rust
EnvVar {
    name,
    owner,
    scope,
    value_type,
    required,
    default_value,
    example_value,
    docs,
    rules,
    sources,
}
```

Important fields:

- `name`: env variable name.
- `owner`: root project or workspace/app process.
- `scope`: private, public, or unknown.
- `value_type`: string, numeric string, number, boolean, enum, url, regex.
- `required`: required, optional, or environment-dependent.
- `default_value`: schema default, not local secret value.
- `example_value`: safe value or template from `.env.example`.
- `docs`: schema comments or adapter-supported inline docs.
- `rules`: production/staging/development/CI overrides.
- `sources`: source file, adapter, line/column/span, and field-level claims.

Use `EnvGraph` to merge adapter observations and produce:

- canonical field values by authority matrix.
- conflicts.
- missing-field diagnostics.
- proposed writes.

## Adapter Architecture

Define an adapter trait around capabilities, not file names hardcoded in core.

Suggested shape:

```rust
trait Adapter {
    fn id(&self) -> AdapterId;
    fn discover(&self, ctx: &ProjectContext) -> Result<Vec<AdapterTarget>>;
    fn read(&self, target: &AdapterTarget) -> Result<Vec<Observation>>;
    fn plan_write(&self, target: &AdapterTarget, graph: &EnvGraph, intent: &Intent) -> Result<Vec<FileEdit>>;
    fn capabilities(&self) -> AdapterCapabilities;
}
```

Core responsibilities:

- project discovery.
- adapter registry.
- merge observations into `EnvGraph`.
- detect drift.
- create intents from CLI commands.
- render confirmation plans.
- apply file edits atomically.

Adapter responsibilities:

- know file conventions.
- parse concrete syntax.
- preserve source locations.
- produce observations.
- write concrete syntax.

Initial adapters:

- `DotenvAdapter`: `.env`, `.env.example`, `.dev.vars`.
- `EnvCoreTsAdapter`: `src/env.private.ts`, `src/env.public.ts`, strongly favoring `@t3-oss/env-core`.
- `PackageJsonAdapter`: workspace discovery and env-loading scripts.
- `DockerComposeAdapter`: `docker-compose.yml` env blocks and env files.
- `DockerfileAdapter`: `ARG` and `ENV` declarations.
- `CloudflareAdapter`: `.dev.vars`, Wrangler conventions, and copy behavior from `.env`.

Follow-up adapters:

- Rust config adapter.
- Flutter/Dart config adapter, including `lib/config/env.dart`.
- Vite config adapter for public prefixes.
- Next.js prefix/boundary adapter.

## CLI Surface

### `crabenv init`

Detect the project shape and propose missing files/scripts.

Initial behavior:

- detect monorepo workspaces.
- identify app/process workspaces.
- propose `env.private.ts` / `env.public.ts` for TypeScript apps.
- propose `.env.example` where missing.
- propose root `.env` strategy for monorepos.
- propose `with-env` scripts where needed.
- write only after confirmation.

### `crabenv list`

Print the env graph.

Useful columns:

- name.
- owner.
- scope.
- type.
- required/default.
- example/local presence.
- source files.
- drift status.

### `crabenv copy` / `crabenv cp`

Create or update local env files from examples.

Behavior:

- non-monorepo: `.env.example` to `.env`.
- monorepo: merge root and app `.env.example` files into one root `.env`.
- dedupe variables by owner/name and report collisions.
- evaluate approved templates like `$(openssl rand -base64 32)`.
- copy `.env` into Cloudflare `.dev.vars` when applicable.
- never overwrite an existing non-empty value unless explicitly confirmed.

### `crabenv doctor`

Report drift and convention violations.

Initial checks:

- schema and `.env.example` are in sync.
- `.env` contains all required local vars.
- only one root `.env` in monorepos.
- app/process workspaces own env vars; packages do not.
- public vars use the expected prefix and strict runtime mapping.
- raw `process.env` consumption is either migrated or documented as an exception.
- Docker/Cloudflare/deployment references match schema/example variables.
- `with-env` scripts exist where workspace scripts need env loading.

### `crabenv doctor --fix`

Create a confirmation plan for each fix.

Fixes should be explicit:

- backfill `.env.example`.
- add schema entry.
- remove stale env entry.
- add `with-env` script.
- sync Docker env references.
- create `.dev.vars` from `.env`.

### CRUD Commands

Commands:

- `crabenv add VARIABLE_NAME`
- `crabenv update VARIABLE_NAME`
- `crabenv remove VARIABLE_NAME`

Flags:

- `--example`
- `--optional`
- `--default`
- `--string`
- `--numeric`
- `--number`
- `--boolean`
- `--enum=a,b,c`
- `--test-regex`
- `--test-regex-message`
- environment overrides for development/staging/production/CI.

These commands should write through all relevant adapters, not only one file.

Example for `add DATABASE_URL`:

- update `env.private.ts`.
- update `.env.example`.
- optionally update `.env`.
- optionally update Docker/Cloudflare references if selected.

## Implementation Roadmap

### Phase 0: Repo Scaffold

Create the Rust project:

- `Cargo.toml`
- `src/main.rs`
- `src/cli.rs`
- `src/core/*`
- `src/adapters/*`
- `src/planner/*`
- `src/template/*`
- `tests/fixtures/*`

Recommended crates:

- `clap` for CLI.
- `anyhow` and `thiserror` for errors.
- `serde`, `serde_json`, `serde_yaml` for structured data.
- `dotenvy` or a custom preserving parser for dotenv files.
- `jsonc-parser` or similar edit-preserving JSON tooling for `package.json`.
- `oxc_parser` or `tree-sitter-typescript` for TypeScript AST parsing.
- `similar` or similar diff tooling for plan output.
- `tempfile`, `assert_cmd`, and `insta` for tests.

Keep it as one package with a library and binary first. Split crates only when the boundaries have proven useful.

### Phase 1: Core Env Graph

Implement:

- project root detection.
- adapter registry.
- `Observation`.
- `SourceSpan`.
- `EnvGraph`.
- field-level authority resolution.
- conflict diagnostics.
- machine-readable `Plan`.

Deliverable:

- `crabenv list` can read fixture observations from adapters and print a graph.

### Phase 2: Dotenv Adapter And Copy

Implement:

- `.env` parser preserving comments/order where practical.
- `.env.example` parser.
- template parser for `$(...)`.
- `crabenv copy`.
- atomic file writes.
- collision handling for monorepo merge.

Deliverable:

- copy from `.env.example` to `.env`.
- evaluate safe templates after confirmation.
- do not overwrite existing values without confirmation.

### Phase 3: Env Core TypeScript Adapter

Implement:

- discover `src/env.private.ts` and `src/env.public.ts`.
- parse `createEnv` calls.
- read server/client schemas.
- read `runtimeEnv` and `runtimeEnvStrict`.
- parse the supported Zod subset.
- write schema additions/removals/updates.

Supported validation subset:

- `z.string()`
- numeric string refinements.
- `z.coerce.number()` or equivalent generated pattern.
- boolean/coerce boolean pattern.
- `z.enum([...])`
- `z.string().url()`
- regex validation.
- `.optional()`
- `.default(...)`
- simple environment-dependent required rules.

Deliverable:

- `crabenv list` and `doctor` understand env-core files.
- `crabenv add/update/remove` can modify env-core and `.env.example`.

### Phase 4: Doctor And Fix Plans

Implement:

- drift detection across adapters.
- interactive fix selection.
- dry-run output.
- `--fix` confirmation flow.
- structured issue severities.

Deliverable:

- real backfill workflow for schema/example drift.
- no silent overwrites.

### Phase 5: Monorepo And Package Scripts

Implement:

- package workspace detection.
- app/package classification heuristics.
- root `.env` merge behavior.
- `with-env` script detection and insertion.
- package env ownership warnings.

Deliverable:

- TypeScript monorepos work end to end.

### Phase 6: Docker And Cloudflare

Implement:

- Docker Compose env reference parsing/writing.
- Dockerfile `ARG`/`ENV` checks.
- Cloudflare `.dev.vars` copy support.
- Wrangler-related detection where useful.

Deliverable:

- `doctor` reports schema/example/docker/cloudflare drift.
- `doctor --fix` can backfill common Docker and Cloudflare issues.

### Phase 7: Rust And Flutter Adapters

Implement after the TS vertical slice is solid:

- Rust config adapter.
- Flutter/Dart config adapter.
- mapping between each language's config representation and the same core model.

Deliverable:

- README support claims are backed by adapter tests.

### Phase 8: Distribution

Mirror the practical release shape from `crabcode`.

Add:

- `dist-workspace.toml` using `cargo-dist`.
- generated `.github/workflows/release.yml`.
- Homebrew publishing to `blankeos/homebrew-tap`.
- `npm/package.json`.
- `npm/install.js`.
- `npm/bin.js`.
- `scripts/tag_and_release.sh`.
- `cliff.toml` and `CHANGELOG.md`.
- `justfile` commands for `dev`, `dist-build`, `tag`, and local smoke checks.

Initial targets:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

Publishing channels:

- crates.io package.
- GitHub releases with cargo-dist artifacts.
- Homebrew formula.
- npm package that downloads the matching GitHub release binary in `postinstall`.

## First Vertical Slice

The first useful version should handle this scenario:

1. A TypeScript monorepo with one app.
2. The app has `src/env.private.ts`, `src/env.public.ts`, and `.env.example`.
3. `crabenv list` shows all vars with source locations.
4. `crabenv doctor` detects schema/example drift.
5. `crabenv doctor --fix` proposes and applies selected backfills.
6. `crabenv copy` creates root `.env`, evaluating approved templates.
7. `crabenv add VARIABLE_NAME` updates env-core and `.env.example`.

This validates the core model, field authority, adapter writes, confirmation plans, and monorepo behavior before broadening language support.

## Test Strategy

Use fixtures heavily.

Test categories:

- adapter read snapshots.
- adapter write snapshots.
- graph merge and conflict tests.
- `doctor` diagnostics.
- `doctor --fix` plan snapshots.
- `copy` behavior with and without existing values.
- template parsing and execution using fake commands.
- monorepo merge and dedupe behavior.
- CLI integration tests with temp directories.

Every adapter should have:

- read fixture.
- write fixture.
- conflict fixture.
- malformed input fixture.

## Open Decisions

These should be settled during implementation, not ignored:

- Whether command templates should support shell syntax beyond simple argv commands.
- Whether docs should be parsed from comments, explicit schema descriptions, or both.
- How to represent environment-specific overrides in generated env-core code.
- How to classify apps vs packages reliably across non-TypeScript ecosystems.
- Whether duplicate variable names across multiple apps should be allowed in one root `.env`.
- Whether an optional generated metadata file is ever worth the philosophical cost.

## Immediate Next Steps

1. Scaffold the Rust crate.
2. Implement the core model and adapter trait.
3. Build the dotenv adapter and `copy`.
4. Build the env-core TypeScript adapter.
5. Build `doctor` conflict reporting and fix plans.
6. Add monorepo package discovery.
7. Add release scaffolding copied from the `crabcode` pattern.
# Historical Plan

This was the first broad plan. Concrete Docker/Wrangler/Cloudflare sink scanning and writing from this document is no longer the current direction. See `BETTER_SINKS.md` for the updated sink plan: explicit managed blocks only, no arbitrary deployment-file inference.

