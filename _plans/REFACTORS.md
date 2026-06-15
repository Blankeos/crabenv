`crabenv ls` should not show `state`.

Decision: use the `surfaces` taxonomy instead:

- `schema` - language-specific validators/config schemas
- `template` - `.env.example`
- `local` - root/local `.env`
- `sinks` - future explicit deployment/runtime integrations via managed blocks; not inferred from arbitrary files today
