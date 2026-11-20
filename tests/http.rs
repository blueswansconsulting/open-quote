use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn post(body: String) -> (StatusCode, Vec<u8>, Option<String>) {
    let res = open_quote::app()
        .oneshot(
            Request::post("/api/quote.pdf")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let ctype = res
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, bytes, ctype)
}

fn valid() -> Value {
    json!({
        "business": { "name": "Sparks Electrical", "phone": "07700 900123" },
        "customer": { "name": "Mrs Jones", "address": "2 Low Rd\nBristol" },
        "quote_number": "Q-0001",
        "date": "20 Nov 2026",
        "valid_until": "20 Dec 2026",
        "items": [
            { "description": "Replace consumer unit", "quantity": 1, "unit_price": 450 },
            { "description": "Labour", "quantity": 6, "unit_price": 45 }
        ],
        "vat": true,
        "notes": "Valid for 30 days."
    })
}

#[tokio::test]
async fn returns_a_pdf_for_a_valid_quote() {
    let (status, body, ctype) = post(valid().to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ctype.as_deref(), Some("application/pdf"));
    assert!(body.starts_with(b"%PDF-1.4"));
    // Total £864.00 (720 + 20% VAT) is drawn as WinAnsi hex.
    let hex: String = "£864.00"
        .chars()
        .map(|c| format!("{:02X}", open_quote::pdf::win_ansi(c)))
        .collect();
    assert!(String::from_utf8_lossy(&body).contains(&hex));
}

#[tokio::test]
async fn validation_errors_are_422_json() {
    let mut body = valid();
    body["items"] = json!([]);
    let (status, bytes, _) = post(body.to_string()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v["error"].as_str().unwrap().contains("at least one"));
}

#[tokio::test]
async fn malformed_json_is_400() {
    let (status, _, _) = post("{not json".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = post(json!({"items": "nope"}).to_string()).await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let mut body = valid();
    body["notes"] = json!("a".repeat(7 * 1024 * 1024));
    let (status, _, _) = post(body.to_string()).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn serves_the_form() {
    let res = open_quote::app()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&bytes).contains("<title>Open Quote</title>"));
}

#[tokio::test]
async fn sets_security_headers() {
    let res = open_quote::app()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let h = res.headers();
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["x-frame-options"], "DENY");
    assert!(h["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn counts_pdfs_without_storing_content() {
    let before = open_quote::pdfs_generated();
    let (status, _, _) = post(valid().to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(open_quote::pdfs_generated() > before);
    let res = open_quote::app()
        .oneshot(Request::get("/api/stats").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let v: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        v.as_object().unwrap().len(),
        1,
        "stats exposes only a count"
    );
    assert!(v["pdfs_generated"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn accepts_photos_within_the_body_limit() {
    // ~1 MB photo as base64 fits the 6 MB limit; it is rejected by validation
    // only because the bytes aren't a JPEG, proving the body got through.
    let mut body = valid();
    body["photos"] = json!([{ "data": "A".repeat(1_300_000), "caption": "" }]);
    let (status, bytes, _) = post(body.to_string()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v["error"].as_str().unwrap().contains("Photo 1"));
}
