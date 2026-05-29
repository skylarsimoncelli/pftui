# Completion Audit: PR #739 Post-Review Batch

Date: 2026-05-29

## Scope

This audit reconciles the 14 feature/fix commits that shipped after PR #739 against the original TODO contracts introduced by `3f9b549` ("todo: 19 new items from post-/pftui-report review"). The reviewed range starts after the TODO import and ends with the news-filter substrate batch shipped on 2026-05-28.

Audited commits:

1. `032b8d6` `feat: auto-pair transaction cash legs`
2. `8bcaeda` `fix: guard destructive set-cash replacements`
3. `f147704` `feat: add portfolio drawdown tracking`
4. `9a564be` `feat: preview transaction allocation impact`
5. `707ea47` `feat: track RSS feed health`
6. `83e85ab` `feat: model allocation targets as ranges`
7. `c4996bc` `feat: add prediction calibration rows for reports`
8. `e6cc9d5` `feat: surface applied prediction lessons`
9. `6d913f2` `feat: classify news source tiers`
10. `a0d9ff9` `feat: tag news source independence`
11. `c525462` `feat: track news source accuracy`
12. `0115c01` `feat: bind news topics to prediction markets`
13. `06e5d19` `feat: score narrative money divergence`
14. `cbc6b65` `feat: track news silence baselines`

## Summary

The audit found one stale follow-up, one real partial, and several gaps already represented by existing TODO entries.

- Stale follow-up removed: `portfolio set-cash` already refuses multi-row destructive replacement without `--confirm`, supports JSON refusal and dry-run paths, and warns/unlinks paired cash legs before deletion.
- New follow-up filed: calibration data exists as rows and routine text, but the original report visualization contract did not ship a native reliability/dot-plot chart or chart-level report integration.
- Existing follow-ups retained: news source-tier seed expansion, news topic classifier accuracy floor, scenario/prediction-market mapping auto-suggest, and historical-data backfill cover the real weaknesses in the news substrate batch.

## Findings

| Feature | Original contract | Shipped state | Classification | Follow-up |
| --- | --- | --- | --- | --- |
| Auto cash deduct/add on transaction add | Add paired cash legs for buys/sells, support FX cash currency, allow opt-out, make removal preserve pairing. | `src/commands/add_tx.rs` creates paired cash legs, supports `--cash-currency`, `--no-auto-cash`, and `--dry-run`; `src/commands/remove_tx.rs` deletes paired legs by default with `--unpaired` escape hatch; `src/commands/list_tx.rs` exposes pairing. | Complete | None |
| `portfolio set-cash` destructive safety | Preview rows that will be discarded and refuse destructive multi-row replacement without explicit confirmation. | `src/commands/set_cash.rs` computes `confirm_required` when more than one row would be discarded, returns structured JSON refusal, supports dry-run, and warns about paired cash rows before unlinking asset legs. Parser support lives in `src/cli.rs`; regression tests cover refusal, confirm, dry-run, JSON dry-run, and paired unlinking. | Complete; stale TODO removed | Removed stale P2 set-cash follow-up |
| Portfolio drawdown tracking | Add drawdown metrics to status and provide historical visibility. | `src/commands/drawdown.rs`, `src/commands/portfolio_status.rs`, and `src/commands/brief.rs` expose drawdown in command and report-facing status flows. | Complete | None |
| Transaction add dry-run and allocation/drift summary | Preview transaction effects and show post-add portfolio impact. | `src/commands/add_tx.rs` and transaction summary code implement dry-run and allocation-impact summaries; tests cover the preview path. | Complete | None |
| RSS news feed health | Track per-feed success/failure, disable failing feeds, surface status and fallback health. | `src/db/rss_feed_health.rs`, `src/commands/news.rs`, `src/commands/refresh.rs`, and `src/commands/status.rs` record success/failure, skip disabled feeds, and expose feed health. | Complete | None |
| Allocation targets as ranges | Replace point target semantics with floor/ceiling bands and update drift calculations. | `src/db/allocation_targets.rs`, `src/commands/target.rs`, `src/commands/rebalance.rs`, and `src/commands/transaction_summary.rs` implement floor/ceiling bands and drift/band position reporting. | Complete with already-filed adjacent follow-up | Existing P3 "Allocation target for cash position" covers cash-band extension |
| Calibration plot in daily report | Show confidence vs realised hit rate by layer/band in report output with sample-size context. | `src/commands/calibration.rs` implements `analytics calibration --by-layer --json` with sample sizes, 1 sigma uncertainty, low-sample flags, and conviction bins; routines consume the textual cells. No native report chart or reliability visualization was shipped. | Partial | New P2 "Calibration visualization follow-up" |
| Lessons applied this run | Surface applicable historical lessons during prediction writing and daily reporting. | `src/commands/lessons_applied.rs`, `src/db/user_predictions.rs`, prediction CLI flags, and analyst routines surface applied lessons and lesson IDs. | Complete | None |
| News source tier classification | Classify source tiers at ingest and expose/filter them to analysts. | `src/db/news_cache.rs` and `src/commands/news.rs` classify and emit `source_tier`, `source_tier_inferred`, and source filtering/management commands; routines now consume the metadata. | Complete with already-filed quality follow-up | Existing P2 source-tier seed expansion |
| Headline-only/source-independence tagging | Tag whether a news item is independent, wire copy, restatement, rumor, or headline-only style signal. | `src/db/news_cache.rs`, `src/commands/news.rs`, and `src/commands/news_sentiment.rs` classify and emit `source_independence`; routines weight independent vs duplicated sources differently. | Complete | None |
| Per-source historical accuracy tracking | Track source/topic accuracy as source-attributed predictions resolve. | `src/db/news_source_accuracy.rs`, `src/db/user_predictions.rs`, and analytics/news source ranking commands maintain forward source accuracy. | Complete with known forward-only limitation | Existing P3 historical-data backfill documents that retroactive source attribution is unreliable |
| News to prediction-market binding | Classify topics, bind topics to relevant prediction markets, and expose the binding to news consumers. | `src/db/news_topic_markets.rs`, `src/commands/news.rs`, and narrative-divergence paths bind topics to markets and expose `bound_markets`. | Complete with already-filed accuracy/mapping follow-ups | Existing P2 topic-classifier accuracy floor and prediction-market auto-suggest items |
| Narrative-vs-money divergence | Compare headline narrative pressure against prediction-market movement and score divergence. | `src/commands/narrative_divergence.rs` and related DB history code score aligned/divergent states and expose them to analytics and routines. | Complete with already-filed backfill follow-up | Existing P3 historical-data backfill for `narrative_money_history` |
| News silence/negative-space tracking | Track topic volume baselines and flag silence/saturation. | `src/commands/news_silence.rs` and `src/db/news_silence.rs` compute rolling baselines and emit silence/saturation signals; analyst routines now consume them. | Complete with already-filed backfill follow-up | Existing P3 historical-data backfill for `news_silence_baselines` |

## Decisions

The set-cash review note in the TODO was no longer accurate by the time this audit ran. The current implementation provides the safety property the note claimed was missing, so keeping that TODO would cause duplicate work and likely regress the simpler one-flag confirmation flow.

The calibration item is the only audited feature where the data substrate and text consumption shipped but the original report-visual contract did not. A focused P2 TODO now asks for a native reliability chart that consumes `analytics calibration --by-layer --json`, carries sample-size/uncertainty labels, and adds chart-level tests.
