# Sample Projects

Fixtures:

- `basic-npm`: one TypeScript npm app with `src/env.private.ts`, `src/env.public.ts`, and `.env.example`.
- `monorepo`: one pnpm workspace with a Cloudflare Worker app, a Next.js app, a Hono backend app, and a shared package that must not own env vars.
- `multilanguage`: one mixed repo with Python, Next.js, Hono, and Cloudflare Worker apps.

These are meant to exercise adapter discovery, env graph merging, monorepo ownership checks, template expansion during `copy`, and drift reporting.

