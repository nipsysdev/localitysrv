use crate::utils::file::FileError;
use serde_json;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CountryError {
    #[error("Failed to load country codes: {0}")]
    LoadFailed(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("File error: {0}")]
    FileError(#[from] FileError),
}

pub struct CountryService {
    country_codes: HashMap<String, String>,
}

impl CountryService {
    pub async fn new(country_codes_path: &Path) -> Result<Self, CountryError> {
        let country_codes = if !country_codes_path.exists() {
            let default_codes = Self::create_default_country_codes();
            let json_content = serde_json::to_string_pretty(&default_codes)?;
            std::fs::write(country_codes_path, json_content)?;
            default_codes
        } else {
            let content = std::fs::read_to_string(country_codes_path)?;
            serde_json::from_str(&content)?
        };

        Ok(Self { country_codes })
    }

    pub fn get_countries_to_process(&self, target_countries: &[String]) -> Vec<String> {
        if target_countries.is_empty() || target_countries.iter().any(|c| c == "ALL") {
            self.country_codes.keys().cloned().collect()
        } else {
            target_countries
                .iter()
                .filter(|country| self.country_codes.contains_key(*country))
                .cloned()
                .collect()
        }
    }

    pub fn get_country_name(&self, country_code: &str) -> Option<&String> {
        self.country_codes.get(country_code)
    }

    pub async fn get_countries(
        &self,
        db_service: &crate::services::database::DatabaseService,
        target_countries: &[String],
    ) -> Vec<crate::models::country::CountryInfo> {
        let mut countries = Vec::new();

        let countries_to_process = self.get_countries_to_process(target_countries);

        match db_service
            .get_countries_locality_counts(&countries_to_process)
            .await
        {
            Ok(counts) => {
                for code in countries_to_process {
                    if let Some(name) = self.country_codes.get(&code) {
                        // Get the locality count from the batch result
                        let count = counts.get(&code).copied().unwrap_or(0);

                        if count > 0 {
                            countries.push(crate::models::country::CountryInfo {
                                country_code: code.clone(),
                                country_name: name.clone(),
                                locality_count: count,
                            });
                        }
                    }
                }
            }
            Err(_) => {
                for code in countries_to_process {
                    if let Some(name) = self.country_codes.get(&code) {
                        match db_service.get_country_locality_count(&code).await {
                            Ok(count) => {
                                if count > 0 {
                                    countries.push(crate::models::country::CountryInfo {
                                        country_code: code.clone(),
                                        country_name: name.clone(),
                                        locality_count: count,
                                    });
                                }
                            }
                            Err(_) => {
                                continue;
                            }
                        }
                    }
                }
            }
        }

        countries.sort_by(|a, b| {
            a.country_name
                .to_lowercase()
                .cmp(&b.country_name.to_lowercase())
        });

        countries
    }

    fn create_default_country_codes() -> HashMap<String, String> {
        let mut codes = HashMap::new();

        codes.insert("US".to_string(), "United States".to_string());
        codes.insert("CA".to_string(), "Canada".to_string());
        codes.insert("GB".to_string(), "United Kingdom".to_string());
        codes.insert("DE".to_string(), "Germany".to_string());
        codes.insert("FR".to_string(), "France".to_string());
        codes.insert("IT".to_string(), "Italy".to_string());
        codes.insert("ES".to_string(), "Spain".to_string());
        codes.insert("AU".to_string(), "Australia".to_string());
        codes.insert("JP".to_string(), "Japan".to_string());
        codes.insert("CN".to_string(), "China".to_string());
        codes.insert("IN".to_string(), "India".to_string());
        codes.insert("BR".to_string(), "Brazil".to_string());
        codes.insert("MX".to_string(), "Mexico".to_string());
        codes.insert("RU".to_string(), "Russia".to_string());

        codes
    }
}
