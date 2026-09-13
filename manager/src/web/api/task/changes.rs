use axum::extract::State;
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

#[derive(Deserialize)]
pub struct ChangePathRequest {
    path: String,
}

#[derive(Serialize)]
pub struct ChangePathResponse {
    path: String,
    removed: bool,
}

pub async fn api_add_delete_file(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let path = match updated.add_forced_deletion(&payload.path) {
        Ok(path) => path,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(ChangePathResponse {
        path,
        removed: false,
    })
}

pub async fn api_remove_delete_file(
    State(state): State<WebState>,
    Json(payload): Json<ChangePathRequest>,
) -> Response {
    let mut guard = state.pending_changes.lock().await;
    let mut updated = guard.clone();
    let removed = match updated.remove_forced_deletion(&payload.path) {
        Ok(removed) => removed,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };
    if let Err(error) = updated.save(&state.apppath.pending_changes_file) {
        return PublicResponseBody::<()>::err(&error);
    }
    *guard = updated;
    PublicResponseBody::ok(ChangePathResponse {
        path: payload.path,
        removed,
    })
}
