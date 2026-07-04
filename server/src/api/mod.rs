use axum::{
    body::Body,
    http::{self, header},
    response::Response,
};

pub mod auth;
pub mod automations;
pub mod email;
pub mod files;
pub mod github;
pub mod google;
pub mod images;
pub mod mcp;
pub mod memories;
pub mod openai;
pub mod projects;
pub mod skills;
pub mod telegram;
pub mod threads;
pub mod transcribe;
pub mod uploads;
pub mod writable_dirs;

const HTML_WIDGET_CSP: &str = "sandbox allow-scripts; default-src 'self'; script-src 'self' \
                               'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' \
                               data:; media-src 'self'; connect-src 'self'; font-src 'self'";

pub(crate) fn file_response(
    path: &str,
    bytes: Vec<u8>,
    stored_mime_type: Option<String>,
) -> Result<Response, http::Error> {
    let content_type = stored_mime_type.unwrap_or_else(|| infer_mime_type(path).to_string());
    let disposition = if should_render_inline(&content_type) {
        "inline"
    } else {
        "attachment"
    };
    let filename = path.split('/').next_back().unwrap_or(path);
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type.as_str())
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CONTENT_DISPOSITION,
            format!("{disposition}; filename=\"{}\"", header_filename(filename)),
        );

    if is_html(&content_type) {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, HTML_WIDGET_CSP);
    }

    builder.body(Body::from(bytes))
}

fn infer_mime_type(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn should_render_inline(content_type: &str) -> bool {
    let mime = bare_mime_lower(content_type);
    mime == "text/html"
        || mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
}

fn is_html(content_type: &str) -> bool {
    bare_mime_lower(content_type) == "text/html"
}

fn bare_mime_lower(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase()
}

fn header_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\r' | '\n' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn html_files_render_inline_with_sandbox_csp() {
        let response = file_response("widget.html", b"<h1>ok</h1>".to_vec(), None).unwrap();
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/html; charset=utf-8")
        );
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION).unwrap(),
            HeaderValue::from_static("inline; filename=\"widget.html\"")
        );
        assert!(
            headers
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("sandbox allow-scripts")
        );
    }

    #[test]
    fn unknown_files_stay_attachments() {
        let response = file_response("data.bin", vec![1, 2, 3], None).unwrap();
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("application/octet-stream")
        );
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION).unwrap(),
            HeaderValue::from_static("attachment; filename=\"data.bin\"")
        );
        assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_none());
    }
}
