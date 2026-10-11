use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Clone, Debug)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
pub struct TokenManager {
    cache: Arc<RwLock<Option<CachedToken>>>,
    ttl: Duration,
    project_id: Arc<RwLock<String>>,
}

impl Default for TokenManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenManager {
    pub fn new() -> Self {
        let initial_project =
            Self::read_project_id_from_db().unwrap_or_else(|| "charming-craft-95w3k".to_string());
        info!("Antigravity Project ID: {}", initial_project);

        Self {
            cache: Arc::new(RwLock::new(None)),
            ttl: Duration::from_secs(50 * 60), // 50 minutes (Google tokens expire after 60m)
            project_id: Arc::new(RwLock::new(initial_project)),
        }
    }

    pub async fn get_token(&self) -> Result<String, anyhow::Error> {
        // 1. Read lock check
        {
            let guard = self.cache.read().await;
            if let Some(cached) = guard.as_ref() {
                if Instant::now() < cached.expires_at {
                    return Ok(cached.token.clone());
                }
            }
        }

        // 2. Write lock fetch
        let mut guard = self.cache.write().await;
        if let Some(cached) = guard.as_ref() {
            if Instant::now() < cached.expires_at {
                return Ok(cached.token.clone());
            }
        }

        info!("Fetching fresh Google Antigravity token via omp...");
        let token = Self::fetch_token_from_omp().await?;

        // Refresh project ID if available
        if let Some(p) = Self::read_project_id_from_db() {
            let mut proj_guard = self.project_id.write().await;
            *proj_guard = p;
        }

        let cached = CachedToken {
            token: token.clone(),
            expires_at: Instant::now() + self.ttl,
        };
        *guard = Some(cached);
        info!("Token successfully refreshed and cached (TTL: 50m)");

        Ok(token)
    }

    pub async fn get_project_id(&self) -> String {
        self.project_id.read().await.clone()
    }

    pub async fn invalidate(&self) {
        warn!("Invalidating cached Antigravity token due to upstream auth error");
        let mut guard = self.cache.write().await;
        *guard = None;
    }

    fn read_project_id_from_db() -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let db_path = format!("{}/.omp/agent/agent.db", home);
        if !std::path::Path::new(&db_path).exists() {
            return None;
        }

        // Run sqlite3 CLI to inspect data column in auth_credentials
        let output = std::process::Command::new("sqlite3")
            .arg(&db_path)
            .arg("SELECT data FROM auth_credentials WHERE provider = 'google-antigravity' AND data LIKE '%projectId%' ORDER BY id DESC LIMIT 1;")
            .output()
            .ok()?;

        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(pid) = v.get("projectId").and_then(|p| p.as_str()) {
                    return Some(pid.to_string());
                }
            }
        }
        None
    }

    async fn fetch_token_from_omp() -> Result<String, anyhow::Error> {
        let omp_bin = std::env::var("OMP_BIN").unwrap_or_else(|_| "omp".to_string());
        let output = tokio::process::Command::new(&omp_bin)
            .args(["token", "google-antigravity"])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("omp token command failed: {}", stderr);
            return Err(anyhow::anyhow!("omp token command failed: {}", stderr));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(anyhow::anyhow!("omp returned an empty token"));
        }

        Ok(token)
    }
}
