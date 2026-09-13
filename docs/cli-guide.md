---
title: CLI Guide
description: Compact crabenv CLI guide for agents
---

# CLI Guide

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
```

GitHub Actions sinks are supported through managed `gha-env` and `gha-echo` blocks. See [Sinks](./sinks/index.md).
When a managed sink covers a schema variable, `crabenv list -p`/`crabenv ls -p` includes `sinks` in that variable's surfaces, and `crabenv doctor` marks the `sinks` checklist cell with `[x]`.

CRUD commands:

```sh
crabenv list -p # notice -p since without it, it's interactive (difficult to use for agents).
crabenv list -p --json
crabenv add VARIABLE_NAME --example "value" --optional
crabenv update VARIABLE_NAME
crabenv remove VARIABLE_NAME
```

Agent rule: run `crabenv --help` first, prefer `crabenv ls -p --json` for machine-readable inventory or `crabenv ls -p` for a human-readable table over interactive `crabenv ls`, use the CLI for routine alignment, then edit files manually only when the CLI cannot express the needed change.

## Scope

Crabenv keeps env files organized and schemas, templates, and CI wiring aligned. It is a convenience and correctness tool—not a secret manager, leak scanner, or barrier preventing AI tools from reading env files. Application schemas provide runtime validation; doctor checks repository consistency and local presence, not actual value types.

## CI exit behavior

| Command | Exit behavior |
| --- | --- |
| `doctor` / `doctor --json` | Findings are advisory (exit 0). Operational failures still fail. |
| `doctor --check` | Exit 1 when an error-severity finding remains; warnings and infos do not fail. |
| `format --check` | Exit 1 when any file would change; exit 0 when no changes are planned. Never writes. |

Clap usage errors exit 2. Schema/template drift, managed-sink drift, and formatting notices are currently **warnings**, not doctor gate failures. Use `format --check` to enforce formatting separately.

For CI without local secrets, run `doctor --repo-only --check`. This skips local `.env` content reads, local presence/value checks, and local-file formatting checks. It still checks misplaced app-level `.env` file **existence** in monorepos. Repository templates, schemas, and managed sink definitions are still checked. `format --check` checks local `.env` files when present; it does not require them to exist.

`doctor --fix --yes --check` applies supported fixes, re-checks, then fails only for remaining errors. `--repo-only` also keeps these fixes away from local `.env` files.

## JSON diagnostics

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

## Formatting guarantees and limits

- Formatting sorts/groups supported entries without expanding references or executing `$(...)` templates.
- Directly attached comments/decorators move with their variable, including the first variable. Separate file/section headers from entries with a blank line to keep them as headers.
- Quoted multiline values remain one block; assignment-like text and section headers inside them are not treated as entries.
- Duplicate keys within a section retain their relative order and values. Commented assignments remain commented. Files with active duplicates across sections are left unchanged to preserve override order.
- Files containing active `$VAR` / `${VAR}` interpolation or unsupported active syntax (such as `export KEY=value`) are conservatively left unchanged. Literal single-quoted or escaped references and command templates remain sortable when otherwise supported.
- Unterminated active quoted values are not rewritten; CLI parsing may report an error before formatting. This command is not a dotenv validator.
- Repeated formatting produces the same output. Sorted files normalize line endings to LF and end with one newline; untouched files keep their original bytes.

`format --check` reports planned changes, not whether every construct is supported. A conservatively skipped file can pass unchanged. These guarantees apply to the formatter, not to `copy` template execution or mutation commands.
