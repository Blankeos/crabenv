use figment::{providers::{Env, Format, Toml}, Figment};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "RUST_WORKER_DATABASE_URL")]
    pub database_url: String,

    #[serde(rename = "RUST_WORKER_SECRET_KEY")]
    pub secret_key: String,

    #[serde(default = "default_log_level", rename = "RUST_WORKER_LOG_LEVEL")]
    pub log_level: String,

    #[serde(rename = "RUST_WORKER_BIND_ADDR")]
    pub bind_addr: String,
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
