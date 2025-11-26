use codex_bindings::{CodexConfig, LogLevel};
use dotenvy::dotenv;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct LocalitySrvConfig {
    // === Core Codex Configuration ===
    pub codex: CodexConfig,

    // === Storage Configuration ===
    pub data_dir: PathBuf, // Directory for Codex data

    // === Localitysrv Specific ===
    pub database_path: PathBuf,     // WhosOnFirst database path
    pub cid_database_path: PathBuf, // CID mappings database path
    pub localities_dir: PathBuf,    // Local PMTiles directory

    // === Tool Configuration ===
    pub pmtiles_cmd: String,
    pub bzip2_cmd: String,
    pub find_cmd: String,
    pub whosonfirst_db_url: String,
    pub planet_pmtiles_path: Option<String>,
    pub target_countries: Vec<String>,
    pub max_concurrent_extractions: usize,
}

impl LocalitySrvConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenv().ok();

        // Core Codex settings
        let codex_data_dir =
            env::var("CODEX_DATA_DIR").unwrap_or_else(|_| "./.codex-data".to_string());
        let storage_quota_gb = env::var("CODEX_STORAGE_QUOTA_GB")
            .unwrap_or_else(|_| "100".to_string())
            .parse::<u64>()
            .unwrap_or(100);
        let storage_quota = storage_quota_gb * 1024 * 1024 * 1024; // Convert GB to bytes

        let discovery_port = env::var("CODEX_DISCOVERY_PORT")
            .unwrap_or_else(|_| "8090".to_string())
            .parse::<u16>()
            .unwrap_or(8090);

        let listen_addrs: Vec<String> = env::var("CODEX_LISTEN_ADDRS")
            .unwrap_or_else(|_| {
                format!(
                    "/ip4/0.0.0.0/tcp/{},/ip4/127.0.0.1/tcp/{}",
                    discovery_port, discovery_port
                )
            })
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let log_level_str = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
        let log_level = match log_level_str.as_str() {
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        };

        // Localitysrv settings
        let assets_dir = env::var("ASSETS_DIR").unwrap_or_else(|_| "./assets".to_string());
        let database_path = env::var("LOCALITYSRV_DB_PATH")
            .unwrap_or_else(|_| format!("{}/whosonfirst-data-admin-latest.db", assets_dir));
        let cid_database_path = env::var("LOCALITYSRV_CID_DB_PATH")
            .unwrap_or_else(|_| format!("{}/locality-cid-mappings.db", assets_dir));
        let localities_dir = env::var("LOCALITYSRV_LOCALITIES_DIR")
            .unwrap_or_else(|_| format!("{}/localities", assets_dir));

        // Create Codex configuration
        let codex_config = CodexConfig::new()
            .log_level(log_level)
            .data_dir(&codex_data_dir)
            .storage_quota(storage_quota)
            .discovery_port(discovery_port)
            .listen_addrs(listen_addrs.clone());

        Ok(Self {
            codex: codex_config,
            data_dir: PathBuf::from(codex_data_dir),
            database_path: PathBuf::from(database_path),
            cid_database_path: PathBuf::from(cid_database_path),
            localities_dir: PathBuf::from(localities_dir),

            // Tool configuration (keep existing)
            pmtiles_cmd: env::var("PMTILES_CMD").unwrap_or_else(|_| "pmtiles".to_string()),
            bzip2_cmd: env::var("BZIP2_CMD").unwrap_or_else(|_| "bzip2".to_string()),
            find_cmd: env::var("FIND_CMD").unwrap_or_else(|_| "find".to_string()),
            whosonfirst_db_url: env::var("WHOSEONFIRST_DB_URL").unwrap_or_else(|_| {
                "https://data.geocode.earth/wof/dist/sqlite/whosonfirst-data-admin-latest.db.bz2"
                    .to_string()
            }),
            planet_pmtiles_path: env::var("PLANET_PMTILES_PATH")
                .ok()
                .filter(|s| !s.is_empty()),
            target_countries: env::var("TARGET_COUNTRIES")
                .unwrap_or_else(|_| "".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            max_concurrent_extractions: env::var("MAX_CONCURRENT_EXTRACTIONS")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(10),
        })
    }

    pub fn country_codes_path(&self) -> PathBuf {
        self.database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("country-codes.json")
    }
}
