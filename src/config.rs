use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_upstream")]
    pub upstream_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            upstream_url: default_upstream(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default = "default_primary_model")]
    pub primary: String,
    #[serde(default = "default_fallbacks")]
    pub fallbacks: Vec<String>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            primary: default_primary_model(),
            fallbacks: default_fallbacks(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8088
}

fn default_upstream() -> String {
    "https://daily-cloudcode-pa.googleapis.com".to_string()
}

fn default_primary_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_fallbacks() -> Vec<String> {
    vec![
        "gemini-3.8-flash".to_string(),
        "gemini-3.5-flash".to_string(),
        "gemini-2.5-flash".to_string(),
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut aliases = HashMap::new();
        aliases.insert(
            "gemini-3.8-flash".to_string(),
            "gemini-3.8-flash-tiered".to_string(),
        );
        aliases.insert(
            "gemini-3.6-flash".to_string(),
            "gemini-3.6-flash-medium".to_string(),
        );
        aliases.insert(
            "gemini-3.5-flash".to_string(),
            "gemini-3.5-flash-extra-low".to_string(),
        );
        aliases.insert(
            "gemini-3.1-pro".to_string(),
            "gemini-3.1-pro-low".to_string(),
        );

        Self {
            server: ServerConfig::default(),
            routing: RoutingConfig::default(),
            aliases,
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: Option<&Path>) -> Self {
        if let Some(p) = path {
            if p.exists() {
                match std::fs::read_to_string(p) {
                    Ok(content) => match toml::from_str::<AppConfig>(&content) {
                        Ok(mut cfg) => {
                            let default_cfg = Self::default();
                            for (k, v) in default_cfg.aliases {
                                cfg.aliases.entry(k).or_insert(v);
                            }
                            return cfg;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse config at {:?}: {}, using defaults",
                                p,
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!("Failed to read config at {:?}: {}, using defaults", p, e);
                    }
                }
            }
        }
        Self::default()
    }

    pub fn resolve_model(&self, model: &str) -> String {
        self.aliases
            .get(model)
            .cloned()
            .unwrap_or_else(|| model.to_string())
    }

    pub fn candidate_chain(&self, requested_model: Option<&str>) -> Vec<String> {
        let mut chain = Vec::new();
        match requested_model {
            None => {
                chain.push(self.routing.primary.clone());
                for fb in &self.routing.fallbacks {
                    if !chain.contains(fb) {
                        chain.push(fb.clone());
                    }
                }
            }
            Some(m) if m.is_empty() || m == self.routing.primary => {
                chain.push(self.routing.primary.clone());
                for fb in &self.routing.fallbacks {
                    if !chain.contains(fb) {
                        chain.push(fb.clone());
                    }
                }
            }
            Some(specific) => {
                chain.push(specific.to_string());
                if !specific.starts_with("tab_") {
                    if !chain.contains(&self.routing.primary) {
                        chain.push(self.routing.primary.clone());
                    }
                    for fb in &self.routing.fallbacks {
                        if !chain.contains(fb) {
                            chain.push(fb.clone());
                        }
                    }
                }
            }
        }
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_chain_default() {
        let cfg = AppConfig::default();
        let chain = cfg.candidate_chain(None);
        assert_eq!(chain[0], "claude-sonnet-4-6");
        assert!(chain.contains(&"gemini-3.8-flash".to_string()));
    }

    #[test]
    fn test_resolve_model() {
        let cfg = AppConfig::default();
        assert_eq!(
            cfg.resolve_model("gemini-3.8-flash"),
            "gemini-3.8-flash-tiered"
        );
        assert_eq!(cfg.resolve_model("claude-sonnet-4-6"), "claude-sonnet-4-6");
    }
}
