mod config;

use config::Settings;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load()?;
    println!("basic-rust listening on {}", settings.bind_addr);
    Ok(())
}
