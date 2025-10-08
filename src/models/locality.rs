use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Locality {
    pub id: i64,
    pub name: String,
    pub country: String,
    pub placetype: String,
    pub latitude: f64,
    pub longitude: f64,
    pub min_longitude: f64,
    pub min_latitude: f64,
    pub max_longitude: f64,
    pub max_latitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalityInfo {
    pub id: i64,
    pub name: String,
    pub country: String,
    pub placetype: String,
    pub latitude: f64,
    pub longitude: f64,
    pub min_longitude: f64,
    pub min_latitude: f64,
    pub max_longitude: f64,
    pub max_latitude: f64,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedLocalitiesResult {
    pub localities: Vec<LocalityInfo>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationInfo {
    pub page: u32,
    pub limit: u32,
    pub total: u32,
    pub total_pages: u32,
}
