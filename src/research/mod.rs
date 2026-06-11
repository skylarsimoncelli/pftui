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
//!
//! Persistence: `db::signal_expectancy` (L2 derived, rebuildable) and
//! `forecast_scores` (L3 ledger, append-only). CLI surface:
//! `pftui research signals|backtest|expectancy|events|forecasts`.

pub mod event_study;
pub mod forecast_scoring;
pub mod registry;
