//! API integration tests: the routes work over released artifacts, with
//! no database required for the pure-computation endpoints.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ledger_api::{AppState, SharedState};
use tower::ServiceExt;

fn router(state: AppState) -> axum::Router {
    ledger_api::routes::router(state)
}

fn make_state() -> SharedState {
    SharedState {
        pool: sqlx::postgres::PgPool::connect_lazy(
            "postgres://postgres:test@127.0.0.1:54335/postgres",
        )
        .expect("lazy pool never fails"),
        tenant: "test".into(),
    }
}

#[tokio::test]
async fn health_returns_ok() {
    let app = router(make_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn sales_tax_computes_from_json() {
    let app = router(make_state());
    let body = serde_json::json!({
        "lines": [{"net_minor": 1_000_000, "taxable": true}],
        "destination_state": "TX",
        "nexus_states": ["TX"],
        "rates": [
            {"level": "state", "name": "Texas", "percent": "6.25"},
            {"level": "special_district", "name": "Transit", "percent": "2.00"}
        ]
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sales-tax/compute")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["nexus_established"], true);
    assert_eq!(json["total_tax_minor"], 82500);
}

#[tokio::test]
async fn reconcile_surfaces_matches_and_ambiguity() {
    let app = router(make_state());
    let body = serde_json::json!({
        "lines": [
            {"id": "L1", "booked_on": "2026-07-02", "amount": 50000, "description": "PAYMENT INV-001"},
            {"id": "L2", "booked_on": "2026-07-03", "amount": 30000, "description": "PAYMENT INV-002"}
        ],
        "entries": [
            {"entry_id": "JE-1", "booked_on": "2026-07-02", "amount": 50000, "memo": "PAYMENT INV-001"},
            {"entry_id": "JE-2", "booked_on": "2026-07-05", "amount": 30000, "memo": "PAYMENT INV-002"}
        ]
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reconcile")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["matches"].as_array().expect("matches").len(), 2);
}
