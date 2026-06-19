//! Research harness — the measurement layer that converts pftui from
//! narrative to measured expectancy and scored self-evaluation.
//!
//! - [`registry`] — canonical deterministic signal emitters. A signal is
//!   `(canonical_id, version, description, emitter)`; the emitter walks a
//!   daily series and yields dated EVENTS (state *transitions*, never
//!   states).
//! - [`event_study`] — forward-return event studies per signal × asset ×
//!   horizon: hit rate, mean/median/quartiles, MAE/MFE, baseline + lift,
//!   honest significance (overlap exclusion + exact binomial vs the
//!   baseline up-rate), era and regime splits, walk-forward `as_of`
//!   semantics.
//! - [`forecast_scoring`] — turns the analyst judgment stream
//!   (`analyst_view_history`) into a scored corpus (`forecast_scores`).
//! - [`shadow_book`] — the counterfactual portfolio that mechanically
//!   executes every recommendations-ledger row (policy v1): shadow vs
//!   actual vs hold since ledger inception, computed on demand from
//!   ledger + prices + transactions with no state tables.
//! - [`thesis_verify`] — re-runs the evidence SQL embedded in curated
//!   thesis sections (`[pftui]` / `[derived]` / `[ext]` contract) and
//!   classifies each claim verified / drift / broken / untagged, with
//!   snapshot-vs-structural drift severity.
//!
//! Persistence: `db::signal_expectancy` (L2 derived, rebuildable) and
//! `forecast_scores` (L3 ledger, append-only). CLI surface:
//! `pftui research signals|backtest|expectancy|events|forecasts|shadowbook|verify-thesis`.

pub mod event_study;
pub mod forecast_scoring;
pub mod registry;
pub mod shadow_book;
pub mod thesis_verify;
pub mod validation;
