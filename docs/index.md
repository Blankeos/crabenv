`crabenv` is an env var management standard created by [Carlo Taleon](http://carlo.tl) to minimize env var schema + documentation drift in any codebase. If you follow this standard, you'll find it extremely seamless to "develop locally" and "deploy to production" in any platform!

It's available as:

1. a cli (recommended)
2. a skill (for agents, no cli needed)
3. a guide (for humans, no cli needed)

You're currently reading the "skill" / "guide". If you're planning to use the CLI, just read the concepts and then use `crabenv --help` and you'll understand how to use it.

## Goals

- [x] Typesafety
- [x] Good documentation
- [x] Seamless local development -> deployment story.

## Concepts

Env Management in production codebases is essentially:

1. **Local** (`.env`) - Can be absent. Always one.
2. **Schema** (`env.*.ts`, `env.rs`, `env.py`, `env.dart`, etc.) - Required. These are opinionated names, deal with it 😎
3. **Template** (`.env.example`) - Required.
4. **Sinks** (`docker.compose.yml`, `.github/actions/*.yml`, `Dockerfile`, etc.) - Can-be-absent. Can include the subset of envs i.e. `NEXT_PUBLIC_*` vars that are essential for builds.

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

> For language-specific rules/examples, visit `/language/*.md`.
