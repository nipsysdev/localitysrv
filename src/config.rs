use dotenvy::dotenv;
use std::env;
use std::path::PathBuf;

pub struct Config {
    pub server_port: u16,
    pub assets_dir: String,
    pub pmtiles_cmd: String,
    pub bzip2_cmd: String,
    pub find_cmd: String,
    pub whosonfirst_db_url: String,
    pub protomaps_builds_url: String,
    pub target_countries: Vec<String>,
    pub max_concurrent_extractions: usize,
    pub db_connection_pool_size: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        dotenv().ok();

        Ok(Self {
            server_port: env::var("SERVER_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            assets_dir: env::var("ASSETS_DIR").unwrap_or_else(|_| "./assets".to_string()),
            pmtiles_cmd: env::var("PMTILES_CMD").unwrap_or_else(|_| "pmtiles".to_string()),
            bzip2_cmd: env::var("BZIP2_CMD").unwrap_or_else(|_| "bzip2".to_string()),
            find_cmd: env::var("FIND_CMD").unwrap_or_else(|_| "find".to_string()),
            whosonfirst_db_url: env::var("WHOSEONFIRST_DB_URL").unwrap_or_else(|_| {
                "https://data.geocode.earth/wof/dist/sqlite/whosonfirst-data-admin-latest.db.bz2"
                    .to_string()
            }),
            protomaps_builds_url: env::var("PROTOMAPS_BUILDS_URL")
                .unwrap_or_else(|_| "https://build-metadata.protomaps.dev/builds.json".to_string()),
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
            db_connection_pool_size: env::var("DB_CONNECTION_POOL_SIZE")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(10),
        })
    }

    pub fn database_path(&self) -> PathBuf {
        PathBuf::from(&self.assets_dir).join("whosonfirst-data-admin-latest.db")
    }

    pub fn database_url(&self) -> String {
        format!("sqlite://{}", self.database_path().display())
    }

    pub fn country_codes_path(&self) -> PathBuf {
        PathBuf::from(&self.assets_dir).join("country-codes.json")
    }

    pub fn localities_dir(&self) -> PathBuf {
        PathBuf::from(&self.assets_dir).join("localities")
    }
}
