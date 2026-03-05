#[allow(dead_code)] // Infrastructure for F24.1+ consumers (BLS indicators, Economy tab)
pub mod bls;
#[allow(dead_code)] // Infrastructure for F12.1+ consumers (calendar CLI, Economy tab)
pub mod calendar;
#[allow(dead_code)] // Infrastructure for F22.1+ consumers (COMEX supply panel, CLI)
pub mod comex;
#[allow(dead_code)] // Infrastructure for F18.1+ consumers (COT section, CLI)
pub mod cot;
#[allow(dead_code)] // Infrastructure for F3.2+ consumers (macro dashboard, refresh)
pub mod fred;
#[allow(dead_code)] // Infrastructure for F21.1+ consumers (on-chain panel, CLI)
pub mod onchain;
#[allow(dead_code)] // Infrastructure for F17.1+ consumers (Predictions panel, CLI)
pub mod polymarket;
pub mod predictions;
#[allow(dead_code)] // Infrastructure for F20.1+ consumers (News tab, CLI)
pub mod rss;
#[allow(dead_code)] // Infrastructure for F19.1+ consumers (sentiment gauges, Economy tab)
pub mod sentiment;
