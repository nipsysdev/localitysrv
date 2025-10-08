use crate::AppState;
use axum::{extract::State, Json};
use serde_json::json;

pub async fn get_countries(State(app_state): State<AppState>) -> Json<serde_json::Value> {
    let countries = app_state
        .country_service
        .get_countries(&app_state.db_service, &app_state.config.target_countries)
        .await;
    Json(json!({
        "success": true,
        "data": countries
    }))
}
