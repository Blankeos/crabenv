mod config;

use config::Settings;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load()?;
    println!("rust-worker listening on {}", settings.bind_addr);
    Ok(())
}
