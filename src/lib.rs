//! HTTP API over the WyattAu accounting estate.
//!
//! # What this crate is
//!
//! An axum binary that exposes the estate's crates as JSON endpoints:
//! the ledger (post, balance, verify chain), reports (trial balance),
//! reconciliation (match), and tax (US sales tax). It is the composition
//! layer that converts the correctness layer into something a platform
//! can deploy.
//!
//! # Design decisions
//!
//! **No business logic in the routes.** Each handler composes estate
//! crates; the correctness lives in the crates, not in the API layer.
//!
//! **Multi-tenancy is per-instance.** One deploy per tenant, not one
//! tenant per header. This is the right model for the embedded-engine
//! buyer (a platform embedding the engine for their own users), where
//! each deployment serves one organisation.
//!
//! **The database is required.** The binary refuses to start without a
//! reachable PostgreSQL. `ensure_schema` runs at startup — idempotent,
//! so a restart is safe.

pub mod routes;

pub use routes::router;

/// Type alias for axum's `State` extractor.
pub type SharedState = AppState;

/// The app state: a connection pool and tenant context.
///
/// `Clone` because axum handlers need per-request clones; the pool is
/// internally reference-counted, so the clone shares the same connections.
#[derive(Clone)]
pub struct AppState {
    /// The Postgres connection pool, shared across handlers.
    pub pool: sqlx::PgPool,
    /// The tenant this instance serves.
    pub tenant: String,
}

/// Connect to the database, ensure the schema, and return the app state.
///
/// The DSN comes from the `DATABASE_URL` environment variable. The tenant
/// from `TENANT_ID`. Both are required — the binary refuses to start
/// without them, because a missing tenant is a routing error and a
/// missing database is a deployment error.
pub async fn connect() -> Result<AppState, Box<dyn std::error::Error>> {
    let dsn = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set")?;
    let tenant = std::env::var("TENANT_ID").map_err(|_| "TENANT_ID must be set")?;
    let pool = sqlx::postgres::PgPool::connect(&dsn).await?;
    ledger_pg::ensure_schema(&pool).await?;
    let handle = ledger_pg::PgLedger::connect(pool.clone(), &tenant).await?;
    drop(handle); // The connect call ensures the tenant row exists.
    Ok(AppState { pool, tenant })
}
