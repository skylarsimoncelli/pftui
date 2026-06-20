//! Technical indicators engine.
//!
//! Pure functions that compute RSI, MACD, SMA, Bollinger Bands, ATR, and correlation
//! from price history slices (`&[f64]`). No I/O, no side effects — just math.

pub mod anchored_vwap;
pub mod atr;
pub mod bollinger;
pub mod correlation;
pub mod ema;
pub mod extended;
pub mod macd;
pub mod momentum;
pub mod rsi;
pub mod sma;
pub mod trend;
pub mod volume;

// Re-export primary types for convenience.
pub use ema::compute_ema;
pub use macd::{compute_macd, MacdResult};
pub use momentum::{compute_cci, compute_fisher, compute_roc, compute_stochastic, compute_williams_r};
pub use rsi::compute_rsi;
pub use sma::compute_sma;
pub use trend::{compute_adx, compute_supertrend};
pub use volume::{compute_mfi, compute_obv};
