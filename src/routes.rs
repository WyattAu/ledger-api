//! The axum routes: one module, because the surface is intentionally small.
//!
//! Each handler composes estate crates — no business logic lives here.
//! The routes are the delivery mechanism; the correctness lives in the
//! crates.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::SharedState;

/// The health check, so the embedder's load balancer has something to probe.
pub async fn health() -> &'static str {
    "ok"
}

/// A single account's balance response.
#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub account: String,
    pub currency: String,
    pub balance_minor: i64,
}

/// GET /accounts/:id/balance/:currency
async fn account_balance(
    State(state): State<SharedState>,
    Path((account, currency)): Path<(String, String)>,
) -> Json<BalanceResponse> {
    let (balance,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN l.side = 'debit' THEN l.amount_units ELSE -l.amount_units END), 0)::BIGINT \
         FROM ledger_lines l \
         JOIN ledger_entries e ON e.tenant_id = l.tenant_id AND e.id = l.entry_id \
         WHERE l.tenant_id = $1 AND l.account = $2 AND l.amount_currency = $3 AND e.state = 'posted'",
    )
    .bind(&state.tenant)
    .bind(&account)
    .bind(&currency)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    Json(BalanceResponse {
        account,
        currency,
        balance_minor: balance,
    })
}

/// A reconciliation request.
#[derive(Debug, Deserialize)]
pub struct ReconcileRequest {
    pub lines: Vec<ReconcileLine>,
    pub entries: Vec<ReconcileEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileLine {
    pub id: String,
    pub booked_on: String,
    pub amount: i64,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileEntry {
    pub entry_id: String,
    pub booked_on: String,
    pub amount: i64,
    pub memo: String,
}

#[derive(Debug, Serialize)]
pub struct MatchResponse {
    pub line_id: String,
    pub entry_id: String,
}

#[derive(Debug, Serialize)]
pub struct AmbiguousResponse {
    pub line_id: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReconcileResponse {
    pub matches: Vec<MatchResponse>,
    pub ambiguous: Vec<AmbiguousResponse>,
}

/// POST /reconcile — propose matches between statement lines and ledger
/// entries.
async fn reconcile(
    State(_state): State<SharedState>,
    Json(request): Json<ReconcileRequest>,
) -> Json<ReconcileResponse> {
    let lines: Vec<recon_kit::StatementLine> = request
        .lines
        .iter()
        .map(|l| recon_kit::StatementLine {
            id: l.id.clone(),
            booked_on: l.booked_on.clone(),
            amount: l.amount.unsigned_abs(),
            direction: recon_kit::Direction::Credit,
            description: l.description.clone(),
        })
        .collect();
    let entries: Vec<recon_kit::LedgerSide> = request
        .entries
        .iter()
        .map(|e| recon_kit::LedgerSide {
            entry_id: e.entry_id.clone(),
            booked_on: e.booked_on.clone(),
            amount: e.amount.unsigned_abs(),
            direction: recon_kit::Direction::Credit,
            memo: e.memo.clone(),
        })
        .collect();

    let empty = std::collections::HashSet::new();
    let exact = recon_kit::match_exact(&lines, &entries, &empty, &empty);
    let taken_lines: std::collections::HashSet<String> =
        exact.iter().map(|m| m.line_id.clone()).collect();
    let taken_entries: std::collections::HashSet<String> =
        exact.iter().map(|m| m.entry_id.clone()).collect();

    let window = recon_kit::match_window(&lines, &entries, 3, &taken_lines, &taken_entries);

    let mut response = ReconcileResponse {
        matches: Vec::new(),
        ambiguous: Vec::new(),
    };
    for m in &exact {
        response.matches.push(MatchResponse {
            line_id: m.line_id.clone(),
            entry_id: m.entry_id.clone(),
        });
    }
    for p in &window {
        match p {
            recon_kit::Proposal::Match(m) => {
                response.matches.push(MatchResponse {
                    line_id: m.line_id.clone(),
                    entry_id: m.entry_id.clone(),
                });
            }
            recon_kit::Proposal::Ambiguous {
                line_id,
                candidates,
            } => {
                response.ambiguous.push(AmbiguousResponse {
                    line_id: line_id.clone(),
                    candidates: candidates.clone(),
                });
            }
        }
    }
    Json(response)
}

/// A US sales tax computation request.
#[derive(Debug, Deserialize)]
pub struct SalesTaxRequest {
    pub lines: Vec<SalesTaxLine>,
    pub destination_state: String,
    pub nexus_states: Vec<String>,
    pub rates: Vec<SalesTaxRate>,
}

#[derive(Debug, Deserialize)]
pub struct SalesTaxLine {
    pub net_minor: i64,
    pub taxable: bool,
}

#[derive(Debug, Deserialize)]
pub struct SalesTaxRate {
    pub level: String,
    pub name: String,
    pub percent: String,
}

/// POST /sales-tax/compute — nexus-gated, jurisdiction-stacked US sales tax.
async fn compute_sales_tax(Json(request): Json<SalesTaxRequest>) -> Json<serde_json::Value> {
    let lines: Vec<sales_tax::SaleLine> = request
        .lines
        .iter()
        .map(|l| sales_tax::SaleLine {
            net_minor: l.net_minor,
            taxable: l.taxable,
        })
        .collect();
    let mut rates = Vec::new();
    for r in &request.rates {
        let level = match r.level.as_str() {
            "state" => sales_tax::JurisdictionLevel::State,
            "county" => sales_tax::JurisdictionLevel::County,
            "city" => sales_tax::JurisdictionLevel::City,
            _ => sales_tax::JurisdictionLevel::SpecialDistrict,
        };
        if let Ok(jr) = sales_tax::JurisdictionRate::from_percent(level, &r.name, &r.percent) {
            rates.push(jr);
        }
    }
    let nexus = sales_tax::NexusRegistry::new(&request.nexus_states);

    let breakdown = sales_tax::compute_tax(
        &lines,
        &request.destination_state,
        &rates,
        &nexus,
        sales_tax::RoundingScope::PerDocument,
        vat_rules::TieMode::HalfUp,
    );

    match breakdown {
        Ok(b) => Json(serde_json::json!({
            "nexus_established": b.nexus_established,
            "total_tax_minor": b.total_tax,
            "per_jurisdiction": b.per_jurisdiction.iter()
                .map(|(level, name, tax)| {
                    serde_json::json!({"level": format!("{level:?}"), "name": name, "tax_minor": tax})
                })
                .collect::<Vec<_>>(),
        })),
        Err(e) => Json(serde_json::json!({ "error": format!("{e:?}") })),
    }
}

/// The routes for the ledger API.
pub fn router(state: crate::AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/accounts/{account_id}/balance/{currency}",
            get(account_balance),
        )
        .route("/reconcile", post(reconcile))
        .route("/sales-tax/compute", post(compute_sales_tax))
        .with_state(state)
}
