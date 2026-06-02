//! Extended technical indicators (channels + signals subset).
//!
//! Pure functions over closes (and where needed OHLC arrays). Each
//! indicator/signal owns its own sub-module so parallel agents can add
//! sub-modules without colliding. Re-exports are kept canonical (no
//! vendor/brand names) per CLAUDE.md.

pub mod bollinger_reversal;
pub mod mtf_breakout;
pub mod mtf_rsi;
pub mod pi_cycle;
pub mod rsi_extreme;

pub use bollinger_reversal::compute_bollinger_reversal;
pub use mtf_breakout::{compute_mtf_breakout, MtfBreakoutConfig};
pub use mtf_rsi::compute_mtf_rsi;
pub use pi_cycle::compute_pi_cycle;
pub use rsi_extreme::compute_rsi_extreme;
