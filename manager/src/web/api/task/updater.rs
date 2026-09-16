use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::task::pack::{
    build_pack_plan, select_updater_self_update, task_pack_selected,
};
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

#[derive(Deserialize)]
pub struct RequestBody {
    label: String,
    #[serde(default)]
    change_logs: String,
}

pub async fn api_pack_updater(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<RequestBody>,
) -> Response {
    let pending = state.pending_changes.lock().await.clone();
    let apppath = state.apppath.clone();
    let config = state.config.clone();
    let label = payload.label.clone();
    let logs = if payload.change_logs.trim().is_empty() {
        "更新自动更新器".to_owned()
    } else {
        payload.change_logs.clone()
    };
    let planning_logs = logs.clone();
    let plan = match tokio::task::spawn_blocking(move || {
        build_pack_plan(&label, &planning_logs, &apppath, &config, &pending)
    })
    .await
    {
        Ok(Ok(plan)) => plan,
        Ok(Err(error)) => return PublicResponseBody::<()>::err(&error),
        Err(error) => return PublicResponseBody::<()>::err(&format!("生成更新器专用包失败: {error}")),
    };
    let selection = match select_updater_self_update(plan) {
        Ok(selection) => selection,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };

    let wait = headers.get("wait").is_some();
    state.clone().te.lock().await.try_schedule(wait, state.clone(), move || {
        let code = task_pack_selected(
            payload.label,
            logs,
            selection.changes,
            Vec::new(),
            &state.apppath,
            &state.config,
            &state.console,
        );
        if code == 0 {
            state.status.blocking_lock().invalidate();
        }
        code
    }).await
}
