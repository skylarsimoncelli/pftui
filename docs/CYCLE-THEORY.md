# CYCLE-THEORY.md — Market Cycle Theory: The Canonical pftui Reference

> **Status:** Reference document. Audience: (1) Rust engineers implementing the deterministic
> cycle engine, (2) analyst agents absorbing the doctrine and ethos of cycle trading.
> **This document contains NO trading advice.** It documents mechanics, measured history, and
> the operational beliefs of the cycle-trading schools — including where those schools disagree
> and where popular cycle folklore fails measurement.
>
> Engine conventions: all prices `rust_decimal::Decimal`; all time counts integer bars
> (trading days or weeks); every emitted value must be reproducible from OHLC history alone.

---

## Part I — Foundations

### 1. Why cycles: the core claim

All three schools studied here (Hurst, Bressert, the modern Loukas-style practitioners) share
one operational claim: **price movement contains a quasi-periodic component, anchored at
LOWS, that makes the *timing* of future lows statistically forecastable even when price
levels are not.** The cycle trader's edge is claimed to be in *when*, not *where*.

- J.M. Hurst (aerospace engineer, NASA-era signal processing) formalized this in
  *The Profit Magic of Stock Transaction Timing* (Prentice-Hall, 1970) and the ten-lesson
  *Cyclitec Cycles Course* (1973, republished as *J.M. Hurst Cycles Course*). Hurst claimed
  ~23% of price motion is oscillatory and semi-predictable, ~75% secular/fundamental trend,
  ~2% noise.
- Walter Bressert (first president of the CME's IMM market, cycle newsletter pioneer)
  operationalized cycles for futures traders in *The Power of Oscillator/Cycle Combinations*
  (1991) and *The Cycle Trading Pattern Manual* (1997–98, walterbressert.com).
- The modern practitioner school (Bob Loukas, *The Financial Tap* / *Bitcoin Live*; Gary
  Savage / Smart Money Tracker; Graddhy; et al.) descends from Bressert via futures cycle
  trading, simplified to two or three tradable degrees and popularized for gold and Bitcoin.

### 2. Hurst's eight principles

Source: Hurst, *Profit Magic* ch. 2–3; restated in the Cycles Course; summarized at
[Sentient Trader: Hurst's Cyclic Theory Principles](https://sentienttrader.com/downloads/SentientTraderHelp/HurstCyclicTheoryPrinciples.htm)
and [Sigma-L](https://www.sigma-l.net/p/hurst-nominal-model).

1. **Commonality** — All instruments' price movements share many elements; in particular,
   cyclic lows tend to occur near-simultaneously across related markets. (Engine implication:
   cross-asset low clustering is evidence FOR a phasing, not coincidence.)
2. **Cyclicality** — Price movement is composed of identifiable waves; it exhibits cyclic
   (not strictly periodic) character.
3. **Summation** — The waves combine by **simple addition** to produce price. Price ≈
   Σ cycles + underlying trend + noise. (This is what licenses detrending: subtract one
   cycle's smooth to see the shorter ones.)
4. **Harmonicity** — Adjacent waves' wavelengths are related by small integer ratios —
   normally **2:1**, occasionally **3:1** (the 54-month → 18-month link).
5. **Synchronicity** — Waves are phased so that **troughs coincide wherever possible**.
   Lows of a longer cycle are simultaneously lows of every shorter cycle. Peaks do NOT
   synchronize (they're displaced by the underlying trend). This is why ALL schools anchor
   on lows.
6. **Proportionality** — Amplitude is proportional to wavelength: longer cycles move price
   further. (Engine implication: expected envelope width scales with degree.)
7. **Nominality** — A specific nominal collection of harmonically related waves is common to
   all price movements (the "nominal model" below). Actual markets deviate; the model is
   the prior, not the law.
8. **Variation** — Principles 4–7 are *strong tendencies*, not constants. Real wavelengths
   and amplitudes drift around the nominal values. (Engine implication: every length is a
   distribution, never a scalar. This principle is the license for timing BANDS.)

### 3. The nominal cycle model

Hurst's nominal model (Cycles Course; table reproduced at
[Sigma-L: The Nominal Model](https://www.sigma-l.net/p/hurst-nominal-model) and in the
[MotiveWave Hurst Cycles Guide](https://www.motivewave.com/guides/MotiveWave_Hurst_Cycles.pdf)):

| Degree (name) | Nominal avg wavelength | Harmonic to next |
|---|---|---|
| 18-year  | 17.93 years (≈ 6,547 d) | 2× the 9-year |
| 9-year   | 8.96 years | 2× the 54-month |
| 54-month | 53.77 months (≈ 4.5 y) | **3×** the 18-month |
| 18-month | 17.93 months | 2× the 40-week |
| 40-week  | 38.97 weeks | 2× the 20-week |
| 20-week  | 19.48 weeks (≈ 136 d) | 2× the 80-day |
| 80-day   | 68.2 days | 2× the 40-day |
| 40-day   | 34.1 days | 2× the 20-day |
| 20-day   | 17.0 days | 2× the 10-day |
| 10-day   | 8.5 days | 2× the 5-day |
| 5-day    | 4.3 days | — |

Notes the engine must respect:
- The **names are round numbers; the averages are not** (a "20-week" cycle nominally runs
  ~19.5 weeks). Names are labels for degrees, not measurements.
- The single 3:1 link (54m/18m) breaks pure octave structure; modern variants (Sentient
  Trader default) sometimes use slightly different averages. Treat the table as a **prior**
  to be re-estimated per asset (Principle 8).
- **Principle of Nominality bridge:** when an asset's measured dominant cycle is, say, 60
  days, it is *assigned* to the nearest nominal degree (80-day or 54-day harmonic slot) —
  the asset-specific measured mean is what drives bands; the nominal slot drives nesting.

### 4. The three schools at a glance

| | Hurst | Bressert | Loukas-style modern |
|---|---|---|---|
| Degrees tracked | ALL degrees simultaneously (full phasing) | 4 dominant cycles, practically 2 (trading + primary) | 2–3: daily cycle (DC), weekly/investor cycle (IC), 4-year |
| Anchor event | Troughs of every degree | Trough-to-trough Trading Cycle | Cycle low (DCL/ICL/4YCL) |
| Timing tool | Envelopes, FLD, VTL | Timing bands (70% containment) + oscillator | Timing bands + swing-low confirmation |
| Trend filter | Sum of longer cycles (underlying trend) | Translation + primary-cycle direction | Translation + failed-cycle logic |
| Confirmation | FLD cross / VTL break | Mechanical oscillator signal in band | Swing low + moving-average reclaim |
| Falsifier | Phasing revision | Low outside band, signal failure | Failed cycle (lower low), count reset |

---

## Part II — Phasing & Measurement

### 5. Phasing analysis (Hurst)

Phasing = determining, for every degree, **how long it has been since that degree's last
trough**. Hurst's process (Profit Magic ch. 5–7; Cycles Course lessons 4–6; summarized at
[Hurst Cycles Notes: Phasing Analysis](https://notes.hurstcycles.com/phasing-analysis/)):

1. **Entry:** find the visually/statistically dominant long cycle's troughs (the unmistakable
   major lows).
2. **Extension:** work *downward* through shorter degrees, using longer-degree troughs as
   anchors — by synchronicity, every long-degree trough is also a trough of every shorter
   degree.
3. **Completion:** after the shortest degree is resolved, work back *up* to confirm longer
   placements.
4. Notation: piles of **diamonds** under price; taller pile = longer degree troughing there.
5. Ambiguity rule: when a trough's placement is unclear, **defer** — do not force a phasing.
   ("Cycle clarity" in modern terms; see §15.)

Hurst's original mechanical aid was the **envelope**: constant-width channels drawn around
centered moving averages of each degree; channel pinches/troughs locate cycle troughs
(see §10). Peaks are second-class citizens: trend displacement makes peak timing unreliable;
**all phasing anchors are troughs**.

### 6. Measurement doctrine (all schools)

- **Cycles are measured LOW to LOW.** (Bressert, *Cycle Trading Pattern Manual* p.2:
  "Cycles are measured from bottom to bottom." Loukas glossary, The Financial Tap.)
- The **cycle top** is simply the highest price between two adjacent lows of the same degree —
  it is an output, not an anchor.
- Each degree's length is a **distribution**: maintain mean, median, σ, min, max, and the
  central ~70% interval of observed low-to-low lengths (Bressert's containment statistic).
- Bressert: "Every time frame of every market has a dominant Trading Cycle averaging from
  14 to 25 bars... most cluster in the 18 to 22 bar range, averaging 20 bars from bottom to
  bottom" (*Cycle Trading Pattern Manual*, p.2).
- Lengths **drift across regimes** (Principle 8): the engine should compute statistics on a
  trailing window (e.g., last 8–12 completed cycles) AND full history, and surface both.

---

## Part III — The Computable Toolkit

Exact algorithms. All are deterministic given (OHLC series, parameter set). Bars are trading
bars unless an asset trades 7d/week (crypto: calendar days).

### 7. Pivot / zigzag cycle-low detection

The primitive every other tool depends on. Two interchangeable detectors; the engine should
implement both and let degree-config choose.

**7a. Rolling-window pivot (fractal) detector**

```
pivot_low(i, w):  low[i] == min(low[i-w ..= i+w])      // strict window minimum
pivot_high(i, w): high[i] == max(high[i-w ..= i+w])
```
- `w` = round(degree_avg_len / 4) is a sane default (a 60-day cycle → w≈15).
- A pivot is only *final* once `w` bars have printed after it (right-edge lag is inherent —
  this is why confirmation tools in §13–14 exist).
- Tie-break: if two equal lows, take the later bar.

**7b. ZigZag (reversal-threshold) detector**

```
state = seeking_low | seeking_high
seeking_low:  track running_min; if price rises ≥ θ% above running_min → emit low, flip
seeking_high: track running_max; if price falls ≥ θ% below running_max → emit high, flip
```
- θ per degree/asset (e.g., BTC daily cycle θ≈8–12%; gold daily cycle θ≈3–4%; investor-cycle
  degree θ larger). θ should be calibrated so that detected low count ≈ history_len /
  degree_avg_len.
- Alternating sequence guaranteed (low, high, low, ...). Lows feed the length statistics.

**Degree assignment:** run the detector per degree with that degree's `w`/θ. Hurst
synchronicity check (§12) then enforces consistency between degrees.

### 8. Low-to-low statistics and timing bands

Given confirmed lows `L1..Ln` of one degree, lengths `d_i = bars(L_i, L_{i+1})`:

- `mean`, `median`, `sd`, `min`, `max` over all `d_i` (and over trailing window).
- **Gaussian band:** `[mean − k·sd, mean + k·sd]`, k = 1.0 (core band, ≈68% if normal —
  matches Bressert's "70% accuracy" claim) and k = 2.0 (outer band).
- **Bressert-style empirical containment band:** sort `d_i`; band = central 70% interval
  `[P15, P85]` (percentiles of the empirical distribution). Bressert: "cycle timing bands...
  forecast time periods for cycle tops and bottoms with 70% accuracy" (*Cycle Trading
  Pattern Manual*, p.10–11). He computed separate **bottoming bands** (from low-to-low
  stats) and **topping bands** (from low-to-HIGH stats: distribution of `bars(L_i, H_i)`).
- **Current position:** `cycle_age = bars_since_last_confirmed_low`. Status:
  - `pre_band` (age < band_lo), `in_band` (band_lo ≤ age ≤ band_hi),
    `over_band` (age > band_hi → either the low printed unconfirmed, or count is wrong,
    or a stretch is in progress — flag for review).
- Next-low projection: `[last_low_date + band_lo, last_low_date + band_hi]` — a **window,
  never a date**. Same for next-top window using the topping-band stats, valid only while
  translation regime persists.

### 9. Translation

The single most load-bearing derived metric in Bressert/Loukas doctrine.

```
for completed cycle i (low L_i .. low L_{i+1}, top H_i between them):
    translation_pct = bars(L_i, H_i) / bars(L_i, L_{i+1})        // 0..1
    classification:  RT if translation_pct > 0.5 + ε
                     LT if translation_pct < 0.5 − ε
                     MID otherwise            (ε ≈ 0.05 recommended)
```

Semantics (Bressert, *Cycle Trading Pattern Manual* pp. 9–10; Loukas,
[The Financial Tap glossary](https://thefinancialtap.com/support/glossary/)):
- **Right translation** (top past midpoint): price spends more time rising than falling →
  the cycle exists inside a rising larger-degree cycle. Bull signature. Bressert: in RT, on
  a 20-day cycle "the move from bottom to top will be three weeks, and the move from top to
  bottom, one week."
- **Left translation** (top before midpoint): bear signature; expect the decline phase to be
  long and to threaten the prior cycle low.
- **Predictive use:** translation of the *just-completed* and *current* cycle is the trend
  filter for the *next* cycle. A string of RT cycles = uptrend; the first LT cycle after a
  string of RTs is the canonical early warning that the larger degree has topped.
- Translation of the LARGER degree predicts whether the current smaller-degree decline makes
  a higher or lower low (Bressert "the direction of the primary cycle sets trend for the
  trading cycle," p.12).

### 10. Detrending: centered moving average + envelope

Bressert's centered detrend (*Cycle Trading Pattern Manual* pp. 3–4), identical in spirit to
Hurst's envelope construction:

```
N = degree_avg_len (e.g., 20 bars)
cma[i] = SMA(close, N) plotted at bar i − floor(N/2)        // centered
detrend_high[i] = high[i] − cma[i]
detrend_low[i]  = low[i]  − cma[i]                          // plot around zero line
```
- Cycle lows/highs of degree N are obvious extremes in the detrend. Bressert thresholds on
  his normalized version: tops mostly above +0.80, bottoms below −0.80; ±2.0 = imminent
  extreme.
- **The centered detrend lags by floor(N/2) bars** — it is an *analysis/calibration* tool,
  not a real-time signal (Bressert is explicit, p.4). Real-time proxy = oscillator (§14).
- **Hurst envelope:** `upper/lower = cma ± h` where `h` = constant width set so the envelope
  contains ~all of the degree's oscillation (e.g., 1.5–2× the mean |detrend| or a fitted
  constant). Nested envelopes of successive degrees visualize summation; envelope troughs
  locate cycle troughs; the unclosed final half-span must be **extrapolated** to the right
  edge — the quality of everything Hurst does at the live edge depends on that extrapolation
  (Profit Magic ch. 5; [Hurst envelopes overview](https://notes.hurstcycles.com/phasing-analysis/)).
- Hurst's **half-span moving average**: the CMA of *half* the degree's span, displaced; its
  turning points lead the full-span CMA and mark mid-channel crossings — used to refine
  trough timing (Profit Magic ch. 6).

### 11. FLD — Future Line of Demarcation

Hurst's signature live-edge tool (Profit Magic ch. 5; Cycles Course; modern specs at
[Sentient Trader FLD help](https://sentienttrader.com/downloads/SentientTraderHelp/FLD.htm),
[Hurst Cycles Notes: FLDs](https://notes.hurstcycles.com/flds/)).

**Construction:**
```
median_price[i] = (high[i] + low[i]) / 2
offset          = floor(degree_avg_len / 2)          // Sentient adds +1 for median FLD
fld[i + offset] = median_price[i]                    // price displaced FORWARD half a cycle
```
The FLD needs no smoothing — displacement by half a wavelength makes the same-degree cycle
component cancel against price at crossings.

**Cross semantics:**
- Price closes **above** the FLD from below → **confirms a trough** of that degree occurred
  (price is ~halfway to the cycle peak).
- Price closes **below** the FLD from above → **confirms a peak** of that degree (price is
  ~halfway to the trough).
- A falling FLD beneath rising price ("future impetus") implies the degree's pressure is
  still up, and vice versa.

**Target projection** (the "FLD measured move"):
```
upside:   target = cross_price + (cross_price − trough_price)
downside: target = cross_price − (peak_price − cross_price)
```
i.e., the move from the extreme to the FLD cross is doubled. Interpretation of error: target
overshoot ⇒ longer degrees pushing the same direction; undershoot ⇒ longer degrees opposing
([Hurst Cycles Notes: FLDs](https://notes.hurstcycles.com/flds/)). The engine emits target,
achieved %, and overshoot/undershoot as a longer-degree pressure diagnostic.

### 12. VTL — Valid Trend Line

(Profit Magic ch. 7; [Sentient Trader VTL help](http://www.sentientcode.com/downloads/SentientTraderHelp/VTL.htm);
[Hurst Cycles Notes: VTLs](https://notes.hurstcycles.com/valid-trendlines-vtls/).)

**Construction:** a straight line through **two consecutive troughs of the SAME degree**
(uptrend VTL), or two consecutive peaks of the same degree (downtrend VTL).
Validity rules:
1. The line may not cut through price between its two anchor points.
2. No trough of a LONGER degree may lie between the two anchors.
3. (Practice) anchor on closes or lows consistently; close-based breaks are stronger
   (Bressert p.23 independently: "a close below a close-chart trendline is much more
   significant").

**Break semantics — the key theorem:** price crossing below a VTL drawn on the troughs of
degree N **confirms that the PEAK of degree N+1 (one degree longer) is in place**.
Symmetrically, crossing above a peak-VTL of degree N confirms the trough of degree N+1.
This is the Hurst-school analogue of the Loukas "failed cycle ⇒ larger degree has turned"
rule, but it triggers EARLIER (no lower low required). Bressert uses the same construction
informally: "two-uptrend lines drawn across Trading Cycle bottoms and their penetration
confirms the top of the larger weekly cycles" (*Cycle Trading Pattern Manual*, p.22).

### 13. Cycle confirmation, failure, and inversion (Loukas-school formalisms)

Source: [The Financial Tap — What Are Market Cycles](https://thefinancialtap.com/what-are-market-cycles/),
[glossary](https://thefinancialtap.com/support/glossary/); Gary Savage terminology at
[Smart Money Tracker Premium](https://www.smartmoneytrackerpremium.com/terminology/);
[Graddhy market cycles](https://www.graddhy.com/pages/market-cycles).

- **Swing low** (new-cycle confirmation primitive): forms when price exceeds the HIGH of the
  bar that holds the candidate cycle low. `swing_low_confirmed(i_low) := exists j > i_low
  with high[j] > high[i_low] (and low[j] > low[i_low])`. A candidate DCL inside the timing
  band + a swing low + (optionally) reclaim of a fast MA (e.g., 10-day) = **confirmed cycle
  low**; the count resets to day 0.
- **Failed cycle:** after a new cycle is underway, price trades **below the low that started
  the cycle** (`low < cycle_origin_low`). Doctrine: "typically signals (in about 80% of
  cases) that the more dominant cycle is in decline" (The Financial Tap). Failed smaller
  cycles are the *expected texture* of a larger-degree decline; strings of failed cycles =
  bear market. A failed cycle also retroactively forces LT classification scrutiny.
- **Cycle inversion** (rare, contentious): a high prints where the count expected a low or
  vice versa — i.e., the half-cycle structure flips ([airlovsky on cycle inversions](https://www.airlovsky.com/cycle-inversions/);
  Savage uses it for metals). Detection heuristic: expected-low window passes with price at
  a local HIGH and the subsequent decline bottoms ~half a cycle later. Doctrine split: Loukas
  rarely invokes inversion (prefers stretched/failed counts); Savage invokes it for gold
  regularly. **Engine must flag the configuration, not adjudicate the school** (see Part VI).
- **Half-cycle low (HCL):** the mid-cycle dip. Bressert: "almost all trading cycles have a
  ½ trading cycle... a 20-day cycle has within it two 10-day cycles. One bottoms halfway
  into the 20-day cycle" (*Cycle Trading Pattern Manual*, p.9) — i.e., the HCL **is** the
  trough of the next degree down (harmonicity makes the two framings identical). Detection:
  most prominent pivot low in the window `cycle_age ∈ [0.35, 0.65] × expected_len` that
  holds above the cycle origin low (else it's a failure, not an HCL). Bressert adds the
  stretch warning: the two half-cycles trade length (7+13, 4+11...) and a 20-day cycle
  contracting to ~15 can make the half-cycle "seem to disappear" (p.10).
- **Mid-cycle pause:** the flat consolidation around the centered MA / FLD as the half-cycle
  components cancel — visible as price hugging the FLD near mid-cycle. Often resolves into
  the HCL.

### 14. Oscillator confirmation (Bressert's layer)

Doctrine: **cycle lows are TIMING anchors; oscillators CONFIRM** — never the reverse. "It is
the combination of time, price and oscillators that allows the early identification of cycle
tops and bottoms" (Bressert). Mechanics from *The Cycle Trading Pattern Manual* pp. 5–8, 24:

- Oscillator selection criteria (p.5): turns when price turns; minimal wiggle at extremes;
  reaches range extremes at cycle tops/bottoms. Period should be tuned to the degree
  (classically ≈ half the cycle length for RSI/stochastic).
- **RSI3M3**: RSI(3) smoothed with SMA(3). Buy line 30, sell line 70 (normalized variant:
  ±0.80). Mechanical buy: (1) oscillator < buy line inside the bottoming band, (2) oscillator
  turns up — that bar is the **setup bar**, (3) entry stop one tick above setup-bar high,
  (4) protective stop one tick below the cycle low. Mirror for tops.
- **Detrended oscillator**: `RSI3M3 − SMA(RSI3M3, 5)`; more sensitive, catches half-cycle
  lows and trading-cycle lows in strong trends that the raw oscillator misses (p.8).
- Same template works with CCI, stochastic, MACD-detrend, "3-10" oscillator (p.24).
- **The band gates the signal:** an oscillator buy outside the bottoming band is ignored; a
  price low in the band without an oscillator/swing confirmation is unconfirmed. Both keys
  must turn. This two-key rule is the heart of Bressert doctrine.
- Supporting filters Bressert stacks (pp. 12–24): primary-cycle (one-degree-longer) direction
  as trend filter; 38–62% Fibonacci retracement zone for trading-cycle bottoms in uptrends
  (close below 62% retr. = trend-reversal warning); EMA pair + weekly MACD trend indicators;
  Keltner channel (5-week SMA ± 1.1 σ) — upper-band test then dip below midline but above
  lower band = classic RT-cycle buy zone.

### 15. Nested-degree consistency (synchronicity checks)

By Hurst synchronicity + harmonicity, the engine must verify, for each adjacent degree pair
(short S, long L, ratio r = len_L/len_S ∈ {2,3}):

1. **Coincidence:** every confirmed L-degree low has an S-degree low within
   `±round(len_S/4)` bars. Violation ⇒ one of the two phasings is wrong.
2. **Count:** number of S-degree lows strictly inside one completed L-cycle ≈ r−1
   (e.g., one 10-day low inside each 20-day cycle; one DCL mid-IC plus one terminal).
   Tolerance ±1 (variation principle).
3. **Terminal failure texture:** the FINAL S-cycle inside a topped L-cycle is expected to be
   LT and/or failed. A failed S-cycle while the L-count says "early, rising" is a
   contradiction → lower the L-count's clarity grade.
4. **Clarity grading** (Loukas's practice, glossary): per degree emit
   `clarity ∈ {green, amber, red}` — green: single coherent count passing checks 1–3;
   amber: a favored count plus a viable alternate; red: contradictory counts. Doctrine:
   red-clarity counts are not acted on.

---

## Part IV — Asset-Specific Bodies of Knowledge

### 16. Bitcoin

**16.1 The 4-year cycle, two framings.** The same phenomenon is counted two ways:

- **Halving clock** (supply-event framing): halvings 2012-11-28, 2016-07-09, 2020-05-11,
  2024-04-19 (next ≈ 2028-03). Measured tops after halving: **367 d** (2013-11-30, ~$1.1k),
  **526 d** (2017-12-17, $19.8k), **547 d** (2021-11-10, $69k), **~535 d** (2025-10, ~$126k,
  then ~50% drawdown — reported by [PANews/HTX cycle retrospectives](https://www.panewslab.com/en/articles/019eab6f-6b1c-767f-b1ab-bf6bdcba7afe)).
  Ex-2013, the top window is remarkably tight: **480–550 days post-halving**. Bear lows have
  landed ~500–550 days *before* the next halving (2022-11-21 low ≈ 515 d before the 2024
  halving). Sources: [Swan halving dates](https://www.swanbitcoin.com/education/bitcoin-halving-dates/),
  [Bitcoin Suisse halving & market cycle](https://bitcoinsuisse.com/learn/bitcoin-halving-market-cycle).
- **Loukas low-to-low framing** ([Bitcoin Live, "Bitcoin's 4-Year Journey"](https://bitcoin.live/4yearcycle)):
  the 4-year cycle is counted low-to-low like any other degree: 2011-11 → 2015-01-14 ($152)
  → 2018-12-15 ($3.1k) → 2022-11-21 ($15.5k) → next. Spacings ≈ 38, 47, 47 months. RT
  structure so far in every completed 4-year cycle (~32–38 months up); an LT 4-year cycle
  (peak before month ~24) would be the regime-break signature. Loukas treats the halving as
  a *narrative correlate*, not the mechanism; the count stands on lows alone.

**16.2 Nesting (Loukas):** **60-day "daily cycle"** (DCL roughly every 54–66 days;
popularized by Loukas, mechanics summarized at
[Whaleportal: Bitcoin 60-Day Cycle](https://whaleportal.com/blog/bitcoin-60-day-cycle-explained/))
→ **weekly/investor cycle** of ~20–24 weeks (Loukas refers to a "24-week cycle low"; 3–4
DCs per IC) → ~12–16 ICs per 4-year cycle (~24 DCs per 4-year cycle per Whaleportal).
Translation cascades exactly as in §9: DC translation reads IC health; IC translation reads
4-year health. Confirmation: swing low + 10-day MA reclaim inside the band (§13).

**16.3 Pi Cycle Top** (Philip Swift, 2019;
[Bitcoin Magazine Pro chart](https://www.bitcoinmagazinepro.com/charts/pi-cycle-top-indicator/)):
```
signal: SMA(close, 111)  crosses ABOVE  2 × SMA(close, 350)
```
(350/111 ≈ 3.153 ≈ π — numerology, acknowledged even by fans.) Track record: flagged the
2013 double-top peaks (Apr & Dec 2013), fired 2017-12-17 — 3 days before the top — and
2021-04-12 — within days of the April $64.8k top, but **the final 2021 high ($69k, Nov) came
with NO second signal**, and **it did not fire at all in the 2025 cycle** (no retail
blow-off; institutional flows ground the MAs together —
[checkonchain](https://charts.checkonchain.com/btconchain/pricing/pricing_picycleindicator/pricing_picycleindicator_light.html),
[PANews retrospective](https://www.panewslab.com/en/articles/019eab6f-6b1c-767f-b1ab-bf6bdcba7afe)).
Classification: **useful curiosity with a 2/4 clean record — emit it, never gate on it.**

**16.4 Pi Cycle Bottom** (community variant;
[tradingdigits](https://www.tradingdigits.io/pi-cycle-bottom-indicator),
[Trader Dončić write-up](https://traderdoncic.medium.com/pi-cycle-bottom-indicator-for-bitcoin-ethereum-4d06cc268145)):
```
signal: EMA(close, 150)  crosses BELOW  0.745 × SMA(close, 471)     (471 ≈ 150π)
```
Fired 2015-01-16 (2 days after the $152 low) and 2018-12-16 (1 day after $3.1k). In 2022 it
fired ~July 13 near $19k — **four months and ~18% above the actual 2022-11-21 low** ($15.5k)
([BeInCrypto coverage](https://beincrypto.com/pi-cycle-bottom-is-flashing-has-bitcoin-btc-bottomed-out/)).
Two clean hits, one miss, n=3. Same classification as the top variant.

**16.5 Diminishing amplitude:** halving-day-to-peak multiples: ~103×, ~30×, ~8×, ~2×
([Simianx halving cycles reference](https://www.simianx.ai/stories/bitcoin-halving-cycles-2012-2028)).
Proportionality (Principle 6) in reverse as market cap grows. Time structure has been far
more stable than price structure — which is precisely the cycle-school claim.

### 17. Gold (and silver)

**17.1 The 8-year folklore.** The "gold 8-year cycle" (3 years up / 5 years complex down;
lows cited near 1976, 1985, 1993, 2000–01, 2008, 2015–16) is a McClellan-school staple
([McClellan: Gold and Dollar share an 8-Year Cycle](https://www.mcoscillator.com/learning_center/weekly_chart/gold_and_dollar_share_an_8-year_cycle_period/)),
often tied to an inverse dollar cycle.

**17.2 What measurement says.** On ~26 years of daily data, pftui's own anchor verification
places the major lows at **2008-11-13, 2015-12-17, 2022-09-26** — spacings **7.09y and
6.78y, mean ≈ 6.9 years**, not 8. Independent critics reach the same verdict on
actionability: the 8-year label "was not helpful during the bull market in the 2000s nor
the 1970s" ([Investing.com: Why Gold's 8-Year Cycle Isn't Actionable](https://www.investing.com/analysis/why-golds-8year-cycle-isnt-actionable-200614566)).
**Engine rule: use the measured ~6.9y mean and its band from the three verified anchors
(wide σ — only 2 intervals!), label the 8-year number as folklore, and grade long-degree
clarity amber at best.** Some analysts instead pair lows into a ~16-year structure
([GoldBroker: a 16-year gold cycle?](https://goldbroker.com/news/sixteen-year-gold-cycle-3125)) —
with n≈2 that is unfalsifiable; record, don't compute.

**17.3 Tradable degrees (Loukas/Savage practice):** gold **daily cycle ≈ 24–28 trading
days** (band roughly 22–30); **investor cycle ≈ 16–22 weeks** (stretch 22–26); the multi-year
degree above ([The Financial Tap](https://thefinancialtap.com/what-are-market-cycles/)).
Dollar DC 16–22 days, and gold cycle lows habitually coincide with dollar cycle highs
(commonality, Principle 1, in inverse form). Savage-school terms the IC decline into the
low the "Intermediate Cycle Low decline" and uses inversions liberally for metals — a school
disagreement to surface, not resolve.

**17.4 Silver:** no independent cycle body of knowledge — doctrine across schools is that
silver phases WITH gold (commonality) with higher amplitude (proportionality + beta) and
slightly later, sharper lows. Engine: run silver's own counts but report gold-silver low
offset as a consistency diagnostic.

### 18. Equities — the 4-year / Kitchin cycle

- **Kitchin (1923):** ~40-month inventory cycle found in bank clearings, commodity prices,
  interest rates 1890–1922 ([Cycles Research Institute: Kitchin](https://cyclesresearchinstitute.org/cycles-research/economy/kitchin/)).
  Modern support: inventory-driven models generate ~40-month GDP cycles explaining up to
  ~20% of U.S. GDP variance (Khan & Thomas 2007, cited in
  [Grokipedia: Kitchin cycle](https://grokipedia.com/page/Kitchin_cycle)).
- **The equity 4-year ("presidential") cycle:** strong 1868–1945 and famously regular
  1949–2000 (lows 1949, '53, '57, '62, '66, '70, '74, '78, '82, '87, '90, '94, '98, 2002);
  it then stretched badly (2002 → 2009 → 2011? → 2016? → 2020 → 2022) — the post-2000
  record is the textbook case of **cycle drift** ([ScienceDirect: presidential cycle puzzle](https://www.sciencedirect.com/science/article/abs/pii/S0261560613001721)).
  Hurst's 54-month nominal degree is the same slot.
- **Tradable degrees:** equities DC ≈ 36–42 trading days; IC ≈ 16–22 weeks (The Financial
  Tap). The 4-year low = IC low of a 9–12-IC sequence, classically accompanied by a failed,
  LT final IC.
- Verdict: the ~40-month/4-year slot has the best *academic* support of any cycle in this
  document, but with σ large enough that only band logic — never date logic — is defensible.

---

## Part V — The Ethos: What Cycle Traders Actually Believe

The doctrine, distilled from Hurst's books, Bressert's manual, and Loukas's published
framework. These are operational beliefs of the school — recorded for analyst-agent
calibration, not endorsed as fact.

1. **You trade TIME windows, not price targets.** The entire output of the discipline is
   "a low is statistically due in this window." Price targets (FLD projections) are
   secondary cross-checks. Bressert titled the manual's first page "TIMING IS EVERYTHING."
2. **The low is the only anchor — and for the modern school, the only tradeable event.**
   Lows synchronize (Hurst P5); tops drift with trend. Every count, band, and confirmation
   keys off lows. Loukas: the cycle low is where risk can be defined (stop below the low);
   everything else is position management.
3. **Translation tells you the trend.** RT = the larger degree is rising; LT = it has
   turned. The first LT cycle after RTs is the earliest structural top warning; strings of
   failed/LT cycles are what a bear market IS, in cycle language.
4. **Confirmation over prediction.** A window without a swing low + band position +
   oscillator turn is a forecast, not an event. Two keys (time + confirmation) must turn.
   Hurst's versions of the second key are the FLD cross and the VTL break.
5. **A count is falsifiable, and must be killed when falsified.** Canonical invalidations:
   low arrives outside the band (count wrong or regime changed); **failed cycle** (price
   below cycle origin after an upturn — the larger degree has rolled); synchronicity
   contradiction between degrees. The school's self-respect rests on resetting counts
   instead of bending them.
6. **Position across the band, not at a point.** Because the band is a distribution, the
   practitioner scales in across the timing window / on confirmation stages rather than
   committing at a single predicted date. (Mechanical corollary of P8 — variation.)
7. **Trade with the degree above.** Bressert's "Holy Grail": the primary (next-longer)
   cycle's direction sets which signals you may take on the trading cycle (buy signals in
   downtrends lose — manual p.17).
8. **The honest critiques, which good practitioners concede:**
   - **Lengths drift** across regimes (equity 4-year post-2000; BTC amplitude decay). A
     band fitted to the last regime quietly degrades.
   - **Phasing is subjective at the margin.** Different competent analysts produce different
     counts from the same chart; "clarity" grading is the school's own admission
     ([Hurst forum on non-subjective analysis](https://forum.hurstcycles.com/t/a-non-subjective-approach-to-hurst-analysis/295)).
   - **Unfalsifiability pressure:** inversion/stretch/translation vocabulary can rationalize
     any outcome ex post if discipline slips ([AlgoStorm critique](https://algostorm.com/cycle-theories/)).
     The defense is mechanical invalidation rules (point 5) enforced by software, not mood.
   - **Survivorship in folklore:** gold "8-year" (measures ~6.9y), Pi Cycle's n=2–4 record,
     halving numerology — celebrated hits, unpublished misses. Anything with n < ~8
     completed cycles deserves an explicit small-n flag.
   - **Spectral reality check:** rigorous spectral analysis of returns finds weak, unstable
     periodicities; the cycle trader's reply is that bands+confirmation only require
     *quasi*-periodicity at lows. Both statements can be true; the engine should never
     claim more than the second.

---

## Part VI — Mechanical Outputs Spec (what the deterministic engine emits per asset)

Per asset, per configured degree (BTC: 60d DC / 20–24w IC / 4y; gold & silver: 24–28d DC /
16–22w IC / ~6.9y measured long degree; equities: 36–42d DC / 16–22w IC / ~40m Kitchin),
the engine emits one deterministic record. All fields reproducible from OHLC + config; no
discretion at runtime. Schools' disagreements are surfaced as parallel fields, never merged.

```
cycle_status {
  asset, degree, as_of,

  // Count
  last_confirmed_low: {date, price},        // pivot + swing-low confirmed (§7, §13)
  cycle_age_bars,                           // current day/week count
  candidate_low: {date, price} | null,      // pivot printed, swing not yet confirmed

  // Band
  band: {mean, sd, p15, p85, n_cycles, window},   // both gaussian & empirical (§8)
  band_position: pre_band | in_band | over_band,
  next_low_window: {start_date, end_date},
  topping_band: {p15, p85} , next_top_window,     // low-to-high stats (§8)

  // Translation
  current_top: {date, price, translation_pct} | null,
  last_n_completed: [{len_bars, translation_pct, class: LT|MID|RT, failed: bool}; N],

  // Hurst tools
  fld: {value, price_side: above|below, last_cross: {date, dir, target, achieved_pct}},
  vtl: {anchors: [low1, low2], slope, intact: bool,
        break_confirms: "peak of degree N+1"},     // §12 semantics
  envelope: {cma_extrapolated, upper, lower, position_pct},

  // Event flags
  half_cycle_low: {date, price} | null,
  failed_cycle: bool,                       // price < cycle origin low (§13)
  possible_inversion: bool,                 // configuration flag only (§13) — school-dependent
  clarity: green | amber | red,             // §15.4

  // Nesting
  nested_alignment: {parent_degree, parent_age_pct, sync_ok: bool,
                     expected_subcycles, observed_subcycles},

  // Asset extras (emit, never gate):
  btc: {days_since_halving, days_to_next_halving, top_window_post_halving: [480,550],
        pi_cycle_top_fired, pi_cycle_bottom_fired, small_n_flag: true},
  gold: {long_degree_mean_years: 6.9, folklore_label: "8y — fails measurement",
         anchors: [2008-11-13, 2015-12-17, 2022-09-26], small_n_flag: true},
  equities: {kitchin_slot_months: ~40, presidential_phase}
}
```

Implementation order of value: low-to-low stats + timing bands →
translation ledger → swing-low confirmation + failed-cycle flag → FLD → centered detrend →
VTL → nesting checks → HCL → asset clocks (halving/Pi) → inversion flag.


---

## Part VII — Implementation (the deterministic engine)

The toolkit above is implemented mechanically in `src/analytics/cycle_engine.rs`
(pure compute over `price_history` — no tables, no network, no runtime
discretion). Analysts must NOT re-derive cycle math agentically; run the
commands. Parameter choices (window K = 10, ε = 0.05, FLD floor-truncation,
small-n band fallback, anchored deep degrees, CMA edge extrapolation) are
documented in the module docs.

| Toolkit item (§) | Engine | CLI |
|---|---|---|
| §7a rolling-window pivot detector | `pivot_lows` on §10-detrended lows (`centered_detrend_lows`, CMA edge-extrapolated per Hurst); lows closer than 0.6×prior merge to the lower raw price | `pftui analytics cycles analyze <SYM> [--json]` → `degrees[].lows` |
| §7b ZigZag detector | `zigzag_pivots`, selectable per degree via `DegreeConfig.detector` | same (config-level) |
| §8 low-to-low stats + timing bands | `build_band_stats` — mean/σ/median/min/max + empirical P15–P85 over the trailing 10 completed cycles; small-n (< 5) falls back to mean ± max(1σ, 15%·mean) and says so | `analyze` → `degrees[].band`, `band_position`, `bars_to_band_start/end`, `next_low_window` |
| §9 translation + ledger | `build_ledger` (top between lows; LT/MID/RT, ε = 0.05; failed flag) + `translation_flags` (first-LT-after-RT-string warning, RT-string-intact) | `pftui analytics cycles ledger <SYM> --degree <d> [--json]`; also in `analyze` |
| §10 centered detrend | `centered_detrend_lows` (detection substrate) | internal |
| §11 FLD | `compute_fld` — hl2 displaced floor(len/2) bars (truncation, NOT Sentient's +1 — school split documented), cross semantics, 2× measured-move target (omitted when degenerate, < 1% from extreme), achieved % | `analyze` → `degrees[].fld` |
| §12 VTL | `compute_vtl` — line through the two most recent confirmed lows, validity rule 1 checked, close-through break confirms the PEAK of the next-longer degree | `analyze` → `degrees[].vtl` |
| §13 swing-low confirmation | `swing_confirmation` (higher high + higher low after the candidate) → `last_confirmed_low` vs `candidate_low` | `analyze` |
| §13 failed cycle | close below the cycle-origin low after confirmation → `failed_cycle` (per-completed-cycle `failed` in the ledger) | `analyze`, `ledger` |
| §13 half-cycle low | `find_half_cycle_low` — most prominent pivot in [0.35, 0.65]×expected length holding above origin | `analyze` → `degrees[].half_cycle_low` |
| §13 inversion | `possible_inversion` FLAG only (over band + price near cycle highs); schools disagree — the engine never adjudicates | `analyze` |
| §15 nesting + clarity | `build_nested_alignment` (coincidence ±len/4, subcycle count ≈ r−1 ±1) + `grade_clarity` (issue count → green/amber/red; small-n capped at amber) | `analyze` → `degrees[].nested_alignment`, `clarity` |
| §16.1 BTC dual framing | halving clock reused from `analytics::cycle_clock`; low-to-low count = the engine's anchored "4-year" degree (2015-01-14 / 2018-12-15 / 2022-11-21, verified) — both emitted, labeled, never merged | `analyze BTC` → `btc_clocks`; `pftui analytics cycles clock` unchanged |
| §17.2 gold long degree | anchored "major" degree from the verified documented lows; "8-year" label carried as folklore | `analyze GC=F` → `gold_clock` |
| §17.4 silver | runs its own counts against gold's anchor dates (verified on silver's own minima) + cross-check note | `analyze SI=F` |

Degree defaults: BTC/BTC-USD daily ~60d / investor ~20wk / 4-year (anchored);
GC=F & SI=F intermediate ~20wk / major ~6.9y (anchored); generic crypto
daily+intermediate (7 bars/wk); generic equities daily ~40d + intermediate
~20wk (5 bars/wk). Report cards prefer the engine's `composite_verdict`
(`cycle_clock` verdict is the fallback). §14's oscillator-confirmation layer
is intentionally NOT in this engine — RSI/MACD live in `analytics technicals`;
the two-key rule (band gates the signal) is doctrine for the analyst, not a
computed field.

---

## Sources

Primary: J.M. Hurst, *The Profit Magic of Stock Transaction Timing* (1970); *J.M. Hurst
Cycles Course* (Cyclitec, 1973). Walter Bressert, *The Power of Oscillator/Cycle
Combinations* (1991); *The Cycle Trading Pattern Manual* (1997–98),
[PDF](https://smartmoneytrackerpremium.com/wp-content/uploads/2016/07/bressert-manual-1.pdf) —
read in full for this document. Bob Loukas: [The Financial Tap — What Are Market Cycles](https://thefinancialtap.com/what-are-market-cycles/) and
[Glossary](https://thefinancialtap.com/support/glossary/); [Bitcoin Live 4-Year Journey](https://bitcoin.live/4yearcycle).

Hurst tooling specs: [Sentient Trader help — Principles](https://sentienttrader.com/downloads/SentientTraderHelp/HurstCyclicTheoryPrinciples.htm),
[FLD](https://sentienttrader.com/downloads/SentientTraderHelp/FLD.htm),
[VTL](http://www.sentientcode.com/downloads/SentientTraderHelp/VTL.htm);
[Hurst Cycles Notes — Phasing](https://notes.hurstcycles.com/phasing-analysis/),
[FLDs](https://notes.hurstcycles.com/flds/), [VTLs](https://notes.hurstcycles.com/valid-trendlines-vtls/);
[Sigma-L — Nominal Model](https://www.sigma-l.net/p/hurst-nominal-model);
[MotiveWave Hurst Cycles Guide](https://www.motivewave.com/guides/MotiveWave_Hurst_Cycles.pdf).

Asset chapters: [Bitcoin Magazine Pro — Pi Cycle Top](https://www.bitcoinmagazinepro.com/charts/pi-cycle-top-indicator/);
[tradingdigits — Pi Cycle Bottom](https://www.tradingdigits.io/pi-cycle-bottom-indicator);
[BeInCrypto — 2022 Pi bottom miss](https://beincrypto.com/pi-cycle-bottom-is-flashing-has-bitcoin-btc-bottomed-out/);
[Swan — halving dates](https://www.swanbitcoin.com/education/bitcoin-halving-dates/);
[Whaleportal — 60-day cycle](https://whaleportal.com/blog/bitcoin-60-day-cycle-explained/);
[PANews — the 4-year cycle "changed form"](https://www.panewslab.com/en/articles/019eab6f-6b1c-767f-b1ab-bf6bdcba7afe);
[McClellan — gold 8-year cycle](https://www.mcoscillator.com/learning_center/weekly_chart/gold_and_dollar_share_an_8-year_cycle_period/);
[Investing.com — why gold's 8-year cycle isn't actionable](https://www.investing.com/analysis/why-golds-8year-cycle-isnt-actionable-200614566);
[Cycles Research Institute — Kitchin](https://cyclesresearchinstitute.org/cycles-research/economy/kitchin/);
[Grokipedia — Kitchin cycle](https://grokipedia.com/page/Kitchin_cycle).

Critique: [AlgoStorm — cycle theories debunked](https://algostorm.com/cycle-theories/);
[Hurst Cycles Forum — non-subjective analysis](https://forum.hurstcycles.com/t/a-non-subjective-approach-to-hurst-analysis/295);
[Smart Money Tracker Premium — terminology](https://www.smartmoneytrackerpremium.com/terminology/);
[Graddhy — market cycles](https://www.graddhy.com/pages/market-cycles);
[airlovsky — cycle inversions](https://www.airlovsky.com/cycle-inversions/).

Gold long-degree anchors (2008-11-13, 2015-12-17, 2022-09-26; mean ≈ 6.9y) verified
internally by pftui on ~26 years of daily data.
