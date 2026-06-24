# Better Sinks

Status: implemented for `gha-env` and `gha-echo`.

## Decision

Sinks exist only as explicit, opt-in, managed regions. crabenv must not infer env ownership from arbitrary deployment files.

Implemented syntax:

```txt
crabenv:start format=<format> scope=<scope> owner=<owner> [format-options]
crabenv:end
```

The generated body is replaceable. The sentinel comments stay stable.

## Current Behavior

- `crabenv doctor` parses managed sink blocks and reports drift as a fixable issue.
- `crabenv doctor --fix --yes` rewrites only managed block bodies.
- Only `.github/workflows/*.yml` and `.github/workflows/*.yaml` are scanned.
- Implemented formats: `gha-env`, `gha-echo`.
- `crabenv format` / `crabenv fmt` does not parse, sort, or rewrite sinks.

## Terminology

- **Schema**: language-specific contract such as `env.public.ts`, `env.private.ts`, `src/env.py`, or Rust config.
- **Template**: `.env.example` value surface.
- **Local**: `.env`, usually one at repository root.
- **Sink**: a consumer file that needs a subset of env names, usually for deployment/build/runtime wiring.
- **Managed block**: a sentinel-bounded region that crabenv is allowed to replace.

## Shared Rules

- Start marker: `crabenv:start`.
- End marker: `crabenv:end`.
- Directives are key/value tokens separated by spaces.
- `format`, `scope`, and `owner` are required.
- Unknown keys are rejected.
- Marker comments are format comments, not a crabenv-specific file type.
- The content between markers is crabenv-managed.
- The comments themselves are user-owned and preserved.
- Existing generated lines that use `${{ vars.NAME }}` or `${{ secrets.NAME }}` preserve that namespace choice for the same variable.
- Ordering uses `ordering.rs` so sink output is deterministic and matches list/format behavior.

## Owner selection

`owner=<path>` is required.

Examples:

```txt
owner=apps/web
owner=apps/api
owner=.
```

Do not support `owner=shared`, globs, or multiple owners in v1. A sink block should map to one app/deployment target.

## Implemented Format: `gha-env`

Use inside a GitHub Actions YAML `env:` map. Emits YAML map entries that reference GitHub Environment Variables or Secrets.

Directive:

```yaml
# crabenv:start format=gha-env scope=public owner=apps/web
# crabenv:end
```

Generated body:

```yaml
NEXT_PUBLIC_BASE_ORIGIN: ${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}
NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
```

Parameters:

- `format=gha-env` required.
- `scope=public` required.
- `owner=<path>` required.

There is intentionally no `source=vars` parameter. Public GitHub Actions env sinks default to `${{ vars.NAME }}`, and manual `${{ secrets.NAME }}` changes are preserved per variable.

There is intentionally no `dest` parameter. `gha-env` writes YAML map entries, not files.

Selection rule: render variables from schema records only:

- owner matches directive owner;
- scope is `Public`;
- schema surface exists.

## Implemented Format: `gha-echo`

Use inside a GitHub Actions `run: |` block. Emits shell lines that append env values to a dotenv file.

Directive:

```yaml
# crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
# crabenv:end
```

Generated body:

```sh
echo "DATABASE_URL=${{ secrets.DATABASE_URL }}" >> .env.local
echo "NEXT_PUBLIC_BASE_ORIGIN=${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}" >> .env.local
```

Parameters:

- `format=gha-echo` required.
- `scope=public`, `scope=private`, or `scope=all` required.
- `owner=<path>` required.
- `dest=<dotenv path>` required.

Default GitHub namespace:

- public schema variables use `${{ vars.NAME }}`;
- private schema variables use `${{ secrets.NAME }}`;
- manual `${{ vars.NAME }}` / `${{ secrets.NAME }}` changes are preserved per variable.

Selection rule: render variables from schema records only:

- owner matches directive owner;
- scope matches the directive scope;
- schema surface exists.

## CLI Shape

No standalone `sink` command. Sinks are part of `doctor` because they are another consistency surface, not a daily command.

Implemented:

```sh
crabenv doctor
crabenv doctor --fix
crabenv doctor --fix --yes
```

Behavior:

- `doctor`: parses managed blocks, renders expected content, and reports drift.
- `doctor --fix`: includes sink drift in the fix plan but does not write without `--yes`.
- `doctor --fix --yes`: replaces only managed block bodies.
- `format` / `fmt`: does not parse, sort, or rewrite sinks.

## Adapter Pattern

Sink formats should be adapter-shaped so new renderers do not change the scanner/replacer.

Current internal shape:

```rust
trait SinkAdapter {
    fn format(&self) -> &'static str;
    fn allowed_options(&self) -> &'static [&'static str];
    fn parse_scope(&self, value: &str, path: &Path, line_number: usize) -> Result<SinkScope>;
    fn normalize_options(
        &self,
        values: &mut BTreeMap<String, String>,
        path: &Path,
        line_number: usize,
    ) -> Result<()>;
    fn render(&self, context: SinkRenderContext<'_>) -> Result<Vec<String>>;
}
```

Shared scanner responsibilities:

- find `.github/workflows/*.yml` and `.yaml` files;
- parse `crabenv:start` / `crabenv:end` blocks;
- parse common directive fields;
- validate owner paths;
- preserve markers, indentation, line endings, and surrounding file content;
- replace only managed block bodies.

Format adapter responsibilities:

- declare supported option keys;
- validate format-specific scope behavior;
- select/render lines from the provided graph context.

Implemented adapters:

- `GhaEnvAdapter` for `format=gha-env`.
- `GhaEchoAdapter` for `format=gha-echo`.

Source layout:

```txt
src/sinks/
  mod.rs       # scanner, marker parser, shared sink helpers
  gha_env.rs   # gha-env adapter
  gha_echo.rs  # gha-echo adapter
  tests.rs     # sink integration tests
```

## Safety Rules

The sink writer must:

- Only edit inside managed crabenv blocks.
- Never infer ownership from surrounding file content.
- Never overwrite user-managed content outside the block.
- Support dry-run before writes.
- Make missing blocks a recommendation, not an automatic mutation.
- Keep the “no new config file” philosophy.
- Fail closed on unknown directive keys or unsupported formats.
- Preserve line endings where practical.
- Preserve indentation of the marker line.

## Non-goals

- No Dockerfile scanning or writing.
- No Docker Compose scanning or writing.
- No `.dev.vars` writing.
- No strict JSON support.
- No full templating language.
- No arbitrary include/exclude query language.
- No automatic insertion of sink blocks into files.
