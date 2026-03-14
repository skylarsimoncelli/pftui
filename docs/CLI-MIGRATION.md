# CLI Migration

F42 removes the old paths. Only the new canonical paths are supported.

| Old | New |
|---|---|
| `pftui portfolios list` | `pftui portfolio profiles list` |
| `pftui portfolios current` | `pftui portfolio profiles current` |
| `pftui portfolios create NAME` | `pftui portfolio profiles create NAME` |
| `pftui portfolios switch NAME` | `pftui portfolio profiles switch NAME` |
| `pftui portfolios remove NAME` | `pftui portfolio profiles remove NAME` |
| `pftui watchlist add ...` | `pftui portfolio watchlist add ...` |
| `pftui watchlist remove ...` | `pftui portfolio watchlist remove ...` |
| `pftui watchlist --json` | `pftui portfolio watchlist --json` |
| `pftui market news ...` | `pftui data news ...` |
| `pftui market sentiment ...` | `pftui data sentiment ...` |
| `pftui market calendar ...` | `pftui data calendar ...` |
| `pftui market fedwatch ...` | `pftui data fedwatch ...` |
| `pftui market economy ...` | `pftui data economy ...` |
| `pftui market predictions ...` | `pftui data predictions ...` |
| `pftui market options ...` | `pftui data options ...` |
| `pftui market etf-flows ...` | `pftui data etf-flows ...` |
| `pftui market supply ...` | `pftui data supply ...` |
| `pftui market sovereign ...` | `pftui data sovereign ...` |
| `pftui dashboard macro ...` | `pftui data dashboard macro ...` |
| `pftui dashboard oil ...` | `pftui data dashboard oil ...` |
| `pftui dashboard crisis ...` | `pftui data dashboard crisis ...` |
| `pftui dashboard sector ...` | `pftui data dashboard sector ...` |
| `pftui dashboard heatmap ...` | `pftui data dashboard heatmap ...` |
| `pftui dashboard global ...` | `pftui data dashboard global ...` |
| `pftui journal entry ...` | `pftui agent journal entry ...` |
| `pftui journal prediction ...` | `pftui agent journal prediction ...` |
| `pftui journal conviction ...` | `pftui agent journal conviction ...` |
| `pftui journal notes ...` | `pftui agent journal notes ...` |
| `pftui journal scenario ...` | `pftui agent journal scenario ...` |
| `pftui agent message send ...` | `pftui agent message send ...` |
| `pftui agent message list ...` | `pftui agent message list ...` |
| `pftui agent message reply ...` | `pftui agent message reply ...` |
| `pftui agent message flag ...` | `pftui agent message flag ...` |
| `pftui agent message ack ...` | `pftui agent message ack ...` |
| `pftui agent message ack-all ...` | `pftui agent message ack-all ...` |
| `pftui agent message purge ...` | `pftui agent message purge ...` |
| `pftui portfolio target set --symbol BTC --target 20` | `pftui portfolio target set BTC --target 20` |
| `pftui portfolio target remove --symbol BTC` | `pftui portfolio target remove BTC` |
| `pftui portfolio opportunity add "EVENT" ...` | `pftui portfolio opportunity add "EVENT" ...` |
| `pftui portfolio opportunity list ...` | `pftui portfolio opportunity list ...` |
| `pftui portfolio opportunity stats ...` | `pftui portfolio opportunity stats ...` |

Removed paths are not supported. There are no compatibility aliases.
