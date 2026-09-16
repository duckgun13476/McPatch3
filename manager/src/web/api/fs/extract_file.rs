use std::collections::HashMap;
use std::time::Duration;
use std::time::SystemTime;

use axum::body::Body;
use axum::extract::Query;
use axum::extract::State;
use axum::response::Response;
use sha2::Digest;
use sha2::Sha256;

use crate::utility::filename_ext::GetFileNamePart;
use crate::web::webstate::WebState;
use crate::web::api::fs::workspace_path;

pub async fn api_extract_file(State(state): State<WebState>, Query(params): Query<HashMap<String, String>>) -> Response {
    let signature = match params.get("sign") {
        Some(ok) => ok,
        None => return Response::builder()
            .status(403)
            .body(Body::empty())
            .unwrap(),
    };

    let mut split = signature.split(":");

    let path = match split.next() {
        Some(ok) => ok,
        None => return Response::builder().status(403).body(Body::empty()).unwrap(),
    };

    let expire = match split.next() {
        Some(ok) => match u64::from_str_radix(ok, 10) {
            Ok(ok) => ok,
            Err(_) => return Response::builder().status(403).body(Body::empty()).unwrap(),
        },
        None => return Response::builder().status(403).body(Body::empty()).unwrap(),
    };

    let digest = match split.next() {
        Some(ok) => ok,
        None => return Response::builder().status(403).body(Body::empty()).unwrap(),
    };

    let username = state.auth.username().await;
    let password = state.auth.password().await;

    let hash = hash(&format!("{}:{}:{}@{}", path, expire, username, password));

    if hash != digest {
        return Response::builder().status(403).body(Body::new("invalid signature".to_owned())).unwrap();
    }

    // 检查是否超过有效期
    if (SystemTime::UNIX_EPOCH + Duration::from_secs(expire)).duration_since(SystemTime::UNIX_EPOCH).is_err() {
        return Response::builder().status(403).body(Body::new("signature is outdate".to_owned())).unwrap();
    }

    let path = match workspace_path(&state.apppath, path, false) {
        Ok(path) => path,
        Err(_) => return Response::builder().status(403).body(Body::empty()).unwrap(),
    };

    let metadata = tokio::fs::metadata(&path).await.unwrap();

    let file = tokio::fs::File::options()
        .read(true)
        .open(&path)
        .await
        .unwrap();

    let file = tokio_util::io::ReaderStream::new(file);

    let preview = params.get("preview").is_some_and(|value| value == "1");
    let content_type = if preview {
        preview_content_type(&path)
    } else {
        "application/octet-stream"
    };
    let disposition = if preview { "inline" } else { "attachment" };

    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(axum::http::header::CONTENT_DISPOSITION, format!("{disposition}; filename=\"{}\"", path.filename()))
        .header(axum::http::header::CONTENT_LENGTH, format!("{}", metadata.len()))
        .body(Body::from_stream(file)).unwrap()
}

fn preview_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("txt") | Some("log") | Some("md") | Some("toml") | Some("yml") | Some("yaml") | Some("properties") => "text/plain; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("pdf") => "application/pdf",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

fn hash(text: &impl AsRef<str>) -> String {
    let hash = Sha256::digest(text.as_ref());
    
    base16ct::lower::encode_string(&hash)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::preview_content_type;

    #[test]
    fn preview_only_assigns_safe_inline_types() {
        assert_eq!(preview_content_type(Path::new("notice.md")), "text/plain; charset=utf-8");
        assert_eq!(preview_content_type(Path::new("image.png")), "image/png");
        assert_eq!(preview_content_type(Path::new("manual.pdf")), "application/pdf");
        assert_eq!(preview_content_type(Path::new("page.html")), "application/octet-stream");
        assert_eq!(preview_content_type(Path::new("icon.svg")), "application/octet-stream");
        assert_eq!(preview_content_type(Path::new("mod.jar")), "application/octet-stream");
    }
}
