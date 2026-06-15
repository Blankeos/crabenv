# Monorepo Fixture

This fixture checks a TypeScript monorepo with:

- `apps/cloudflare-worker`
- `apps/next-web`
- `apps/hono-api`
- `packages/shared`

Expected `crabenv` behavior:

- app workspaces own env vars.
- `packages/shared` does not own env vars.
- `crabenv copy` should merge app `.env.example` files into one root `.env`.
- Cloudflare `.dev.vars` is not written by crabenv; use project scripts/docs for that workflow.

