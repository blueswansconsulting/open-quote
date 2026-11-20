//! Open Quote: a free, self-hostable quote PDF generator for one-person
//! trade businesses. Stateless: nothing a user types is stored server-side.

pub mod logo;
pub mod pdf;
pub mod quote;
pub mod render;

use std::sync::atomic::{AtomicU64, Ordering};

use std::collections::HashMap;

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Query},
    http::{header, HeaderValue, StatusCode},
    middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;

const INDEX_HTML: &str = include_str!("../static/index.html");
/// Room for a logo and three phone photos after client-side downscaling.
pub const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;

/// PDFs generated since start-up. A count only: no quote content is kept.
static PDFS_GENERATED: AtomicU64 = AtomicU64::new(0);

pub fn pdfs_generated() -> u64 {
    PDFS_GENERATED.load(Ordering::Relaxed)
}

pub fn app() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/stats", get(stats))
        .route("/api/logo", get(fetch_logo))
        .route("/api/quote.pdf", post(make_pdf))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::map_response(security_headers))
}

async fn security_headers(mut res: Response) -> Response {
    let h = res.headers_mut();
    // Keep a stricter policy a handler already set (the logo endpoint).
    if !h.contains_key(header::CONTENT_SECURITY_POLICY) {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
        );
    }
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_FRAME_OPTIONS, "DENY"),
    ] {
        h.insert(name, HeaderValue::from_static(value));
    }
    res
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// At most this many logo downloads run at once, so the endpoint can't be
/// used to tie up the server.
const LOGO_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

static LOGO_FETCHES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

/// `GET /api/logo?url=https://…` downloads a logo for a prefill link. See
/// [`logo`] for the SSRF protections. Nothing is stored or logged.
async fn fetch_logo(Query(params): Query<HashMap<String, String>>) -> Response {
    let Some(url) = params.get("url").cloned() else {
        return error(StatusCode::BAD_REQUEST, "Add ?url=https://… for the logo");
    };
    if let Err(e) = logo::check_url(&url) {
        return error(StatusCode::UNPROCESSABLE_ENTITY, e.message());
    }
    let Ok(_permit) = LOGO_FETCHES.try_acquire() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Busy fetching other logos. Try again in a moment.",
        );
    };
    let result = tokio::task::spawn_blocking(move || logo::fetch(&url)).await;
    match result {
        // Served as a sandboxed download: the form only reads it with fetch()
        // to convert it, so even a hostile SVG can never run on this origin.
        Ok(Ok((content_type, bytes))) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "no-store"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"logo\""),
                (header::CONTENT_SECURITY_POLICY, LOGO_CSP),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(e)) => {
            // One status for every failure, so responses reveal nothing about
            // the server's network.
            error(StatusCode::UNPROCESSABLE_ENTITY, e.message())
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Logo download failed"),
    }
}

async fn stats() -> Json<serde_json::Value> {
    Json(json!({ "pdfs_generated": pdfs_generated() }))
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

/// Keep only filename-safe characters from the quote number.
fn filename_for(quote_number: &str) -> String {
    let safe: String = quote_number
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(30)
        .collect();
    if safe.is_empty() {
        "quote.pdf".into()
    } else {
        format!("quote-{safe}.pdf")
    }
}

async fn make_pdf(payload: Result<Json<quote::QuoteRequest>, JsonRejection>) -> Response {
    let req = match payload {
        Ok(Json(req)) => req,
        Err(rejection) => {
            let message = match rejection.status() {
                StatusCode::PAYLOAD_TOO_LARGE => {
                    "The quote is too large. Try fewer or smaller photos.".to_string()
                }
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "Send the quote as JSON.".to_string(),
                _ => format!("Invalid request: {}", rejection.body_text()),
            };
            return error(rejection.status(), message);
        }
    };
    let q = match quote::validate(req) {
        Ok(q) => q,
        Err(message) => return error(StatusCode::UNPROCESSABLE_ENTITY, message),
    };
    let bytes = render::render_quote(&q);
    let total = PDFS_GENERATED.fetch_add(1, Ordering::Relaxed) + 1;
    // Anonymous usage signal for operators: a running count, nothing else.
    println!("pdf_generated total={total}");
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename_for(&q.quote_number)),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::filename_for;

    #[test]
    fn filename_is_sanitised() {
        assert_eq!(filename_for("Q-0001"), "quote-Q-0001.pdf");
        assert_eq!(filename_for("../../etc\"; x"), "quote-etcx.pdf");
        assert_eq!(filename_for(""), "quote.pdf");
    }
}
