use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};

#[derive(serde::Deserialize)]
pub struct CountryQueryParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub q: Option<String>,
}

pub async fn search_countries(
    State(app_state): State<AppState>,
    Query(params): Query<CountryQueryParams>,
) -> Json<serde_json::Value> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(10);
    let query = params.q.as_deref();

    if page < 1 {
        return Json(serde_json::json!({
            "success": false,
            "error": "Page must be a positive integer"
        }));
    }

    let config = app_state.config.lock().await;
    let countries_result = match app_state
        .country_service
        .get_countries_paginated(
            &app_state.db_service,
            &config.target_countries,
            page,
            limit,
            query,
        )
        .await
    {
        Ok(countries) => countries,
        Err(e) => {
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to get countries: {}", e)
            }));
        }
    };

    let total = match app_state
        .country_service
        .get_countries_count(&app_state.db_service, &config.target_countries, query)
        .await
    {
        Ok(count) => count,
        Err(e) => {
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to get countries count: {}", e)
            }));
        }
    };

    let total_pages = (total as f64 / limit as f64).ceil() as u32;

    Json(serde_json::json!({
        "success": true,
        "data": countries_result,
        "pagination": {
            "total": total,
            "page": page,
            "limit": limit,
            "total_pages": total_pages
        }
    }))
}
