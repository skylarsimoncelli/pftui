pub mod adversary_view;
pub mod gex;
pub mod capital_flows;
pub mod private_bottom_line;
pub mod private_conviction_trajectory;
pub mod private_cross_layer_signals;
pub mod private_parallels;
pub mod private_decisions_pending;
pub mod private_epistemic_health;
pub mod private_investor_panel;
pub mod private_closing;
pub mod private_external_ta;
pub mod private_macro_news_outlook;
pub mod private_operator_deep_dive;
pub mod private_overview;
pub mod private_lessons_applied;
pub mod private_macro_context;
pub mod private_mismatch_surface;
pub mod private_news_catalysts;
pub mod private_open_predictions;
pub mod private_outlook_by_horizon;
pub mod private_per_asset_convergence;
pub mod private_analytics_risk;
pub mod private_basket_allocation;
pub mod private_portfolio_snapshot;
pub mod private_risk_concentration;
pub mod private_self_retrospective_calibration;
pub mod private_synthesis;
pub mod private_upcoming_calendar;
pub mod public_allocation_framework;
pub mod public_bitcoin;
pub mod public_equities;
pub mod public_executive_summary;
pub mod public_gold_precious_metals;
pub mod public_how_we_analyse;
pub mod public_macro;
pub mod public_market_snapshot;
pub mod public_methodology;
pub mod public_news_catalysts;
pub mod public_scenario_dashboard;
pub mod real_rates_macro;
pub mod thesis_chains_macro;

/// Prefix of the suppression-reason marker a section renderer returns when
/// its empty-state condition fires. The assembler strips the marker (it
/// never reaches the report) and records the reason in the section-outcome
/// accounting + integrity footer, so an auto-suppressed section is always
/// explainable. Use [`suppressed`] to build one — never return a bare empty
/// string from a section's empty state.
pub const SUPPRESSED_PREFIX: &str = "<!-- suppressed: ";

/// Build a suppression marker carrying the renderer's empty-state reason.
/// Returned INSTEAD of an empty string from a section's auto-suppress path.
pub fn suppressed(reason: &str) -> String {
    format!("{SUPPRESSED_PREFIX}{} -->", reason.replace("-->", "—"))
}
