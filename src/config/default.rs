use serde::Deserialize;

fn default_host() -> String {
    "127.0.0.1".into()
}

fn default_port() -> u16 {
    3001
}

fn default_frontend_dir() -> String {
    format!("{}/frontend/dist", env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub data_dir: String,
    #[serde(default = "default_frontend_dir")]
    pub frontend_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3001,
            data_dir: String::new(),
            frontend_dir: default_frontend_dir(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("config/config.toml");
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}
