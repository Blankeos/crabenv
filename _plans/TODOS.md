- [x] a crabenv format (alias fmt), that automatically sorts everything well...
  - Rules:
    - Group by proper prefixes i.e. `S3_*` and `RESEND_*` grouped together well.
    - The env example usual groupings for `# --- apps/something ---` is important as well.
    - Always put the standard envs to the very top i.e. `NODE_ENV` `CI`, the usual stuff you can find in a lot of places, it makes sense to make this deterministic.
    - Preserve comments
    - Preserve placeholder variables
  - The very core of this feature is also "sorting" so it might also make sense to standardize this sorting rule so that for displaying commands like "list/ls/doctor" or the select options when doing `crabenv add/delete/update`, they're shown properly

- [x] a description system for env vars.. The rule we currently have is that they're stored in the schema files as comments just above the env var definition.

- [x] data sink template replacer. what will be the syntax pattern?

- [x] `crabcode ls` In 'shared(2)', maybe an option to expand this shared(2). Like which specific "apps".
- [x] `crabcode ls` In enum(2), might wanna see the specific enum values.
- [x] Add fixes for the `--shared` case.. i.e. being able to choose the specific or `"*"` all. so maybe also say `shared(*)` if it's all.
  - some ideas, in cli: be able to pass multiple apps. in wizard, have a multiselect (no need filter) option maybe.

- [x] Make a TUI version of `crabenv ls` and make that the default using ratatui. The print mode is just when we do `crabenv ls --print`. Reason: the command is normally done via a human and the table is not readable for a human.

- [x] Very important. apparently, optional variables that are defined in `.env` files are ALWAYS EVALUATED as not empty.
  - The simple solutions in my head are:
    - ~~typescript: always add emptyStringAsUndefined: true, but for other languages idk (so this becomes a typescript-only-like solution).~~
    - something that fits all languages: as long as a variable's SCHEMA defines optional && EXAMPLE defines none, add a `# VAR` to it. meaning comment it out, but make it appear in the .env.example (when generating an example) and the .env (when done via crabenv cp) - gpt likes this!

- [ ] Enum values during `crabenv update` doesn't show the 'old' values. it's always input required.
