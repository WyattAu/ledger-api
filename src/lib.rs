//! HTTP API over the WyattAu accounting estate.
//!
//! # What this crate is
//!
//! An axum router that exposes the estate's crates as JSON endpoints: the
//! ledger (balance), reports (trial balance), reconciliation (match), tax
//! (US sales tax), and health. It is the composition layer that converts
//! the correctness layer into something a platform can embed.
//!
//! # What this crate is not
//!
//! Not a UI, not a payment processor, not a rate data service, not a
//! MTD filing transport. The endpoints take and return typed serde
//! structs — the caller wires them to their own frontend, payment
//! processor, rate provider, and HMRC credentials.

pub mod routes;

pub use routes::router;

/// The app state: a connection pool and tenant context.
///
/// `Clone` because axum handlers need per-request clones; the pool is
/// internally reference-counted, so the clone shares the same connections.
#[derive(Clone)]
pub struct AppState {
    /// The Postgres connection pool, shared across tenants.
    pub pool: sqlx::PgPool,
    /// The tenant this instance serves. Multi-tenancy is per-instance,
    /// not per-request: one deploy per tenant, not one tenant per header.
    pub tenant: String,
}

/// Type alias for axum's `State` extractor.
pub type SharedState = AppState;
