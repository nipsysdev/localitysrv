use crate::models::locality::LocalityInfo;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use std::path::Path as StdPath;
use tokio::fs;

#[derive(serde::Deserialize)]
pub struct LocalityQueryParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub q: Option<String>,
}

pub async fn search_localities(
    State(app_state): State<AppState>,
    Path(country_code): Path<String>,
    Query(params): Query<LocalityQueryParams>,
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

    let localities_result = match app_state
        .db_service
        .get_localities(&country_code, page, limit, query)
        .await
    {
        Ok(localities) => localities,
        Err(e) => {
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to get localities: {}", e)
            }));
        }
    };

    let total = match app_state
        .db_service
        .get_localities_count(&country_code, query)
        .await
    {
        Ok(count) => count,
        Err(e) => {
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to get localities count: {}", e)
            }));
        }
    };

    let total_pages = (total as f64 / limit as f64).ceil() as u32;

    let country_code_clone = country_code.clone();
    let localities_info: Vec<LocalityInfo> =
        futures::future::join_all(localities_result.into_iter().map(|locality| {
            let id = locality.id;
            let name = locality.name;
            let country = locality.country;
            let placetype = locality.placetype;
            let latitude = locality.latitude;
            let longitude = locality.longitude;
            let min_longitude = locality.min_longitude;
            let min_latitude = locality.min_latitude;
            let max_longitude = locality.max_longitude;
            let max_latitude = locality.max_latitude;
            let assets_dir = app_state.config.assets_dir.clone();
            let country_code_for_async = country_code_clone.clone();

            async move {
                let file_path = StdPath::new(&assets_dir)
                    .join("localities")
                    .join(&country_code_for_async)
                    .join(format!("{}.pmtiles", id));

                let file_size = match fs::metadata(&file_path).await {
                    Ok(metadata) => metadata.len(),
                    Err(_) => 0,
                };

                LocalityInfo {
                    id,
                    name,
                    country,
                    placetype,
                    latitude,
                    longitude,
                    min_longitude,
                    min_latitude,
                    max_longitude,
                    max_latitude,
                    file_size,
                }
            }
        }))
        .await;

    Json(serde_json::json!({
        "success": true,
        "data": localities_info,
        "pagination": {
            "total": total,
            "page": page,
            "limit": limit,
            "total_pages": total_pages
        }
    }))
}
