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

    pub async fn get_countries_paginated(
        &self,
        db_service: &crate::services::database::DatabaseService,
        target_countries: &[String],
        page: u32,
        limit: u32,
        query: Option<&str>,
    ) -> Result<Vec<crate::models::country::CountryInfo>, CountryError> {
        let countries_to_process = self.get_countries_to_process(target_countries);

        let filtered_countries = if let Some(q) = query {
            countries_to_process
                .into_iter()
                .filter(|code| {
                    if let Some(name) = self.country_codes.get(code) {
                        name.to_lowercase().contains(&q.to_lowercase())
                            || code.to_lowercase().contains(&q.to_lowercase())
                    } else {
                        false
                    }
                })
                .collect()
        } else {
            countries_to_process
        };

        let mut countries = Vec::new();

        // Get all countries with their counts
        match db_service
            .get_countries_locality_counts(&filtered_countries)
            .await
        {
            Ok(counts) => {
                for code in filtered_countries {
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
                for code in filtered_countries {
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

        // Sort by country name
        countries.sort_by(|a, b| {
            a.country_name
                .to_lowercase()
                .cmp(&b.country_name.to_lowercase())
        });

        // Apply pagination
        let offset = (page - 1) * limit;
        let end = std::cmp::min(offset + limit, countries.len() as u32);
        let paginated_countries = countries
            .into_iter()
            .skip(offset as usize)
            .take((end - offset) as usize)
            .collect();

        Ok(paginated_countries)
    }

    pub async fn get_countries_count(
        &self,
        db_service: &crate::services::database::DatabaseService,
        target_countries: &[String],
        query: Option<&str>,
    ) -> Result<u32, CountryError> {
        let countries_to_process = self.get_countries_to_process(target_countries);

        let filtered_countries = if let Some(q) = query {
            countries_to_process
                .into_iter()
                .filter(|code| {
                    if let Some(name) = self.country_codes.get(code) {
                        name.to_lowercase().contains(&q.to_lowercase())
                            || code.to_lowercase().contains(&q.to_lowercase())
                    } else {
                        false
                    }
                })
                .collect()
        } else {
            countries_to_process
        };

        let mut count = 0;

        for code in filtered_countries {
            if self.country_codes.contains_key(&code) {
                match db_service.get_country_locality_count(&code).await {
                    Ok(locality_count) => {
                        if locality_count > 0 {
                            count += 1;
                        }
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }
        }

        Ok(count)
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
