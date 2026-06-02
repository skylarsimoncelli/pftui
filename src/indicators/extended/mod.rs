//! Extended technical indicators (channels + signals subset).
//!
//! Houses the channel/trend-line subset (Gaussian channel with σ-bands, a
//! two-EMA zone channel, a volatility-weighted trend line, Donchian midline
//! trend, hybrid blend) plus the signals subset (multi-timeframe RSI, Pi Cycle
//! Top/Bottom, multi-timeframe breakout composite, Bollinger reversals, RSI
//! extreme highlighting). Each method owns its own sub-module so parallel
//! agents can add sub-modules without colliding. All functions are pure: they
//! take a slice of closes (and where needed OHLC arrays) and return scalar
//! outputs or `Option<f64>` series.
//!
//! Canonical TA terminology only — no vendor / brand names.

pub mod bollinger_reversal;
pub mod donchian;
pub mod gaussian_channel;
pub mod mtf_breakout;
pub mod mtf_rsi;
pub mod pi_cycle;
pub mod rsi_extreme;
pub mod volatility_trend;
pub mod zone_channel;

pub use bollinger_reversal::compute_bollinger_reversal;
pub use donchian::{compute_donchian_trend, hybrid_trend_blend, DonchianTrendConfig};
pub use gaussian_channel::{compute_gaussian_channel, GaussianChannelConfig};
pub use mtf_breakout::{compute_mtf_breakout, MtfBreakoutConfig};
pub use mtf_rsi::compute_mtf_rsi;
pub use pi_cycle::compute_pi_cycle;
pub use rsi_extreme::compute_rsi_extreme;
pub use volatility_trend::{compute_volatility_trend, VolatilityTrendConfig};
pub use zone_channel::{compute_zone_channel, ZoneChannelConfig};
