//! Technical indicators engine.
//!
//! Pure functions that compute RSI, MACD, SMA, and Bollinger Bands from
//! price history slices (`&[f64]`). No I/O, no side effects — just math.
//!
//! Not yet wired into UI/CLI — consumed by upcoming F1.2–F1.4 tasks.

// These modules are fully tested but not yet consumed by the main binary.
// Suppress dead_code until F1.2+ integrates them into views/commands.
#[allow(dead_code)]
pub mod bollinger;
#[allow(dead_code)]
pub mod macd;
#[allow(dead_code)]
pub mod rsi;
#[allow(dead_code)]
pub mod sma;

// Re-export primary types for convenience.
#[allow(unused_imports)]
pub use bollinger::BollingerBands;
#[allow(unused_imports)]
pub use macd::MacdResult;
#[allow(unused_imports)]
pub use rsi::compute_rsi;
#[allow(unused_imports)]
pub use sma::compute_sma;
