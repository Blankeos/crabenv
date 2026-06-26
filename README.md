# 🦀 crabenv

The simplest, opinionated way to keep .env files, schemas, and examples aligned.

`crabenv` is an env var management standard created by [Carlo Taleon](http://carlo.tl) to minimize env var schema + documentation drift in any codebase. If you follow this standard, you'll find it extremely seamless to "develop locally" and "deploy to production" in any platform!

## Why use it

- [x] Typesafety & Validation
- [x] Good documentation. Never stale, does what it says.
- [x] Seamless _local development_ to _deployment_ story.
- [x] No new config files. Your team doesn't need to install crabenv, it's just manual-crud made automated via CLI.
- [x] Language-agnostic. No need to learn language-specific configurations for multi-language and monorepos, just use the same CLI commands.

## Installation

```sh
brew install blankeos/tap/crabenv # Homebrew (macOS/Linux)
npm install -g crabenv            # or npm
bun install -g crabenv            # or bun
cargo binstall crabenv            # or cargo-binstall (prebuilt binary, faster)
cargo install crabenv             # or cargo (build from source)
curl -sSL https://raw.githubusercontent.com/Blankeos/crabenv/main/install.sh | sh # or linux/macos (via curl)
```

## Quickstart

> 💡 Before anything else, read this 1-page concept of the standard [here](https://crabenv.pages.dev/#concepts). Must know **Local (`.env`)**, **Schema (`env.ts`, etc.)**, **Template (`.env.example`)**

1. This command auto-detects your project and creates a Local, Schema, and Template

```sh
crabenv init
```

2. Add an env var

```sh
crabenv add
◆ Variable name: DATABASE_URL
◆ Scope: private
◆ Type: string
◆ Example Value: $(pwd)/data.db
◆ Mark as optional: no
◆ Add? yes

# Notice that Local, Template, and Schema are correctly synced 🎉
```

3. List your env vars to see if it's used, remove, or update vars

```sh
crabenv ls
crabenv remove
crabenv update
```

😎 That's it! Now imagine the convenience when...

- A new dev onboards on a new project, they just need to do `crabenv cp` to get a correct `.env` file! It's a better `cp .env.example .env` command!
- A senior dev wants to deploy a monorepo so what env vars needed for specific apps? Show them all with `crabenv ls`
- A senior dev wants to reorganize, sort, standardize the structure of env vars? Re-sort them without thinking about it with `crabenv fmt`
- A senior dev wants to check for any drift? `crabenv doctor`

## 📁 Languages supported

Regardless of the language or mix of languages in your repositories, you'll be able to use the same commands.

- [x] 💙 TypeScript/Javascript and Monorepos (includes React, Solid, Vue, Svelte, Vite, NextJS, ReactNative, Backends, and Cloudflare apps)
- [x] 🐍 Python
- [x] 🦀 Rust
- [x] 🐦 Flutter
- [x] More? Request an adapter.

<!--### 🤒 Pains solved:

- [x] CRUD and Documentation drift
- [ ] Deployment/sink drift via explicit managed blocks, not arbitrary file inference
- [x] `.env.example` and `env.ts` drift
- [x] Validation
- [x] Client and Server Boundaries
- [x] Local Development (creating the first .env) to Actual Deployment (translating that into the env on deployment) stories
- [x] A better `cp .env.example .env` command (This is not enough!) Creating envs (local env or for new dev,staging,prod envs).
  - [x] Use templating patterns like `"RSA_KEY=$(openssl  rand -base64 32)` - the crabenv copy
- [x] ~Rotating??~ Kinda impossible actually.
- [ ] Cloudflare `.dev.vars` guidance/docs
-->

## Agent skill

Completely optional, but in case you want your agent to be autonomous when adding new env vars... Install the crabenv skill for coding agents with:

```sh
npx skills add blankeos/crabenv
```

## Useful commands you should know

```sh
crabenv init
crabenv copy         # or crabenv cp. (It's a better `cp .env.example .env` command)
crabenv doctor       # It's a checklist of common mistakes
crabenv doctor --fix

# CRUD
crabenv list   # Lists variables
crabenv add    # Wizard-like experience
crabenv update # Wizard-like experience
crabenv remove # Wizard-like experience

crabenv add {VARIABLE_NAME}
    --shared   # if monorepo, adds it to all apps (also --shared '*')
    --shared apps/api apps/web # add it to selected apps
    --example  # optional, for .env.example (you can use templating w/ "$(pwd)/data.db")
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

## Documentation

https://crabenv.pages.dev

## License

MIT. Fork it if you want!
