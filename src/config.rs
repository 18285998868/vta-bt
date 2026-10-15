use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: String,
    pub frontend_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3001,
            data_dir: String::new(),
            frontend_dir: format!("{}/frontend/dist", env!("CARGO_MANIFEST_DIR")),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("config/config.toml");
        let content = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&content)?;
        cfg.validate()
    }

    fn validate(self) -> anyhow::Result<Self> {
        if self.data_dir.trim().is_empty() {
            anyhow::bail!("data_dir must not be empty");
        }
        Ok(self)
    }
}
