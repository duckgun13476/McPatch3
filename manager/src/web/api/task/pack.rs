use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::task::pack::{build_pack_plan, select_pack_changes, task_pack_selected};
use crate::web::api::PublicResponseBody;
use crate::web::webstate::WebState;

#[derive(Deserialize)]
pub struct RequestBody {
    label: String,
    change_logs: String,
    #[serde(default)]
    confirmation_fingerprint: Option<String>,
    #[serde(default)]
    excluded_change_ids: Vec<String>,
}

/// 第一次调用返回变化预览；携带相同预览指纹再次调用才执行打包。
pub async fn api_pack(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<RequestBody>,
) -> Response {
    let pending = state.pending_changes.lock().await.clone();
    let apppath = state.apppath.clone();
    let config = state.config.clone();
    let label = payload.label.clone();
    let logs = payload.change_logs.clone();
    let plan = match tokio::task::spawn_blocking(move || {
        build_pack_plan(&label, &logs, &apppath, &config, &pending)
    })
    .await
    {
        Ok(Ok(plan)) => plan,
        Ok(Err(error)) => return PublicResponseBody::<()>::err(&error),
        Err(error) => return PublicResponseBody::<()>::err(&format!("生成变化预览失败: {error}")),
    };

    let Some(confirmed) = payload.confirmation_fingerprint else {
        return PublicResponseBody::ok(plan.preview);
    };
    if confirmed != plan.preview.fingerprint {
        return PublicResponseBody::<()>::err("工作区已变化，请重新检查并确认变化列表");
    }

    let selection = match select_pack_changes(plan, &payload.excluded_change_ids) {
        Ok(selection) => selection,
        Err(error) => return PublicResponseBody::<()>::err(&error),
    };

    let wait = headers.get("wait").is_some();
    state
        .clone()
        .te
        .lock()
        .await
        .try_schedule(wait, state.clone(), move || {
            let code = task_pack_selected(
                payload.label,
                payload.change_logs,
                selection.changes,
                selection.hash_deletions,
                &state.apppath,
                &state.config,
                &state.console,
            );
            if code == 0 {
                if !selection.emitted_pending_deletions.is_empty()
                    || !selection.emitted_pending_hash_deletions.is_empty()
                {
                    let mut pending = state.pending_changes.blocking_lock();
                    pending.mark_emitted(&selection.emitted_pending_deletions);
                    pending.mark_hash_emitted(&selection.emitted_pending_hash_deletions);
                    if let Err(error) = pending.save(&state.apppath.pending_changes_file) {
                        state
                            .console
                            .log_warning(format!("更新包已生成，但删除规则状态保存失败: {error}"));
                    }
                }
                state.status.blocking_lock().invalidate();
            }
            code
        })
        .await
}
