use axum::extract::State;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use crate::web::api::PublicResponseBody;
use crate::web::api::fs::workspace_path;
use crate::web::webstate::WebState;

#[derive(Deserialize)]
pub struct RequestBody {
    /// 原路径
    from: String,

    /// 目标路径
    to: String,
}

pub async fn api_move(State(state): State<WebState>, Json(payload): Json<RequestBody>) -> Response {
    let from = payload.from;
    let to = payload.to;

    // 路径不能为空
    if from.is_empty() {
        return PublicResponseBody::<()>::err("parameter 'from' is empty");
    }

    if to.is_empty() {
        return PublicResponseBody::<()>::err("parameter 'to' is empty");
    }

    let file_from = match workspace_path(&state.apppath, &from, false) {
        Ok(path) => path,
        Err(err) => return PublicResponseBody::<()>::err(&err),
    };
    let file_to = match workspace_path(&state.apppath, &to, false) {
        Ok(path) => path,
        Err(err) => return PublicResponseBody::<()>::err(&err),
    };

    if !file_from.exists() {
        return PublicResponseBody::<()>::err(&format!("'{}' not exists.", from));
    }

    if file_to.exists() {
        return PublicResponseBody::<()>::err(&format!("'{}' exists.", to));
    }
    
    match tokio::fs::rename(&file_from, &file_to).await {
        Ok(_) => (),
        Err(err) => return PublicResponseBody::<()>::err(&format!("{:?}", err)),
    }

    // 清除文件状态缓存
    let mut status = state.status.lock().await;
    status.invalidate();

    PublicResponseBody::<()>::ok_no_data()
}
