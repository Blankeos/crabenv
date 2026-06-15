# Better Sinks

Status: future plan, intentionally not implemented yet.

## Decision

Remove the current sink behavior that inferred env references from arbitrary deployment files such as Dockerfiles, Docker Compose files, and Wrangler files.

Sinks should come back only as explicit, opt-in, managed regions. crabenv should not guess that every `ARG`, `ENV`, `environment`, `vars`, or CI `env` entry is part of the crabenv env surface.

## Why

Deployment files are not a single reliable env surface:

- Dockerfiles mix build args, runtime env, image defaults, and non-app variables.
- Docker Compose can read `.env`, `env_file`, inline `environment`, shell expansion, secrets, or profiles.
- GitHub Actions often builds the image and is a better place to inject public build-time env.
- Cloudflare can use `.dev.vars`, Wrangler config, dashboard secrets, or CI-injected values.
- Some sink variables are combinations or deployment-only implementation details, not schema variables.

The only practice that feels consistently recommendable right now is: keep schema/template/local as the core, then explicitly copy/inject the public env needed by deployment/build tooling where that deployment actually happens, often in CI.

## Current Behavior

- `crabenv doctor` still shows a `sinks` column as a placeholder.
- Missing sinks render as `[-]` because sinks are optional/not implemented.
- crabenv does not currently scan or write Dockerfile, Docker Compose, Wrangler, `.dev.vars`, or GitHub Actions env blocks.

## Future Shape

Sinks should use managed placeholders/comments, not whole-file inference.

Example for comment-friendly formats:

```yaml
# crabenv:start format=github-actions filter=public
# crabenv:end
```

```dockerfile
# crabenv:start format=docker-args filter=public
# crabenv:end
```

```toml
# crabenv:start format=wrangler-vars filter=public
# crabenv:end
```

For JSON/strict JSON-adjacent files, comments are not valid. Possible workaround:

```json
{
  "//crabenv:start format=json-env filter=public": "",
  "//crabenv:end": ""
}
```

JSONC can use normal comments, but strict JSON needs a property-based sentinel if we ever support it.

## Open Decisions

### 1. Templating syntax

Need one syntax that is readable, explicit, and safe across formats.

Candidates:

```txt
crabenv:start format=<format> filter=<filter>
crabenv:end
```

or:

```txt
crabenv:begin format=<format> filter=<filter>
crabenv:end
```

Current preference: `crabenv:start` / `crabenv:end`.

### 2. Formats

Possible initial formats:

- `github-actions-env`
- `github-actions-build-args`
- `docker-args`
- `docker-env`
- `compose-environment`
- `dotenv`
- `json-env`

Avoid generic rendering until a real usecase demands it.

### 3. Filters

Only known real filter need right now:

```txt
filter=public
```

Potential filters:

- `all`
- `public`
- `private`
- `owner=<path>`

Current preference: start with only `filter=public`, because adding a full query language would make sinks feel like a config language.

### 4. Recommended Docker/Compose practice

Need one recommended practice before implementing anything.

Current lean:

- Do not duplicate env names into Dockerfiles by default.
- Prefer CI/deployment injection.
- For public build-time vars, GitHub Actions can materialize the required public env/build args at the point where the Docker image is built.
- Keep private runtime secrets in the deployment platform, not in Dockerfile-managed crabenv blocks.

### 5. Safety rules

Any future sink writer should:

- Only edit inside managed crabenv blocks.
- Never infer ownership from surrounding file content.
- Never overwrite user-managed content outside the block.
- Support dry-run first.
- Make missing blocks a recommendation, not an automatic mutation.
- Keep the “no new config file” philosophy.

## Non-goals For Now

- No Dockerfile scanning.
- No Docker Compose scanning.
- No Wrangler scanning.
- No `.dev.vars` writing.
- No GitHub Actions writing.
- No full templating language.
