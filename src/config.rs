use std::{collections::HashMap, error::Error, net::Ipv6Addr, path::Path};

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_query_server")]
    pub query_server: String,

    pub services: HashMap<String, ServiceConfig>,
    pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct ServiceConfig {
    pub suffix: Ipv6Addr,
    pub name: String,
    pub fqdn: String,
    pub ttl: u32,
}

impl Config {
    pub fn load<P>(path: P) -> Result<Self, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        let config_raw = std::fs::read(path)?;
        Ok(toml::from_slice(&config_raw)?)
    }
}

// Default implementations
fn default_query_server() -> String {
    "https://ifconfig.co".to_string()
}
