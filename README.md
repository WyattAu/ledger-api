# ledger-api

HTTP API over the WyattAu accounting estate: ledger, reconciliation, tax,
reports — all from the released crates. The composition layer that converts
the correctness layer into something a platform can embed.

## The routes

| Route | Method | What it does | Crate |
|---|---|---|---|
| `/health` | GET | Health check | — |
| `/reconcile` | POST | Statement-to-ledger matching | `recon-kit` |
| `/sales-tax/compute` | POST | US sales tax | `sales-tax` |

## Status: 0.1.0

Minimal viable surface. The endpoints that touch the database (trial
balance, aging, ledger posting) are planned for 0.2.0 with `ledger-pg`
integration. The reconcile and tax endpoints are pure computation over
the request body — no database needed, which means they can be tested
without infrastructure.

## Design decisions

**No business logic in the routes.** Each handler composes estate crates;
the correctness lives in the crates, not in the API layer. A bug in the
routes is a routing bug, not an accounting bug.

**Nexus-gated by default.** The sales-tax endpoint refuses to compute when
nexus is absent — the response reports `nexus_established: false` so the
caller knows it's a *legal* zero, not a missing rate.

**Ambiguity surfaced, never decided.** The reconcile endpoint returns
`ambiguous` entries separately from `matches`. The operator confirms;
the API proposes.

## Licence

AGPL-3.0.
