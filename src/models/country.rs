use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryInfo {
    pub country_code: String,
    pub country_name: String,
    pub locality_count: u32,
}
