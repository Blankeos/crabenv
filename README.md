A language-agnostic, multi-project, 100% opinionated way to manage environment variables.
These opinions make env var management simpler, which notoriously get a lot of drift at some point.

This project will once and for all solve environment variables. This CLI only does what you can already do manually. So it doesn't introduce any new config files, if your team doesn't want to use crabenv. Completely fine too!

📁 Languages supported:

- [x] TypeScript/Javascript and Monorepos (includes React, Solid, Vue, Svelte, Vite, NextJS, ReactNative, Backends, and Cloudflare apps)
- [x] Rust
- [x] Flutter
- [x] More? Request an adapter.

🤒 Pains solved:

- [x] CRUD and Documentation drift
- [x] Docker compose and Dockerfile drift (is this even possible, lmao)
- [x] `.env.example` and `env.ts` drift
- [x] Validation
- [x] Client and Server Boundaries
- [x] Local Development (creating the first .env) to Actual Deployment (translating that into the env on deployment) stories
- [x] A better `cp .env.example .env` command (This is not enough!) Creating envs (local env or for new dev,staging,prod envs).
  - [x] Use templating patterns like `"RSA_KEY=$(openssl  rand -base64 32)` - the crabenv copy
- [x] Rotating?? Kinda impossible actually.
- [x] Cloudflare projects copy the .env into .dev.vars

## Philosophy

- No new config file. Your team doesn't have to install crabenv, but it helps! The CLI just abstracts the manual maintenance.
- Env config is synced across **surfaces**:
  - 1. **Schema** - The validator language-specific schema. Multiple based on apps. i.e. `env.ts`, `env.private.ts`, `env.public.ts`, `config.rs`, `config.dart`.
  - 2. **Template (`.env.example`)** - Safe example/template values for env values. Multiple based on apps.
  - 3. **Local (`.env`)** - The actual local runtime values/secrets. 1 only.
  - 4. **Sinks (Docker, Wrangler)** (only if exists) - Multiple based on apps. i.e. `Dockerfile`, `docker-compose.yml`, `wrangler.toml`.
  - More? Make an adapter
- Packages in monorepos don't have .env.

## Usage

```sh
crabenv init
crabenv copy       # or crabenv cp.
crabenv doctor     # It's a checklist of common mistakes
  - [x] Surfaces are synced
    - [x] schema
    - [x] template
    - [x] local
    - [x] sinks (optional/read-only when present)
  - [x] Only one .env
  - [x] Cloudflare projects copy .env into .dev.vars
  - [x] Has with-env scripts to every project
  - [x] Consumption does not use
  - [x] Skip env validation in CI (based on CI=true env)
  - [x] Public variables are strict (like VITE_ or PUBLIC_ or NEXT_, or customized ones in vite config)
crabenv doctor --fix

# CRUD
crabenv list   # Lists variables
crabenv add    # Wizard-like experience
crabenv update # Wizard-like experience
crabenv remove # Wizard-like experience

crabenv add {VARIABLE_NAME}
    --example
    --optional # optional, required by default
    --default  # optional
    # Type flags
    --string   # optional, passed by default
    --numeric  # optional, conflicts w/ string. It's a string, but validates as a number (a numeric string)
    --number   # optional, conflicts w/ string (implicitly z.preprocess(Number))
    --boolean  # optional, conflicts w/ string (implicitly z.preprocess(Boolean))
    --enum     # optional, i.e. --enum=development,production
    # Custom regex
    --testRegex         # optional
    --testRegexMessage  # optional, required when used with `testRegex`
    # ... essentially a cli version of zod
    # Special environment overrides
    --{}_development     # Checks env=development, before applying the type
    --{}_staging         # Checks env=staging, before applying the type
    --{}_production      # Checks env=production, before applying the type
    --{}_ci              # Checks CI=true, before applying the type
crabenv remove {VARIABLE_NAME}
crabenv update {VARIABLE_NAME} # Same flags as add
```

## Current CLI

This repo currently has a first Rust CLI implementation that works against the sample projects in `_checks/sample-projects`.

```sh
crabenv --root _checks/sample-projects/basic-npm list
crabenv --root _checks/sample-projects/basic-npm doctor
crabenv --root _checks/sample-projects/basic-npm copy

crabenv --root _checks/sample-projects/monorepo doctor --fix --yes
crabenv --root _checks/sample-projects/monorepo add NEXT_PUBLIC_ANALYTICS_ID --owner apps/next-web --public --example dev --optional
crabenv --root _checks/sample-projects/monorepo remove NEXT_PUBLIC_ANALYTICS_ID --owner apps/next-web --public
```
