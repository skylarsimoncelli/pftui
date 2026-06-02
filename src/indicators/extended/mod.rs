//! Extended technical indicators (channel and trend-line family).
//!
//! Houses the channel/trend-line subset of the technicals expansion: a Gaussian-
//! filtered channel with σ-bands, a two-EMA zone channel, a volatility-weighted
//! trend line, and a Donchian midline trend (plus a hybrid blend with the
//! volatility-weighted trend). All functions are pure: they take a slice of
//! closes (and where needed highs/lows) and return scalar outputs or
//! `Option<f64>` series.
//!
//! Canonical TA terminology only — no vendor / brand names.

pub mod donchian;
pub mod gaussian_channel;
pub mod volatility_trend;
pub mod zone_channel;

pub use donchian::{compute_donchian_trend, hybrid_trend_blend, DonchianTrendConfig};
pub use gaussian_channel::{compute_gaussian_channel, GaussianChannelConfig};
pub use volatility_trend::{compute_volatility_trend, VolatilityTrendConfig};
pub use zone_channel::{compute_zone_channel, ZoneChannelConfig};
