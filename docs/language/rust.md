# Rust

Dependencies: `figment`, `serde`, `dotenvy`; monorepo justfile examples use `dotenv-cli`

```sh
# single app
- .env
- .env.example
- src/
    - config.rs
```

```sh
# monorepo (Cargo workspace)
- .env
- .env.example
- apps/
    - api/
        - .env.example
        - src/config.rs
```

Crabenv should discover `src/config.rs` for Rust apps. Env var names are explicit serde aliases/renames (typically `SCREAMING_SNAKE`). Server apps only — no client/public split.

## Example

`src/config.rs`

```rust
use figment::{providers::{Env, Format, Toml}, Figment};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "DATABASE_URL")]
    pub database_url: String,

    #[serde(rename = "SECRET_KEY")]
    pub secret_key: String,

    #[serde(default = "default_log_level", rename = "LOG_LEVEL")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Settings {
    pub fn load() -> Result<Self, figment::Error> {
        dotenvy::dotenv().ok();

        Figment::new()
            .merge(Toml::file("Config.toml").nested())
            .merge(Env::raw())
            .extract()
    }
}
```

`main.rs`

```rust
mod config;

use config::Settings;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load()?;
    println!("{}", settings.database_url);
    Ok(())
}
```

`justfile` (or a similar script runner)

```just
dev *args:
    cargo run -- {{ args }}
```

Monorepo local run (root `.env` → process env):

```just
_env *cmd:
    dotenv -e ../../.env -- {{ cmd }}

dev *args:
    just _env "cargo run -- {{ args }}"
```

Use `Settings::load()` and typed fields in code — not direct `std::env::var` reads. Docs/defaults live on the config struct; keep `.env.example` keys and serde aliases aligned.
