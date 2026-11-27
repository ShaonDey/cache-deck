//! Saved connections and preferences, kept in a small JSON file.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ServerKind {
    #[default]
    Auto,
    Redis,
    Valkey,
}

impl ServerKind {
    pub fn label(self) -> &'static str {
        match self {
            ServerKind::Auto => "Auto-detect",
            ServerKind::Redis => "Redis",
            ServerKind::Valkey => "Valkey",
        }
    }
}

/// Which environment a saved server belongs to (shown as a coloured tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Env {
    Production,
    Staging,
    Development,
    Local,
}

impl Env {
    pub const ALL: [Env; 4] = [Env::Production, Env::Staging, Env::Development, Env::Local];
    pub fn tag(self) -> &'static str {
        match self {
            Env::Production => "PROD",
            Env::Staging => "STAGING",
            Env::Development => "DEV",
            Env::Local => "LOCAL",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Env::Production => "Production",
            Env::Staging => "Staging",
            Env::Development => "Development",
            Env::Local => "Local",
        }
    }
}

/// The longest connection timeout a connection may ask for, in seconds.
pub const TIMEOUT_MAX_SECS: u32 = 120;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnCfg {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub db: i64,
    pub tls: bool,
    /// With TLS on: don't compare the certificate's name with `host`. For tunnels and port
    /// forwards, where `host` is 127.0.0.1 but the certificate names the real server. The
    /// certificate must still be signed by a trusted authority.
    pub tls_skip_host_check: bool,
    /// Seconds to wait for the server when connecting (1 to `TIMEOUT_MAX_SECS`).
    pub timeout_secs: u32,
    pub kind: ServerKind,
    pub version: String,
    pub env: Option<Env>,
    /// Seconds since 1970 when this connection was last opened.
    pub last_used: Option<u64>,
}

impl Default for ConnCfg {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            host: "127.0.0.1".into(),
            port: 6379,
            username: String::new(),
            password: String::new(),
            db: 0,
            tls: false,
            tls_skip_host_check: false,
            timeout_secs: 10,
            kind: ServerKind::Auto,
            version: String::new(),
            env: None,
            last_used: None,
        }
    }
}

impl ConnCfg {
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub connections: Vec<ConnCfg>,
    pub dark: bool,
    pub next_id: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            connections: Vec::new(),
            dark: true,
            next_id: 1,
        }
    }
}

fn path() -> Option<PathBuf> {
    // A different settings file can be used for testing.
    if let Some(p) = std::env::var_os("CACHE_DECK_CONFIG") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("CacheDeck").join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = path().ok_or("No settings folder on this computer")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, text).map_err(|e| e.to_string())?;
        // The file holds passwords: only this user may read it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
