use crate::models::locality::Locality;
use crate::utils::file::FileError;
use sqlx::SqlitePool;
use std::path::Path;
use thiserror::Error;

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
    #[error("SQLx error: {0}")]
    SqlxError(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("File error: {0}")]
    FileError(#[from] FileError),
    #[error("Command error: {0}")]
    CmdError(#[from] crate::utils::cmd::CmdError),
}

pub struct DatabaseService {
    pool: SqlitePool,
    database_path: String,
    whosonfirst_db_url: String,
    bzip2_cmd: String,
}

impl DatabaseService {
    pub async fn new(
        database_url: &str,
        database_path: &str,
        whosonfirst_db_url: &str,
        bzip2_cmd: &str,
    ) -> Result<Self, DatabaseError> {
        let pool = SqlitePool::connect(database_url).await?;

        let service = Self {
            pool,
            database_path: database_path.to_string(),
            whosonfirst_db_url: whosonfirst_db_url.to_string(),
            bzip2_cmd: bzip2_cmd.to_string(),
        };

        service.create_optimized_indexes().await?;

        Ok(service)
    }

    async fn create_optimized_indexes(&self) -> Result<(), DatabaseError> {
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

        sqlx::query(create_countries_index)
            .execute(&self.pool)
            .await?;
        sqlx::query(create_country_count_index)
            .execute(&self.pool)
            .await?;
        sqlx::query(create_pagination_index)
            .execute(&self.pool)
            .await?;
        sqlx::query(create_search_index).execute(&self.pool).await?;
        sqlx::query(create_count_index).execute(&self.pool).await?;
        sqlx::query(create_search_count_index)
            .execute(&self.pool)
            .await?;

        Ok(())
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
            println!("Downloading WhosOnFirst database...");

            crate::utils::file::download_file_with_progress(
                &self.whosonfirst_db_url,
                compressed_path,
            )
            .await?;
            println!("Database download completed!");
        }

        Ok(())
    }

    pub async fn decompress_database(&self) -> Result<(), DatabaseError> {
        let compressed_path = format!("{}.bz2", self.database_path);
        let compressed_path = Path::new(&compressed_path);
        let database_path = Path::new(&self.database_path);

        if compressed_path.exists() && !database_path.exists() {
            println!("Decompressing database...");

            let output = crate::utils::cmd::run_command(
                &self.bzip2_cmd,
                &["-dv", &compressed_path.to_string_lossy()],
                None,
            )
            .await?;

            if !output.stderr.is_empty() {
                eprintln!("Decompression output: {}", output.stderr);
            }

            println!("Database decompressed successfully!");
        }

        Ok(())
    }

    pub async fn get_localities_count(
        &self,
        country_code: &str,
        query: Option<&str>,
    ) -> Result<u32, DatabaseError> {
        let conditions = [
            "placetype = 'locality'",
            "is_current = 1",
            "is_deprecated = 0",
            "country = ?",
        ];

        let where_clause = conditions.join(" AND ");
        let query_str = format!("SELECT COUNT(*) as count FROM spr WHERE {}", where_clause);

        let (count,) = if let Some(q) = query {
            let search_query = format!(
                "SELECT COUNT(*) as count FROM spr WHERE {} AND name LIKE ? COLLATE NOCASE",
                where_clause
            );
            let search_param = format!("{}%", q);
            sqlx::query_as::<_, (i64,)>(&search_query)
                .bind(country_code)
                .bind(&search_param)
                .fetch_one(&self.pool)
                .await?
        } else {
            sqlx::query_as::<_, (i64,)>(&query_str)
                .bind(country_code)
                .fetch_one(&self.pool)
                .await?
        };

        Ok(count as u32)
    }

    pub async fn get_localities(
        &self,
        country_code: &str,
        page: u32,
        limit: u32,
        query: Option<&str>,
    ) -> Result<Vec<Locality>, DatabaseError> {
        let offset = (page - 1) * limit;

        let conditions = [
            "placetype = 'locality'",
            "is_current = 1",
            "is_deprecated = 0",
            "country = ?",
        ];

        let where_clause = conditions.join(" AND ");

        let localities = if let Some(q) = query {
            let search_query = format!(
                "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} AND name LIKE ? COLLATE NOCASE ORDER BY name COLLATE NOCASE ASC LIMIT ? OFFSET ?",
                where_clause
            );
            let search_param = format!("{}%", q);
            sqlx::query_as::<_, Locality>(&search_query)
                .bind(country_code)
                .bind(&search_param)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
        } else {
            let paginated_query = format!(
                "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} ORDER BY name ASC LIMIT ? OFFSET ?",
                where_clause
            );
            sqlx::query_as::<_, Locality>(&paginated_query)
                .bind(country_code)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
        };

        Ok(localities)
    }

    pub async fn get_country_localities(
        &self,
        country_code: &str,
    ) -> Result<Vec<Locality>, DatabaseError> {
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
            "country = ?",
        ];

        let where_clause = conditions.join(" AND ");
        let query_str = format!(
            "SELECT id, name, country, placetype, latitude, longitude, min_longitude, min_latitude, max_longitude, max_latitude FROM spr WHERE {} ORDER BY id",
            where_clause
        );

        let localities = sqlx::query_as::<_, Locality>(&query_str)
            .bind(country_code)
            .fetch_all(&self.pool)
            .await?;

        Ok(localities)
    }

    pub async fn get_country_locality_count(
        &self,
        country_code: &str,
    ) -> Result<u32, DatabaseError> {
        let conditions = [
            "placetype = 'locality'",
            "is_current = 1",
            "is_deprecated = 0",
            "country = ?",
        ];

        let where_clause = conditions.join(" AND ");
        let query_str = format!("SELECT COUNT(*) as count FROM spr WHERE {}", where_clause);

        let (count,) = sqlx::query_as::<_, (i64,)>(&query_str)
            .bind(country_code)
            .fetch_one(&self.pool)
            .await?;

        Ok(count as u32)
    }

    pub async fn get_countries_locality_counts(
        &self,
        country_codes: &[String],
    ) -> Result<std::collections::HashMap<String, u32>, DatabaseError> {
        if country_codes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

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

        let mut query = sqlx::query_as::<_, (String, i64)>(&query_str);

        for country_code in country_codes {
            query = query.bind(country_code);
        }

        let results = query.fetch_all(&self.pool).await?;

        let mut counts = std::collections::HashMap::new();
        for (country, count) in results {
            counts.insert(country, count as u32);
        }

        Ok(counts)
    }
}
