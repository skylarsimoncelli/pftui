//! Research harness — the signal registry and event-study engine that
//! converts pftui from narrative to measured expectancy.
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
//!
//! Persistence lives in `db::signal_expectancy` (L2 derived, rebuildable);
//! the CLI surface is `pftui research signals|backtest|expectancy|events`.

pub mod event_study;
pub mod registry;
