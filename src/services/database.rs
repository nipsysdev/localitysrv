use crate::models::locality::Locality;
use crate::utils::file::FileError;
use rusqlite::Connection;
use tracing::info;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Query failed: {0}")]
    QueryFailed(String),
    #[error("Database download failed: {0}")]
    DownloadFailed(String),
    #[error("Database decompression failed: {0}")]
    DecompressionFailed(String),
    #[error("Rusqlite error: {0}")]
    RusqliteError(#[from] rusqlite::Error),
    #[error("Tokio join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("File error: {0}")]
    FileError(#[from] FileError),
    #[error("Command error: {0}")]
    CmdError(#[from] crate::utils::cmd::CmdError),
}

pub struct DatabaseService {
    conn: Arc<Mutex<Connection>>,
    database_path: String,
    whosonfirst_db_url: String,
    bzip2_cmd: String,
}

impl DatabaseService {
    pub async fn new(
        _database_url: &str,
        database_path: &str,
        whosonfirst_db_url: &str,
        bzip2_cmd: &str,
    ) -> Result<Self, DatabaseError> {
        let conn = Connection::open(database_path)?;

        let service = Self {
            conn: Arc::new(Mutex::new(conn)),
            database_path: database_path.to_string(),
            whosonfirst_db_url: whosonfirst_db_url.to_string(),
            bzip2_cmd: bzip2_cmd.to_string(),
        };

        service.create_optimized_indexes().await?;

        Ok(service)
    }

    async fn create_optimized_indexes(&self) -> Result<(), DatabaseError> {
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            // Index for countries query
            let create_countries_index = r#"
            CREATE INDEX IF NOT EXISTS spr_countries_query_idx
            ON spr (placetype, is_current, is_deprecated, country)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            // Index for country count query
            let create_country_count_index = r#"
            CREATE INDEX IF NOT EXISTS spr_country_count_query_idx
            ON spr (placetype, is_current, is_deprecated, country)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            // Index for localities pagination queries
            let create_pagination_index = r#"
            CREATE INDEX IF NOT EXISTS spr_localities_pagination_idx
            ON spr (placetype, is_current, is_deprecated, country, name)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            // Index for localities search queries (case-insensitive)
            let create_search_index = r#"
            CREATE INDEX IF NOT EXISTS spr_localities_search_idx
            ON spr (placetype, is_current, is_deprecated, country, name COLLATE NOCASE)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            // Index for localities count queries
            let create_count_index = r#"
            CREATE INDEX IF NOT EXISTS spr_localities_count_idx
            ON spr (placetype, is_current, is_deprecated, country)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            // Index for localities search count queries (case-insensitive)
            let create_search_count_index = r#"
            CREATE INDEX IF NOT EXISTS spr_localities_search_count_idx
            ON spr (placetype, is_current, is_deprecated, country, name COLLATE NOCASE)
            WHERE placetype = 'locality' AND is_current = 1 AND is_deprecated = 0
            "#;

            conn.execute(create_countries_index, [])?;
            conn.execute(create_country_count_index, [])?;
            conn.execute(create_pagination_index, [])?;
            conn.execute(create_search_index, [])?;
            conn.execute(create_count_index, [])?;
            conn.execute(create_search_count_index, [])?;

            Ok::<(), DatabaseError>(())
        })
        .await?
    }

    pub async fn ensure_database_present(&self) -> Result<(), DatabaseError> {
        let path = Path::new(&self.database_path);

        if !path.exists() {
            self.download_database().await?;
            self.decompress_database().await?;
        }

        Ok(())
    }

    pub async fn download_database(&self) -> Result<(), DatabaseError> {
        let compressed_path = format!("{}.bz2", self.database_path);
        let compressed_path = Path::new(&compressed_path);

        if !compressed_path.exists() {
            info!("Downloading WhosOnFirst database...");

            crate::utils::file::download_file_with_progress(
                &self.whosonfirst_db_url,
                compressed_path,
            )
            .await?;
            info!("Database download completed!");
        }

        Ok(())
    }

    pub async fn decompress_database(&self) -> Result<(), DatabaseError> {
        let compressed_path = format!("{}.bz2", self.database_path);
        let compressed_path = Path::new(&compressed_path);
        let database_path = Path::new(&self.database_path);

        if compressed_path.exists() && !database_path.exists() {
            info!("Decompressing database...");

            let output = crate::utils::cmd::run_command(
                &self.bzip2_cmd,
                &["-dv", &compressed_path.to_string_lossy()],
                None,
            )
            .await?;

            if !output.stderr.is_empty() {
                tracing::error!("Decompression output: {}", output.stderr);
            }

            info!("Database decompressed successfully!");
        }

        Ok(())
    }

    pub async fn get_localities_count(
        &self,
        country_code: &str,
        query: Option<&str>,
    ) -> Result<u32, DatabaseError> {
        let conn = self.conn.clone();
        let country_code = country_code.to_string();
        let query_param = query.map(|q| format!("{}%", q));

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            let conditions = [
                "placetype = 'locality'",
                "is_current = 1",
                "is_deprecated = 0",
                "country = ?1",
            ];

            let where_clause = conditions.join(" AND ");

            let count = if let Some(q) = query_param {
                let search_query = format!(
                    "SELECT COUNT(*) as count FROM spr WHERE {} AND name LIKE ?2 COLLATE NOCASE",
                    where_clause
                );
                conn.query_row(&search_query, [&country_code, &q], |row| {
                    row.get::<_, i64>(0)
                })
            } else {
                let query_str = format!("SELECT COUNT(*) as count FROM spr WHERE {}", where_clause);
                conn.query_row(&query_str, [&country_code], |row| row.get::<_, i64>(0))
            };

            Ok(count.map(|c| c as u32)?)
        })
        .await?
    }

    pub async fn get_localities(
        &self,
        country_code: &str,
        page: u32,
        limit: u32,
        query: Option<&str>,
    ) -> Result<Vec<Locality>, DatabaseError> {
        let conn = self.conn.clone();
        let country_code = country_code.to_string();
        let query_param = query.map(|q| format!("{}%", q));
        let offset = (page - 1) * limit;

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            
            let conditions = [
                "placetype = 'locality'",
                "is_current = 1",
                "is_deprecated = 0",
                "country = ?1",
            ];

            let where_clause = conditions.join(" AND ");

            let localities = if let Some(q) = query_param {
                let search_query = format!(
                    "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} AND name LIKE ?2 COLLATE NOCASE ORDER BY name COLLATE NOCASE ASC LIMIT ?3 OFFSET ?4",
                    where_clause
                );
                let mut stmt = conn.prepare(&search_query)?;
                let rows = stmt.query_map([&country_code, &q, &limit.to_string(), &offset.to_string()], |row| {
                    Locality::from_row(row)
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            } else {
                let paginated_query = format!(
                    "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} ORDER BY name ASC LIMIT ?2 OFFSET ?3",
                    where_clause
                );
                let mut stmt = conn.prepare(&paginated_query)?;
                let rows = stmt.query_map([&country_code, &limit.to_string(), &offset.to_string()], |row| {
                    Locality::from_row(row)
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            Ok(localities)
        }).await?
    }

    pub async fn get_country_localities(
        &self,
        country_code: &str,
    ) -> Result<Vec<Locality>, DatabaseError> {
        let conn = self.conn.clone();
        let country_code = country_code.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            
            let conditions = [
                "placetype = 'locality'",
                "is_current = 1",
                "is_deprecated = 0",
                "name IS NOT NULL",
                "name != ''",
                "latitude IS NOT NULL",
                "longitude IS NOT NULL",
                "min_longitude IS NOT NULL",
                "min_latitude IS NOT NULL",
                "max_longitude IS NOT NULL",
                "max_latitude IS NOT NULL",
                "country = ?1",
            ];

            let where_clause = conditions.join(" AND ");
            let query_str = format!(
                "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} ORDER BY id",
                where_clause
            );

            let mut stmt = conn.prepare(&query_str)?;
            let rows = stmt.query_map([&country_code], |row| {
                Locality::from_row(row)
            })?;
            
            let localities = rows.collect::<Result<Vec<_>, _>>()?;
            Ok(localities)
        }).await?
    }

    pub async fn get_country_locality_count(
        &self,
        country_code: &str,
    ) -> Result<u32, DatabaseError> {
        let conn = self.conn.clone();
        let country_code = country_code.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            
            let conditions = [
                "placetype = 'locality'",
                "is_current = 1",
                "is_deprecated = 0",
                "country = ?1",
            ];

            let where_clause = conditions.join(" AND ");
            let query_str = format!("SELECT COUNT(*) as count FROM spr WHERE {}", where_clause);

            let count = conn.query_row(&query_str, [&country_code], |row| row.get::<_, i64>(0))?;
            Ok(count as u32)
        }).await?
    }

    pub async fn get_countries_locality_counts(
        &self,
        country_codes: &[String],
    ) -> Result<std::collections::HashMap<String, u32>, DatabaseError> {
        if country_codes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let conn = self.conn.clone();
        let country_codes = country_codes.to_vec();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            
            let conditions = [
                "placetype = 'locality'",
                "is_current = 1",
                "is_deprecated = 0",
            ];

            let base_where_clause = conditions.join(" AND ");

            let placeholders: Vec<_> = country_codes.iter().map(|_| "?").collect();
            let in_clause = format!("country IN ({})", placeholders.join(","));

            let where_clause = format!("{} AND {}", base_where_clause, in_clause);
            let query_str = format!(
                "SELECT country, COUNT(*) as count FROM spr WHERE {} GROUP BY country",
                where_clause
            );

            let mut stmt = conn.prepare(&query_str)?;
            
            // Create parameter values
            let params: Vec<&dyn rusqlite::ToSql> = country_codes
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            
            let mut counts = std::collections::HashMap::new();
            for row in rows {
                let (country, count) = row?;
                counts.insert(country, count as u32);
            }

            Ok(counts)
        }).await?
    }
}
