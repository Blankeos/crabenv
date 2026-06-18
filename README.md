The simplest, opinionated way to keep .env files, schemas, and examples aligned.

crabenv is an **opinionated**, language-agnostic CLI that keeps your environment variables aligned across schema, template, and local files. No new config file required. It validates, copies, and checks for drift so your team doesn't have to.

This project will once and for all solve environment variables typesafe schema definition and documentation so it will never drift. This CLI only does what you can already do manually. So it doesn't introduce any new config files, if your team doesn't want to use crabenv. Completely fine too!

📁 Languages supported:

- [x] TypeScript/Javascript and Monorepos (includes React, Solid, Vue, Svelte, Vite, NextJS, ReactNative, Backends, and Cloudflare apps)
- [ ] Python
- [ ] Rust
- [ ] Flutter
- [x] More? Request an adapter.

🤒 Pains solved:

- [x] CRUD and Documentation drift
- [ ] Deployment/sink drift via explicit managed blocks, not arbitrary file inference
- [x] `.env.example` and `env.ts` drift
- [x] Validation
- [x] Client and Server Boundaries
- [x] Local Development (creating the first .env) to Actual Deployment (translating that into the env on deployment) stories
- [x] A better `cp .env.example .env` command (This is not enough!) Creating envs (local env or for new dev,staging,prod envs).
  - [x] Use templating patterns like `"RSA_KEY=$(openssl  rand -base64 32)` - the crabenv copy
- [x] Rotating?? Kinda impossible actually.
- [ ] Cloudflare `.dev.vars` guidance/docs

## Philosophy

- No new config file. Your team doesn't have to install crabenv, but it helps! The CLI just abstracts the manual maintenance.
- Env config is synced across **surfaces**:
  - 1. **Schema** - The validator language-specific schema. Multiple based on apps. i.e. `env.ts`, `env.private.ts`, `env.public.ts`, `config.rs`, `config.dart`.
  - 2. **Template (`.env.example`)** - Safe example/template values for env values. Multiple based on apps.
  - 3. **Local (`.env`)** - The actual local runtime values/secrets. 1 only.
  - 4. **Sinks** - Reserved for future explicit integrations. crabenv does not currently infer or rewrite arbitrary deployment files.
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
    - [-] sinks (future explicit integrations)
  - [x] Only one .env
  - [ ] Cloudflare `.dev.vars` guidance/docs
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
