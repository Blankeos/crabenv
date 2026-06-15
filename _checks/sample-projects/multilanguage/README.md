# Multilanguage Fixture

This fixture checks a mixed repository with:

- `apps/python-service`
- `apps/next-web`
- `apps/hono-api`
- `apps/cloudflare-worker`

Expected `crabenv` behavior:

- TypeScript apps use `@t3-oss/env-core`.
- Python uses an existing config module, not a crabenv-specific config file.
- `crabenv copy` should merge all app `.env.example` files into one root `.env`.
- Docker and Cloudflare files are not inferred as sinks; future sink support should use explicit managed blocks.

