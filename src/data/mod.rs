#[allow(dead_code)] // Infrastructure for F12.1+ consumers (calendar CLI, Economy tab)
pub mod calendar;
#[allow(dead_code)] // Infrastructure for F18.1+ consumers (COT section, CLI)
pub mod cot;
#[allow(dead_code)] // Infrastructure for F3.2+ consumers (macro dashboard, refresh)
pub mod fred;
#[allow(dead_code)] // Infrastructure for F17.1+ consumers (Predictions panel, CLI)
pub mod polymarket;
pub mod predictions;
#[allow(dead_code)] // Infrastructure for F19.1+ consumers (sentiment gauges, Economy tab)
pub mod sentiment;
