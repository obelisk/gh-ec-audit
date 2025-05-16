use std::{fs::File, io::Read};

use serde::Deserialize;
use spotcheck::Bootstrap;

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub token: String,
    pub organization: String,
}

impl Into<Bootstrap> for &Configuration {
    fn into(self) -> Bootstrap {
        Bootstrap {
            token: self.token.clone(),
            org: self.organization.clone(),
        }
    }
}

/// Get the configuration for GHVHS.
/// If the path is not provided, we will check the current directory
/// If there is no configuration file either passed or in the current directory,
/// we will read it from the ENV variable GHVHS_CONFIG
/// If there is no ENV variable either we will return an error.
pub fn get_configuration(path: Option<String>) -> Result<Configuration, String> {
    let path = path.unwrap_or_else(|| "configuration.yaml".to_string());
    let mut file = File::open(&path).map_err(|e| format!("Failed to open config file: {}", e))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|e| format!("Failed to read config file: {}", e))?;
    let config: Configuration =
        toml::from_str(&buf).map_err(|e| format!("Failed to parse config file: {}", e))?;
    Ok(config)
}
