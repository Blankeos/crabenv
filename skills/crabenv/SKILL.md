---
name: crabenv
description: Read when you need to add, update, or delete env vars. Understand and apply the crabenv env var management standard: one local env, aligned schemas, templates, docs, and deployment sinks across languages.
---

`crabenv` is an env var management standard created by [Carlo Taleon](http://carlo.tl) to minimize env var schema + documentation drift in any codebase. If you follow this standard, you'll find it extremely seamless to "develop locally" and "deploy to production" in any platform!

It's available as:

1. a cli (recommended)
2. a skill (for agents, no cli needed)
3. a guide (for humans, no cli needed)

You're currently reading the "skill" / "guide". If you're planning to use the CLI, just read the concepts and then use `crabenv --help` and you'll understand how to use it.

## Getting started

**New project:**

```sh
crabenv init   # create missing schema + .env.example surfaces (never overwrites)
crabenv copy   # create local .env from examples (keeps existing values)
crabenv doctor # fix drift until clean
```

**Existing project:** ask your agent to align envs to this guide until `crabenv doctor` is clean.

Then `crabenv add/update/remove` for changes, `crabenv ls -p` for inventory.

## Goals

- [x] Typesafety & Validation
- [x] Good documentation. Never stale, does what it says.
- [x] Seamless _local development_ to _deployment_ story.
- [x] No new config files. Your team doesn't need to install crabenv, it's just manual-crud made automated via CLI.
- [x] Language-agnostic. No need to learn language-specific configurations for multi-language and monorepos, just use the same CLI commands.

## Concepts

Env Management in production codebases is essentially:

1. **Local** (`.env`) - Can be absent. Always one.
2. **Schema** (`env.*.ts`, `env.rs`, `env.py`, `env.dart`, etc.) - Required. These are opinionated names, deal with it 😎
3. **Template** (`.env.example`) - Required.
4. **Sinks** (`.github/workflows/*.yml`, etc.) - Can-be-absent. Explicit managed regions that include the subset of envs, such as public `NEXT_PUBLIC_*` vars, that are essential for builds.

## Agnostic Rules

```sh
# File structure (ts for example)
- .env
- .env.example
- src/
    - env.*.ts
```

This is applicable for every language.

1. Locally, always only one `.env`. Even in monorepos.
2. Always make sure that a variable defined in "Schema" is defined in "Template", "Local", "Sinks".
3. In Template (`.env.example`) - always add a default value if you can.
4. In Template (`.env.example`) - you can indicate to make default random values with `"$(openssl rand -hex 32)"` or `"$(pwd)/local.db"` - this helps in self-hosted deployments, etc.
5. Documentation should live in the "schema" file. i.e. "I got this variable from"
6. As much as possible the structure/sorting should be equal for: `.env.example` (example values) = `.env` (real values)

Special monorepo/multi-language repo rules:

```sh
# File Structure (ts for example)
- .env
- .env.example
- apps/
    - app1/
        - src/env.*.ts
        - .env.example
    - app2/
        - src/env.*.ts
        - .env.example
```

1. Again, always have one `.env` at the very root of the codebase. It makes definition simpler.
2. Use a `"with-env"` script to always channel the root env into sub-apps in a monorepo.
3. NEVER add envs in sub-packages (i.e. `packages/*` in npm monorepos)
4. Using the same name ALWAYS MEANS "shared variable" i.e. `DATABASE_URL` in app1 and app2 should always mean the same thing. If it's not meant to be shared, just call it differently. (It really helps to make a distinction for it)
5. For the ROOT `.env` and `.env.example`, the structure should look like this (note the comment groupings):

   ```sh
   # shared
   NODE_ENV="production"
   DATABASE_URL=""

   # apps/app1
   RSA_PRIVATE_KEY="123"

   # apps/app2
   CMS_URL="http://localhost:3001"
   ```

> For language-specific rules/examples, visit **Language Guides**.

For implemented sink formats, see [Sinks](./sinks/index.md).

## CLI Guide

Prefer the CLI when it is installed; otherwise apply the docs manually.

```sh
crabenv --help        # inspect current commands/options
crabenv init          # create/align schema + .env.example
crabenv copy          # create/update local .env from .env.example
crabenv ls -p         # print expanded env inventory table; use this for agents/scripts
crabenv ls -p --json  # print compact JSON env inventory for agents/scripts
crabenv doctor        # detect drift, common mistakes, and managed sink drift
crabenv doctor --fix  # preview safe fixes
crabenv doctor --fix --yes # apply safe fixes
crabenv doctor --repo-only --check # CI gate without reading local .env contents
crabenv doctor --repo-only --json  # machine-readable diagnostics
crabenv format        # or fmt: sort/group env files and supported schemas
crabenv format --check # fail on formatting drift, without writing
crabenv skill         # install agent skill (thin wrapper around npx skills)
```

GitHub Actions sinks are supported through managed `gha-env` and `gha-echo` blocks. See [Sinks](./sinks/index.md).
When a managed sink covers a schema variable, `crabenv list -p`/`crabenv ls -p` includes `sinks` in that variable's surfaces, and `crabenv doctor` marks the `sinks` checklist cell with `[x]`.

CRUD commands:

```sh
crabenv list -p # notice -p since without it, it's interactive (difficult to use for agents).
crabenv list -p --json
crabenv ls      # interactive: search, press Enter on a variable, then choose Update / Remove / Attach / Back to list; Esc goes back or quits. Removal keeps its confirmation prompt.
crabenv add VARIABLE_NAME --example "value" --optional
crabenv update VARIABLE_NAME
crabenv remove VARIABLE_NAME
```

Agent rule: run `crabenv --help` first, prefer `crabenv ls -p --json` for machine-readable inventory or `crabenv ls -p` for a human-readable table over interactive `crabenv ls`, use the CLI for routine alignment, then edit files manually only when the CLI cannot express the needed change.

### Scope

Crabenv keeps env files organized and schemas, templates, and CI wiring aligned. It is a convenience and correctness tool—not a secret manager, leak scanner, or barrier preventing AI tools from reading env files. Application schemas provide runtime validation; doctor checks repository consistency and local presence, not actual value types.

### CI exit behavior

| Command | Exit behavior |
| --- | --- |
| `doctor` / `doctor --json` | Findings are advisory (exit 0). Operational failures still fail. |
| `doctor --check` | Exit 1 when an error-severity finding remains; warnings and infos do not fail. |
| `format --check` | Exit 1 when any file would change; exit 0 when no changes are planned. Never writes. |

Clap usage errors exit 2. Schema/template drift, managed-sink drift, and formatting notices are currently **warnings**, not doctor gate failures. Use `format --check` to enforce formatting separately.

For CI without local secrets, run `doctor --repo-only --check`. This skips local `.env` content reads, local presence/value checks, and local-file formatting checks. It still checks misplaced app-level `.env` file **existence** in monorepos. Repository templates, schemas, and managed sink definitions are still checked. `format --check` checks local `.env` files when present; it does not require them to exist.

`doctor --fix --yes --check` applies supported fixes, re-checks, then fails only for remaining errors. `--repo-only` also keeps these fixes away from local `.env` files.

### JSON diagnostics

```sh
crabenv doctor --repo-only --json --check > doctor-report.json
```

Successful diagnostic collection writes exactly one JSON object to stdout, including when `--check` fails. Operational errors go to stderr and may prevent a report from being emitted. `--json` cannot be combined with `--fix` or `--yes`.

The version 1 report has `version`, `check`, `repo_only`, `issues`, and `summary` fields. Each issue includes:

- `code`: stable machine-readable identifier; prefer this to parsing message text.
- `severity`: `error`, `warn`, or `info`.
- `message`: human-readable explanation.
- `owner`: repository-relative owner (`apps/web`, `.`), or `null` for project-wide issues.
- `paths`: affected repository-relative paths, using `/` separators.
- `remediation`: suggested next step.
- `fixable`: whether an automatic doctor fix is available.

`summary` contains `error`, `warn`, `info`, and `total` counts. Consumers should tolerate new issue codes and additional fields.

Current issue codes:

| Code | Meaning |
| --- | --- |
| `sink-drift` | Managed CI blocks differ from expected output. |
| `missing-local-env` | The local `.env` file is absent. |
| `monorepo-local-misplaced` | An app has a local `.env` instead of using the monorepo root. |
| `package-owns-env` | A non-app package owns env files. |
| `schema-without-template` | A schema variable is missing from its template. |
| `template-without-schema` | A template variable is missing from its schema. |
| `public-missing-runtime-strict` | A public schema lacks `runtimeEnvStrict`. |
| `public-var-missing-runtime-strict` | A public variable lacks its runtime mapping. |
| `missing-with-env-script` | A monorepo app lacks a `with-env` script. |
| `required-missing-from-local` | A required template variable is absent locally. |
| `local-only-var` | A local variable has no schema or template definition. |
| `needs-formatting` | Files would change under `format`. |

### Formatting guarantees and limits

- Formatting sorts/groups supported entries without expanding references or executing `$(...)` templates.
- Directly attached comments/decorators move with their variable, including the first variable. Separate file/section headers from entries with a blank line to keep them as headers.
- Quoted multiline values remain one block; assignment-like text and section headers inside them are not treated as entries.
- Duplicate keys within a section retain their relative order and values. Commented assignments remain commented. Files with active duplicates across sections are left unchanged to preserve override order.
- Files containing active `$VAR` / `${VAR}` interpolation or unsupported active syntax (such as `export KEY=value`) are conservatively left unchanged. Literal single-quoted or escaped references and command templates remain sortable when otherwise supported.
- Unterminated active quoted values are not rewritten; CLI parsing may report an error before formatting. This command is not a dotenv validator.
- Repeated formatting produces the same output. Sorted files normalize line endings to LF and end with one newline; untouched files keep their original bytes.

`format --check` reports planned changes, not whether every construct is supported. A conservatively skipped file can pass unchanged. These guarantees apply to the formatter, not to `copy` template execution or mutation commands.

## Skill references

Use the concept above first. For exact language examples, read the matching reference file:

- `references/typescript-javascript.md` — Schema conventions for @t3-oss/env-core, zod, public/private env files, and monorepo with-env scripts.
- `references/python.md` — Pydantic settings conventions for env.py, Field aliases, and monorepo run scripts.
- `references/rust.md` — Serde/Figment conventions for src/config.rs, explicit env renames, and workspace run scripts.
- `references/flutter-dart.md` — String.fromEnvironment conventions, dart-define files, and public-only mobile env rules.
- `references/github-actions.md` — Deployment sink notes for GitHub Actions.
