# Edge-case polish audit

The core model is simple: schema + template + one local file. The main risk is safely editing human-owned source without losing information.

## Fixed in this pass

- TypeScript CLI metadata updates (`--description`, `--optional`, `--default`) preserve existing validators instead of rebuilding them from the lossy inventory type. Oxc spans preserve custom regexes and embedded comments. Unsupported optional/default edits and invalid numeric/boolean defaults fail before schema/template writes.
- Rust fields are inserted into the actual top-level `Settings`/`Config` struct, not the last closing brace (which could belong to a helper or impl). Syn spans ignore declarations inside comments/strings, preserve unrelated structs, and validate output before writing.
- Multiline quoted dotenv values remain one entry across inventory, copy, formatting, update, and removal. Assignment-like lines and section headers inside values stay inside values. Disabled multiline values comment every physical line. Unterminated active quotes fail parsing/mutations; the standalone formatter leaves malformed content unchanged.

## Follow-ups (not fixed here)

- Python adapter: upsert currently emits `str` regardless of numeric/boolean flags and uses an example as a runtime default. Metadata updates can discard custom Field validators. This conflicts with the language-agnostic type/default intent.
- Rust adapter still does not generate runtime default helpers or enum validation from mutation flags; metadata rebuilds can discard serde defaults.
- TypeScript interactive update wizard supplies type flags even when retaining the selected type, so it can still rebuild custom validators. Regex-message-only edits also use the rebuild path. The preserving path in this patch is for CLI metadata-only updates, not every mutation mode.
- Workspace discovery supports limited wildcard expansion; recursive/complex workspace patterns need dedicated fixtures before claiming full pnpm/npm glob compatibility.
- `copy --overwrite` rebuilds from parsed active local entries. Free-form comments and unsupported assignment syntax can be lost; preserving arbitrary text needs a separate lossless overwrite plan.
- Command-template execution is intentionally enabled by the documented standard. Only run copy/template-expanding commands in trusted repositories; `--no-templates` disables it.
- Multi-file mutation is not transactional. Preflight checks cover the new TypeScript metadata path, but an I/O failure or later unsupported adapter can still leave a partially applied operation.

## Validation

- Regression suite covers metadata validators, unsupported edits leaving files unchanged, Rust helper/impl placement, comments/raw strings, Unicode spans, multiline values, disabled values, malformed quotes, and formatting idempotence.
- Independent temporary-project CLI smoke test: metadata update, multiline copy, repeated format, malformed copy refusal.
- Strict Clippy still reports existing style warnings outside this change's new logic; formatting and tests are checked separately.
