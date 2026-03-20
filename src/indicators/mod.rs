//! Technical indicators engine.
//!
//! Pure functions that compute RSI, MACD, SMA, Bollinger Bands, ATR, and correlation
//! from price history slices (`&[f64]`). No I/O, no side effects — just math.

pub mod atr;
pub mod bollinger;
pub mod correlation;
pub mod macd;
pub mod rsi;
pub mod sma;

// Re-export primary types for convenience.
pub use macd::{compute_macd, MacdResult};
pub use rsi::compute_rsi;
pub use sma::compute_sma;
