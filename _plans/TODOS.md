- [x] a crabenv format (alias fmt), that automatically sorts everything well...
  - Rules:
    - Group by proper prefixes i.e. `S3_*` and `RESEND_*` grouped together well.
    - The env example usual groupings for `# --- apps/something ---` is important as well.
    - Always put the standard envs to the very top i.e. `NODE_ENV` `CI`, the usual stuff you can find in a lot of places, it makes sense to make this deterministic.
    - Preserve comments
    - Preserve placeholder variables
  - The very core of this feature is also "sorting" so it might also make sense to standardize this sorting rule so that for displaying commands like "list/ls/doctor" or the select options when doing `crabenv add/delete/update`, they're shown properly

- [x] a description system for env vars.. The rule we currently have is that they're stored in the schema files as comments just above the env var definition.

- [ ] data sink template replacer. what will be the syntax pattern?
