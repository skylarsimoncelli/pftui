//! `pftui portfolio transaction import-delta` — import a Delta tracker CSV
//! export (full trade + fiat-flow history) into the transactions ledger.
//!
//! The Delta export is treated as the ground-truth ledger for the window it
//! covers. The importer:
//!
//! 1. Parses the Delta CSV shape (Date, Way, Base amount, Base currency,
//!    Base type, Quote amount, Quote currency, ..., Notes, Sync flag).
//! 2. Pairs `SYNC-BASE-HOLDINGS_*` fiat rows with their same-timestamp trade
//!    and represents the pair as pftui's native paired-cash mechanism
//!    (trade leg + cash leg linked via `paired_tx_id`).
//! 3. Imports trades WITHOUT a sync partner with NO auto-cash leg (model B):
//!    the export's own DEPOSIT/WITHDRAW rows already carry the funding cash
//!    movements for those trades, so a synthetic cash leg would double-count.
//!    (Verified against the data: reconstructing the USD balance with
//!    auto-cash legs for non-sync trades drives the balance far negative.)
//! 4. Imports non-sync DEPOSIT/WITHDRAW rows as external `transfer_in` /
//!    `transfer_out` flows on the fiat symbol (USD or GBP). Same-window
//!    opposite-direction USD/GBP pairs with a plausible implied rate are
//!    annotated as fx-conversion pairs (both legs kept — the implied rate
//!    documents itself) and excluded from the external-capital total.
//! 5. Reconciles pre-existing hand-entered rows against the CSV truth and
//!    classifies each as SUPERSEDED (deleted on apply), KEPT, or CONFLICT.
//!
//! Idempotent: every imported row carries a `[delta:<key>]` marker in its
//! notes; re-running the import skips rows whose key already exists.
//!
//! PRIVACY: amounts are printed only to the operator's local terminal /
//! local DB — never embedded in code, tests, or repo artifacts.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::transactions::{
    delete_transaction, insert_transaction, list_transactions_backend, set_paired_transaction,
};
use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, Transaction, TxType};

// ── options ─────────────────────────────────────────────────────────────────

pub struct Options {
    pub csv_path: String,
    pub apply: bool,
    pub json: bool,
    /// Back up the DB (full + transactions JSON) before applying. CLI always
    /// passes true; tests disable to avoid touching the archive dir.
    pub backup: bool,
}

// ── CSV model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Buy,
    Sell,
    Deposit,
    Withdraw,
}

impl Way {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "BUY" => Ok(Way::Buy),
            "SELL" => Ok(Way::Sell),
            "DEPOSIT" => Ok(Way::Deposit),
            "WITHDRAW" => Ok(Way::Withdraw),
            other => bail!("unknown Way value '{other}' in Delta CSV"),
        }
    }
    fn is_trade(self) -> bool {
        matches!(self, Way::Buy | Way::Sell)
    }
}

#[derive(Debug, Clone)]
struct DeltaRow {
    ts: String, // full ISO8601 timestamp from the export
    way: Way,
    base_amount: Decimal,
    base_symbol: String, // "BTC", "GOLD", "USD", ...
    base_type: String,   // CRYPTO / COMMODITY / FUND / FIAT
    quote_amount: Option<Decimal>,
    quote_currency: String,
    notes: String,
    sync_flag: bool,
    line: usize,
}

impl DeltaRow {
    fn is_sync_cash_leg(&self) -> bool {
        self.notes.starts_with("SYNC-BASE-HOLDINGS_")
    }
    fn date(&self) -> &str {
        &self.ts[..10.min(self.ts.len())]
    }
}

/// `"BTC (Bitcoin)"` → `"BTC"`.
fn base_symbol(raw: &str) -> String {
    raw.split(' ').next().unwrap_or(raw).trim().to_string()
}

/// Delta symbol → pftui (symbol, category). eToro non-expiry GOLD/SILVER
/// units are troy ounces, mapped onto the futures continuous symbols.
fn map_symbol(sym: &str, base_type: &str) -> (String, AssetCategory) {
    match sym {
        "GOLD" => ("GC=F".to_string(), AssetCategory::Commodity),
        "SILVER" => ("SI=F".to_string(), AssetCategory::Commodity),
        "BTC" => ("BTC".to_string(), AssetCategory::Crypto),
        other => {
            let cat = match base_type {
                "CRYPTO" => AssetCategory::Crypto,
                "COMMODITY" => AssetCategory::Commodity,
                "FUND" => AssetCategory::Fund,
                "FIAT" => AssetCategory::Cash,
                _ => AssetCategory::Equity,
            };
            (other.to_string(), cat)
        }
    }
}

fn parse_csv(path: &str) -> Result<Vec<DeltaRow>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("opening Delta CSV {path}"))?;
    let headers = reader.headers()?.clone();
    let col = |name: &str| -> Result<usize> {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| anyhow::anyhow!("Delta CSV missing column '{name}'"))
    };
    let c_date = col("Date")?;
    let c_way = col("Way")?;
    let c_base_amt = col("Base amount")?;
    let c_base_cur = col("Base currency (name)")?;
    let c_base_type = col("Base type")?;
    let c_quote_amt = col("Quote amount")?;
    let c_quote_cur = col("Quote currency")?;
    let c_notes = col("Notes")?;
    let c_sync = col("Sync Base Holding")?;

    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.with_context(|| format!("reading Delta CSV record {}", i + 2))?;
        let get = |idx: usize| record.get(idx).unwrap_or("").trim().to_string();
        let ts = get(c_date);
        if ts.is_empty() {
            continue;
        }
        let way = Way::parse(&get(c_way))?;
        let base_amount = Decimal::from_str(&get(c_base_amt))
            .with_context(|| format!("line {}: bad Base amount", i + 2))?;
        if base_amount <= Decimal::ZERO {
            bail!("line {}: nonpositive Base amount", i + 2);
        }
        let quote_raw = get(c_quote_amt);
        let quote_amount = if quote_raw.is_empty() {
            None
        } else {
            Some(
                Decimal::from_str(&quote_raw)
                    .with_context(|| format!("line {}: bad Quote amount", i + 2))?,
            )
        };
        rows.push(DeltaRow {
            ts: ts.clone(),
            way,
            base_amount,
            base_symbol: base_symbol(&get(c_base_cur)),
            base_type: get(c_base_type),
            quote_amount,
            quote_currency: get(c_quote_cur),
            notes: get(c_notes),
            sync_flag: get(c_sync) == "true",
            line: i + 2,
        });
    }
    Ok(rows)
}

// ── planning ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PlannedKind {
    Trade,
    SyncCashLeg,
    ExternalFlow,
    FxConversionLeg,
}

struct PlannedTx {
    kind: PlannedKind,
    key: String,
    tx: NewTransaction,
    /// For a sync cash leg: index (into the planned vec) of its trade.
    pair_with: Option<usize>,
}

struct Plan {
    planned: Vec<PlannedTx>,
    /// Date (YYYY-MM-DD) of the last row in the export — the CSV window end.
    window_end: String,
    /// Symbols (mapped) with any trade activity in the CSV.
    traded_symbols: HashSet<String>,
    /// (currency, implied USD-per-GBP rate, date) per detected fx pair.
    fx_pairs: Vec<FxPairReport>,
}

#[derive(Debug, Clone, Serialize)]
struct FxPairReport {
    date: String,
    gbp_amount: String,
    usd_amount: String,
    implied_usd_per_gbp: String,
    direction: String, // "gbp_to_usd" | "usd_to_gbp"
}

fn dedup_key(prefix: &str, row: &DeltaRow, seen: &mut HashMap<String, u32>) -> String {
    let base = format!(
        "{prefix}|{}|{}|{}",
        row.ts, row.base_symbol, row.base_amount
    );
    let n = seen.entry(base.clone()).or_insert(0);
    *n += 1;
    if *n == 1 {
        base
    } else {
        format!("{base}#{n}")
    }
}

fn ts_seconds(ts: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

fn build_plan(rows: &[DeltaRow]) -> Result<Plan> {
    // 1. Split: trades / sync cash legs / plain fiat flows.
    let mut sync_legs: Vec<&DeltaRow> = Vec::new();
    let mut plain_fiat: Vec<&DeltaRow> = Vec::new();
    let mut trades: Vec<&DeltaRow> = Vec::new();
    for r in rows {
        if r.way.is_trade() {
            if r.base_type == "FIAT" {
                bail!("line {}: BUY/SELL of a FIAT base is not supported", r.line);
            }
            trades.push(r);
        } else if r.is_sync_cash_leg() {
            sync_legs.push(r);
        } else {
            plain_fiat.push(r);
        }
    }

    // 2. Pair each sync cash leg with its same-timestamp trade.
    let mut leg_for_trade: HashMap<usize, &DeltaRow> = HashMap::new(); // trade line -> leg
    let mut used_legs: HashSet<usize> = HashSet::new();
    for t in &trades {
        if !t.sync_flag {
            continue;
        }
        let leg = sync_legs
            .iter()
            .find(|l| {
                !used_legs.contains(&l.line)
                    && l.ts == t.ts
                    && Some(l.base_amount) == t.quote_amount
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "line {}: sync-flagged trade has no matching SYNC-BASE-HOLDINGS fiat leg",
                    t.line
                )
            })?;
        used_legs.insert(leg.line);
        leg_for_trade.insert(t.line, leg);
    }
    if let Some(orphan) = sync_legs.iter().find(|l| !used_legs.contains(&l.line)) {
        bail!(
            "line {}: SYNC-BASE-HOLDINGS fiat row has no matching sync-flagged trade",
            orphan.line
        );
    }

    // 3. Detect fx-conversion pairs among plain fiat flows: opposite-direction
    //    USD/GBP rows within 30 minutes with an implied USD-per-GBP rate in
    //    a plausible band. Greedy, each row used at most once.
    let mut conversion_partner: HashMap<usize, (usize, Decimal)> = HashMap::new(); // line -> (partner line, rate)
    let lo = Decimal::from_str("1.10")?;
    let hi = Decimal::from_str("1.60")?;
    for (i, a) in plain_fiat.iter().enumerate() {
        if conversion_partner.contains_key(&a.line) {
            continue;
        }
        for b in plain_fiat.iter().skip(i + 1) {
            if conversion_partner.contains_key(&b.line) {
                continue;
            }
            let (ca, cb) = (a.base_symbol.as_str(), b.base_symbol.as_str());
            let cross = (ca == "USD" && cb == "GBP") || (ca == "GBP" && cb == "USD");
            let opposite = (a.way == Way::Deposit) != (b.way == Way::Deposit);
            if !cross || !opposite {
                continue;
            }
            if (ts_seconds(&a.ts) - ts_seconds(&b.ts)).abs() > 1800 {
                continue;
            }
            let (gbp, usd) = if ca == "GBP" { (a, b) } else { (b, a) };
            if gbp.base_amount.is_zero() {
                continue;
            }
            let rate = usd.base_amount / gbp.base_amount;
            if rate < lo || rate > hi {
                continue;
            }
            conversion_partner.insert(a.line, (b.line, rate));
            conversion_partner.insert(b.line, (a.line, rate));
            break;
        }
    }

    // 4. Build planned transactions in export order.
    let mut planned: Vec<PlannedTx> = Vec::new();
    let mut seen_keys: HashMap<String, u32> = HashMap::new();
    let mut traded_symbols: HashSet<String> = HashSet::new();
    let mut fx_pairs: Vec<FxPairReport> = Vec::new();
    let mut fx_pair_seen: HashSet<(usize, usize)> = HashSet::new();

    for r in rows {
        if r.way.is_trade() {
            let (symbol, category) = map_symbol(&r.base_symbol, &r.base_type);
            traded_symbols.insert(symbol.clone());
            let quote = r.quote_amount.ok_or_else(|| {
                anyhow::anyhow!("line {}: trade row missing Quote amount", r.line)
            })?;
            let price_per = quote / r.base_amount;
            let tx_type = if r.way == Way::Buy {
                TxType::Buy
            } else {
                TxType::Sell
            };
            let key = dedup_key(
                if r.way == Way::Buy { "BUY" } else { "SELL" },
                r,
                &mut seen_keys,
            );
            let sync_leg = leg_for_trade.get(&r.line).copied();
            let mut note = format!("Delta import {} fill", r.base_symbol);
            if sync_leg.is_none() {
                note.push_str(
                    "; no cash leg — funding carried by the export's own flow rows (model B)",
                );
            }
            if !r.notes.is_empty() {
                note.push_str(&format!("; source notes: {}", r.notes));
            }
            note.push_str(&format!(" [delta:{key}]"));
            let trade_idx = planned.len();
            planned.push(PlannedTx {
                kind: PlannedKind::Trade,
                key,
                tx: NewTransaction {
                    symbol,
                    category,
                    tx_type,
                    quantity: r.base_amount,
                    price_per,
                    currency: r.quote_currency.clone(),
                    date: r.date().to_string(),
                    notes: Some(note),
                },
                pair_with: None,
            });
            if let Some(leg) = sync_leg {
                let leg_key = dedup_key("CASHLEG", leg, &mut seen_keys);
                let leg_type = if r.way == Way::Buy {
                    TxType::Sell
                } else {
                    TxType::Buy
                };
                let note = format!(
                    "Delta sync cash leg for {} {} [delta:{leg_key}]",
                    r.base_symbol,
                    if r.way == Way::Buy { "buy" } else { "sell" },
                );
                planned.push(PlannedTx {
                    kind: PlannedKind::SyncCashLeg,
                    key: leg_key,
                    tx: NewTransaction {
                        symbol: leg.base_symbol.clone(),
                        category: AssetCategory::Cash,
                        tx_type: leg_type,
                        quantity: leg.base_amount,
                        price_per: Decimal::ONE,
                        currency: leg.base_symbol.clone(),
                        date: leg.date().to_string(),
                        notes: Some(note),
                    },
                    pair_with: Some(trade_idx),
                });
            }
        } else if !r.is_sync_cash_leg() {
            // Plain fiat flow → external transfer (or fx conversion leg).
            let tx_type = if r.way == Way::Deposit {
                TxType::TransferIn
            } else {
                TxType::TransferOut
            };
            let conversion = conversion_partner.get(&r.line).copied();
            let kind = if conversion.is_some() {
                PlannedKind::FxConversionLeg
            } else {
                PlannedKind::ExternalFlow
            };
            let key = dedup_key("FLOW", r, &mut seen_keys);
            let mut note = match conversion {
                Some((_, rate)) => format!(
                    "Delta import: fx-conversion pair leg (implied {} USD/GBP)",
                    rate.round_dp(6)
                ),
                None => "Delta import: external flow".to_string(),
            };
            if !r.notes.is_empty() {
                note.push_str(&format!("; source notes: {}", r.notes));
            }
            note.push_str(&format!(" [delta:{key}]"));
            if let Some((partner_line, rate)) = conversion {
                let pair_id = (r.line.min(partner_line), r.line.max(partner_line));
                if fx_pair_seen.insert(pair_id) {
                    let gbp_is_self = r.base_symbol == "GBP";
                    let partner = rows.iter().find(|x| x.line == partner_line);
                    let (gbp_amt, usd_amt) = match (gbp_is_self, partner) {
                        (true, Some(p)) => (r.base_amount, p.base_amount),
                        (false, Some(p)) => (p.base_amount, r.base_amount),
                        _ => (Decimal::ZERO, Decimal::ZERO),
                    };
                    let gbp_out = (gbp_is_self && r.way == Way::Withdraw)
                        || (!gbp_is_self && r.way == Way::Deposit);
                    fx_pairs.push(FxPairReport {
                        date: r.date().to_string(),
                        gbp_amount: gbp_amt.to_string(),
                        usd_amount: usd_amt.to_string(),
                        implied_usd_per_gbp: rate.round_dp(6).to_string(),
                        direction: if gbp_out { "gbp_to_usd" } else { "usd_to_gbp" }.to_string(),
                    });
                }
            }
            planned.push(PlannedTx {
                kind,
                key,
                tx: NewTransaction {
                    symbol: r.base_symbol.clone(),
                    category: AssetCategory::Cash,
                    tx_type,
                    quantity: r.base_amount,
                    price_per: Decimal::ONE,
                    currency: r.base_symbol.clone(),
                    date: r.date().to_string(),
                    notes: Some(note),
                },
                pair_with: None,
            });
        }
    }

    let window_end = rows
        .iter()
        .map(|r| r.date().to_string())
        .max()
        .unwrap_or_default();

    Ok(Plan {
        planned,
        window_end,
        traded_symbols,
        fx_pairs,
    })
}

// ── reconciliation of pre-existing rows ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Classification {
    Superseded,
    Kept,
    Conflict,
    AlreadyImported,
}

#[derive(Debug, Clone, Serialize)]
struct ReconRow {
    id: i64,
    symbol: String,
    date: String,
    tx_type: String,
    classification: Classification,
    reason: String,
}

fn parse_date(d: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&d[..10.min(d.len())], "%Y-%m-%d").ok()
}

fn day_delta(a: &str, b: &str) -> i64 {
    match (parse_date(a), parse_date(b)) {
        (Some(x), Some(y)) => (x - y).num_days().abs(),
        _ => i64::MAX,
    }
}

/// Quantity-similarity gate for direct fill matching: within ±30%.
fn qty_close(a: Decimal, b: Decimal) -> bool {
    if a.is_zero() || b.is_zero() {
        return false;
    }
    let ratio = a / b;
    let lo = Decimal::from_str("0.7").unwrap_or_default();
    let hi = Decimal::from_str("1.43").unwrap_or_default();
    ratio >= lo && ratio <= hi
}

fn classify_existing(existing: &[Transaction], plan: &Plan) -> Vec<ReconRow> {
    let mut out: Vec<ReconRow> = Vec::new();
    let mut class_by_id: HashMap<i64, (Classification, String)> = HashMap::new();

    let already = |t: &Transaction| {
        t.notes
            .as_deref()
            .map(|n| n.contains("[delta:"))
            .unwrap_or(false)
    };

    // CSV trade facts (planned trades only).
    struct Fill<'a> {
        idx: usize,
        tx: &'a NewTransaction,
    }
    let fills: Vec<Fill> = plan
        .planned
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PlannedKind::Trade)
        .map(|(idx, p)| Fill { idx, tx: &p.tx })
        .collect();
    let flows: Vec<&PlannedTx> = plan
        .planned
        .iter()
        .filter(|p| {
            matches!(
                p.kind,
                PlannedKind::ExternalFlow | PlannedKind::FxConversionLeg
            )
        })
        .collect();

    // Pass 1: trades (non-cash rows), best-match greedy with fill exclusivity.
    struct Candidate {
        existing_id: i64,
        fill_idx: usize,
        qty_dist: Decimal,
        day_dist: i64,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let trade_rows: Vec<&Transaction> = existing
        .iter()
        .filter(|t| t.category != AssetCategory::Cash && !already(t))
        .collect();
    for t in &trade_rows {
        if t.date[..10.min(t.date.len())] > *plan.window_end {
            continue;
        }
        for f in &fills {
            if f.tx.symbol != t.symbol
                || f.tx.tx_type != t.tx_type
                || !qty_close(t.quantity, f.tx.quantity)
                || day_delta(&t.date, &f.tx.date) > 45
            {
                continue;
            }
            let qty_dist = (t.quantity - f.tx.quantity).abs()
                / f.tx.quantity.max(Decimal::from_str("0.000001").unwrap_or(Decimal::ONE));
            candidates.push(Candidate {
                existing_id: t.id,
                fill_idx: f.idx,
                qty_dist,
                day_dist: day_delta(&t.date, &f.tx.date),
            });
        }
    }
    // Same-day fills win over closer-quantity fills further away: hand
    // entries are usually logged on the trade day with an approximate size.
    candidates.sort_by(|a, b| {
        a.day_dist
            .cmp(&b.day_dist)
            .then(a.qty_dist.cmp(&b.qty_dist))
    });
    let mut claimed_fills: HashSet<usize> = HashSet::new();
    let mut matched_fill_for: HashMap<i64, usize> = HashMap::new();
    for c in candidates {
        if claimed_fills.contains(&c.fill_idx) || matched_fill_for.contains_key(&c.existing_id) {
            continue;
        }
        claimed_fills.insert(c.fill_idx);
        matched_fill_for.insert(c.existing_id, c.fill_idx);
    }

    for t in &trade_rows {
        let date10 = &t.date[..10.min(t.date.len())];
        let (class, reason) = if date10 > plan.window_end.as_str() {
            (
                Classification::Kept,
                "dated after the CSV window end".to_string(),
            )
        } else if let Some(fill_idx) = matched_fill_for.get(&t.id) {
            let f = &plan.planned[*fill_idx];
            (
                Classification::Superseded,
                format!(
                    "direct match: CSV fill {} {} on {} replaces it",
                    f.tx.tx_type, f.tx.symbol, f.tx.date
                ),
            )
        } else if plan.traded_symbols.contains(&t.symbol) {
            (
                Classification::Superseded,
                "coarse hand aggregate — symbol's full fill history comes from the CSV"
                    .to_string(),
            )
        } else {
            (
                Classification::Conflict,
                "no CSV activity for this symbol — operator review".to_string(),
            )
        };
        class_by_id.insert(t.id, (class, reason));
    }

    // Pass 2: cash rows.
    for t in existing {
        if t.category != AssetCategory::Cash || already(t) {
            continue;
        }
        let date10 = &t.date[..10.min(t.date.len())];
        let (class, reason) = if let Some(pid) = t.paired_tx_id {
            match class_by_id.get(&pid) {
                Some((c, _)) => (*c, format!("cash leg follows paired trade #{pid}")),
                None => {
                    if date10 > plan.window_end.as_str() {
                        (
                            Classification::Kept,
                            "dated after the CSV window end".to_string(),
                        )
                    } else {
                        (
                            Classification::Conflict,
                            "paired leg not classified — operator review".to_string(),
                        )
                    }
                }
            }
        } else if date10 > plan.window_end.as_str() {
            (
                Classification::Kept,
                "dated after the CSV window end".to_string(),
            )
        } else if t
            .notes
            .as_deref()
            .map(|n| n.to_lowercase().contains("set-cash"))
            .unwrap_or(false)
        {
            (
                Classification::Superseded,
                "set-cash baseline replaced by the full flow ledger".to_string(),
            )
        } else {
            // Match against CSV flows: same currency + direction, amount
            // within 2%, within ±3 days.
            let two_pct = Decimal::from_str("0.02").unwrap_or_default();
            let matched = flows.iter().find(|f| {
                f.tx.symbol == t.symbol
                    && f.tx.tx_type.increases_quantity() == t.tx_type.increases_quantity()
                    && day_delta(&t.date, &f.tx.date) <= 3
                    && !f.tx.quantity.is_zero()
                    && ((t.quantity - f.tx.quantity).abs() / f.tx.quantity) <= two_pct
            });
            match matched {
                Some(f) => (
                    Classification::Superseded,
                    format!("matches CSV flow on {}", f.tx.date),
                ),
                None => (
                    Classification::Kept,
                    "deliberate operator cash flow with no CSV counterpart".to_string(),
                ),
            }
        };
        class_by_id.insert(t.id, (class, reason));
    }

    for t in existing {
        if already(t) {
            out.push(ReconRow {
                id: t.id,
                symbol: t.symbol.clone(),
                date: t.date.clone(),
                tx_type: t.tx_type.to_string(),
                classification: Classification::AlreadyImported,
                reason: "carries a [delta:] import marker".to_string(),
            });
            continue;
        }
        let (class, reason) = class_by_id
            .get(&t.id)
            .cloned()
            .unwrap_or((Classification::Conflict, "unclassified".to_string()));
        out.push(ReconRow {
            id: t.id,
            symbol: t.symbol.clone(),
            date: t.date.clone(),
            tx_type: t.tx_type.to_string(),
            classification: class,
            reason,
        });
    }

    // Notes carry-over: direct-matched superseded rows donate operator
    // context to the CSV row that replaces them. Mutating the plan happens
    // in `run` via this map.
    out
}

/// For direct-matched superseded trades: (planned index, context snippet).
fn notes_carryover(
    existing: &[Transaction],
    plan: &Plan,
    recon: &[ReconRow],
) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let superseded: HashSet<i64> = recon
        .iter()
        .filter(|r| r.classification == Classification::Superseded)
        .map(|r| r.id)
        .collect();
    for r in recon {
        if !superseded.contains(&r.id) {
            continue;
        }
        // Reason embeds the matched fill only for direct matches.
        if !r.reason.starts_with("direct match") {
            continue;
        }
        let Some(t) = existing.iter().find(|t| t.id == r.id) else {
            continue;
        };
        let Some(notes) = t.notes.as_deref().filter(|n| !n.is_empty()) else {
            continue;
        };
        // Locate the matched planned fill again (same criteria as classify).
        let matched = plan
            .planned
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == PlannedKind::Trade)
            .filter(|(_, p)| {
                p.tx.symbol == t.symbol
                    && p.tx.tx_type == t.tx_type
                    && qty_close(t.quantity, p.tx.quantity)
                    && day_delta(&t.date, &p.tx.date) <= 45
            })
            .min_by_key(|(_, p)| day_delta(&t.date, &p.tx.date));
        if let Some((idx, _)) = matched {
            let snippet: String = notes.chars().take(200).collect();
            out.push((idx, format!(" | operator context (hand entry #{}): {snippet}", t.id)));
        }
    }
    out
}

// ── verification arithmetic ─────────────────────────────────────────────────

fn signed_qty(t: TxType, q: Decimal) -> Decimal {
    if t.increases_quantity() {
        q
    } else {
        -q
    }
}

fn net_by_symbol(txs: &[Transaction]) -> HashMap<String, Decimal> {
    let mut out: HashMap<String, Decimal> = HashMap::new();
    for t in txs {
        *out.entry(t.symbol.clone()).or_default() += signed_qty(t.tx_type, t.quantity);
    }
    out
}

#[derive(Debug, Serialize)]
struct UsdEquation {
    external_in: String,
    external_out: String,
    fx_conversion_net: String,
    sync_sell_proceeds: String,
    sync_buy_cost: String,
    csv_usd_balance: String,
}

#[derive(Debug, Serialize)]
struct Report {
    mode: String,
    csv_rows: usize,
    planned_trades: usize,
    planned_sync_cash_legs: usize,
    planned_external_flows: usize,
    planned_fx_conversion_legs: usize,
    skipped_already_imported: usize,
    window_end: String,
    cash_model: String,
    fx_pairs: Vec<FxPairReport>,
    reconciliation: Vec<ReconRow>,
    superseded_count: usize,
    kept_count: usize,
    conflict_count: usize,
    usd_equation: UsdEquation,
    csv_net_quantities: HashMap<String, String>,
    external_capital_usd_equivalent: String,
    gbp_residual: String,
    holdings_before: HashMap<String, String>,
    holdings_after: Option<HashMap<String, String>>,
    backup_db_path: Option<String>,
    backup_transactions_json_path: Option<String>,
    journal_note_id: Option<i64>,
}

fn usd_equation(plan: &Plan, to_import: &[&PlannedTx]) -> UsdEquation {
    let mut external_in = Decimal::ZERO;
    let mut external_out = Decimal::ZERO;
    let mut fx_net = Decimal::ZERO;
    let mut sync_in = Decimal::ZERO;
    let mut sync_out = Decimal::ZERO;
    for p in to_import {
        if p.tx.symbol != "USD" {
            continue;
        }
        let signed = signed_qty(p.tx.tx_type, p.tx.quantity);
        match p.kind {
            PlannedKind::ExternalFlow => {
                if signed > Decimal::ZERO {
                    external_in += signed;
                } else {
                    external_out += -signed;
                }
            }
            PlannedKind::FxConversionLeg => fx_net += signed,
            PlannedKind::SyncCashLeg => {
                if signed > Decimal::ZERO {
                    sync_in += signed;
                } else {
                    sync_out += -signed;
                }
            }
            PlannedKind::Trade => {}
        }
    }
    let balance = external_in - external_out + fx_net + sync_in - sync_out;
    let _ = plan;
    UsdEquation {
        external_in: external_in.to_string(),
        external_out: external_out.to_string(),
        fx_conversion_net: fx_net.to_string(),
        sync_sell_proceeds: sync_in.to_string(),
        sync_buy_cost: sync_out.to_string(),
        csv_usd_balance: balance.to_string(),
    }
}

/// External capital contributed, USD-equivalent. GBP flows are converted at
/// the implied rate of the nearest-in-time fx-conversion pair (the pairs
/// document their own rates); falls back to the average pair rate, then 1:1
/// with a residual warning if the export contains no conversion pairs.
fn external_capital_usd(plan: &Plan, to_import: &[&PlannedTx]) -> Decimal {
    let pair_rates: Vec<(NaiveDate, Decimal)> = plan
        .fx_pairs
        .iter()
        .filter_map(|p| {
            Some((
                parse_date(&p.date)?,
                Decimal::from_str(&p.implied_usd_per_gbp).ok()?,
            ))
        })
        .collect();
    let rate_for = |date: &str| -> Decimal {
        let Some(d) = parse_date(date) else {
            return Decimal::ONE;
        };
        pair_rates
            .iter()
            .min_by_key(|(pd, _)| (*pd - d).num_days().abs())
            .map(|(_, r)| *r)
            .unwrap_or(Decimal::ONE)
    };
    let mut total = Decimal::ZERO;
    for p in to_import {
        if p.kind != PlannedKind::ExternalFlow {
            continue;
        }
        let mut signed = signed_qty(p.tx.tx_type, p.tx.quantity);
        if p.tx.symbol == "GBP" {
            signed *= rate_for(&p.tx.date);
        } else if p.tx.symbol != "USD" {
            continue;
        }
        total += signed;
    }
    total
}

// ── price-deviation annotation (data-driven, sqlite only) ───────────────────

fn nearest_close(
    conn: &rusqlite::Connection,
    symbol: &str,
    date: &str,
) -> Option<Decimal> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT close FROM price_history
             WHERE symbol = ?1
               AND date BETWEEN date(?2, '-5 days') AND date(?2, '+5 days')
             ORDER BY abs(julianday(date) - julianday(?2)) ASC, date ASC
             LIMIT 1",
        )
        .ok()?;
    let close: Option<String> = stmt
        .query_row(rusqlite::params![symbol, date], |row| row.get(0))
        .ok();
    close.and_then(|c| Decimal::from_str(&c).ok())
}

fn annotate_price_outliers(conn: &rusqlite::Connection, planned: &mut [PlannedTx]) {
    for p in planned.iter_mut() {
        if p.kind != PlannedKind::Trade || p.tx.currency != "USD" {
            continue;
        }
        let Some(close) = nearest_close(conn, &p.tx.symbol, &p.tx.date) else {
            continue;
        };
        if close <= Decimal::ZERO {
            continue;
        }
        let lo = close * Decimal::from_str("0.85").unwrap_or_default();
        let hi = close * Decimal::from_str("1.15").unwrap_or_default();
        if p.tx.price_per < lo || p.tx.price_per > hi {
            if let Some(n) = p.tx.notes.as_mut() {
                n.push_str(
                    " | price >15% from nearest session close — imported faithfully from the source export, review",
                );
            }
        }
    }
}

// ── runner ──────────────────────────────────────────────────────────────────

pub fn run(backend: &BackendConnection, opts: Options) -> Result<()> {
    let rows = parse_csv(&opts.csv_path)?;
    if rows.is_empty() {
        bail!("Delta CSV contains no data rows");
    }
    let mut plan = build_plan(&rows)?;

    let existing = list_transactions_backend(backend)?;
    let existing_keys: HashSet<String> = existing
        .iter()
        .filter_map(|t| t.notes.as_deref())
        .flat_map(extract_delta_keys)
        .collect();

    let recon = classify_existing(&existing, &plan);
    for (idx, context) in notes_carryover(&existing, &plan, &recon) {
        if let Some(n) = plan.planned[idx].tx.notes.as_mut() {
            n.push_str(&context);
        }
    }

    // Price-outlier annotation needs the local price_history (sqlite only).
    if let BackendConnection::Sqlite { conn } = backend {
        annotate_price_outliers(conn, &mut plan.planned);
    }

    let (to_import, skipped): (Vec<&PlannedTx>, Vec<&PlannedTx>) = plan
        .planned
        .iter()
        .partition(|p| !existing_keys.contains(&p.key));

    let superseded: Vec<&ReconRow> = recon
        .iter()
        .filter(|r| r.classification == Classification::Superseded)
        .collect();
    let kept = recon
        .iter()
        .filter(|r| r.classification == Classification::Kept)
        .count();
    let conflicts = recon
        .iter()
        .filter(|r| r.classification == Classification::Conflict)
        .count();

    let equation = usd_equation(&plan, &to_import);
    let external_capital = external_capital_usd(&plan, &to_import);
    let gbp_residual: Decimal = to_import
        .iter()
        .filter(|p| p.tx.symbol == "GBP")
        .map(|p| signed_qty(p.tx.tx_type, p.tx.quantity))
        .sum();

    let holdings_before = net_by_symbol(&existing);

    let mut report = Report {
        mode: if opts.apply { "apply" } else { "dry_run" }.to_string(),
        csv_rows: rows.len(),
        planned_trades: to_import
            .iter()
            .filter(|p| p.kind == PlannedKind::Trade)
            .count(),
        planned_sync_cash_legs: to_import
            .iter()
            .filter(|p| p.kind == PlannedKind::SyncCashLeg)
            .count(),
        planned_external_flows: to_import
            .iter()
            .filter(|p| p.kind == PlannedKind::ExternalFlow)
            .count(),
        planned_fx_conversion_legs: to_import
            .iter()
            .filter(|p| p.kind == PlannedKind::FxConversionLeg)
            .count(),
        skipped_already_imported: skipped.len(),
        window_end: plan.window_end.clone(),
        cash_model: "model B: sync trades carry their own paired cash legs; non-sync trades \
                     import with no cash leg (the export's flow rows are the funding); \
                     non-sync DEPOSIT/WITHDRAW rows are external transfer_in/transfer_out"
            .to_string(),
        fx_pairs: plan.fx_pairs.clone(),
        reconciliation: recon.clone(),
        superseded_count: superseded.len(),
        kept_count: kept,
        conflict_count: conflicts,
        usd_equation: equation,
        csv_net_quantities: {
            let mut m: HashMap<String, Decimal> = HashMap::new();
            for p in &plan.planned {
                if p.kind == PlannedKind::Trade {
                    *m.entry(p.tx.symbol.clone()).or_default() +=
                        signed_qty(p.tx.tx_type, p.tx.quantity);
                }
            }
            m.into_iter().map(|(k, v)| (k, v.to_string())).collect()
        },
        external_capital_usd_equivalent: external_capital.to_string(),
        gbp_residual: gbp_residual.to_string(),
        holdings_before: holdings_before
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
        holdings_after: None,
        backup_db_path: None,
        backup_transactions_json_path: None,
        journal_note_id: None,
    };

    if opts.apply {
        let conn = match backend {
            BackendConnection::Sqlite { conn } => conn,
            BackendConnection::Postgres { .. } => {
                bail!("import-delta --apply supports the local SQLite backend only")
            }
        };

        // Backup BEFORE any mutation.
        if opts.backup {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let db_dest = crate::db::archive::archive_dir()
                .join(format!("pftui-pre-delta-import-{stamp}.db"));
            crate::db::archive::backup_database(conn, &db_dest)?;
            let tx_dest = crate::db::archive::archive_dir()
                .join(format!("transactions-pre-delta-import-{stamp}.json"));
            crate::db::archive::export_table_json(conn, "transactions", &tx_dest)?;
            report.backup_db_path = Some(db_dest.display().to_string());
            report.backup_transactions_json_path = Some(tx_dest.display().to_string());
        }

        conn.execute_batch("BEGIN")?;
        let apply_result = (|| -> Result<()> {
            // Unlink paired_tx_id references first: superseded pairs
            // reference each other and FK enforcement would reject the
            // first delete otherwise.
            for r in &superseded {
                conn.execute(
                    "UPDATE transactions SET paired_tx_id = NULL
                     WHERE id = ?1 OR paired_tx_id = ?1",
                    rusqlite::params![r.id],
                )?;
            }
            for r in &superseded {
                delete_transaction(conn, r.id)?;
            }
            let mut id_for_planned: HashMap<usize, i64> = HashMap::new();
            for (idx, p) in plan.planned.iter().enumerate() {
                if existing_keys.contains(&p.key) {
                    continue;
                }
                let id = insert_transaction(conn, &p.tx)?;
                id_for_planned.insert(idx, id);
                if let Some(trade_idx) = p.pair_with {
                    if let Some(trade_id) = id_for_planned.get(&trade_idx) {
                        set_paired_transaction(conn, *trade_id, Some(id))?;
                        set_paired_transaction(conn, id, Some(*trade_id))?;
                    }
                }
            }
            Ok(())
        })();
        match apply_result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e).context("delta import failed — rolled back");
            }
        }

        let after = list_transactions_backend(backend)?;
        report.holdings_after = Some(
            net_by_symbol(&after)
                .into_iter()
                .map(|(k, v)| (k, v.to_string()))
                .collect(),
        );

        // Journal audit trail (author system, section system).
        let note = format!(
            "Delta-export transaction import applied: {} CSV rows -> {} trades, {} sync cash legs, \
             {} external flows, {} fx-conversion legs imported ({} skipped as already imported); \
             {} hand-entered rows superseded and deleted, {} kept, {} conflicts. Cash model B: \
             sync trades carry paired cash legs, non-sync trades import without a cash leg \
             (the export's own DEPOSIT/WITHDRAW rows are the funding), plain fiat rows are \
             external transfer_in/transfer_out. External capital contributed (USD-equivalent, \
             GBP at the conversion pairs' implied rates): {}. CSV-window USD balance: {}. \
             GBP residual: {}. This closes journal note #728's flow-contamination gap — \
             money-weighted returns are now computable from the transfer ledger.",
            report.csv_rows,
            report.planned_trades,
            report.planned_sync_cash_legs,
            report.planned_external_flows,
            report.planned_fx_conversion_legs,
            report.skipped_already_imported,
            report.superseded_count,
            report.kept_count,
            report.conflict_count,
            report.external_capital_usd_equivalent,
            report.usd_equation.csv_usd_balance,
            report.gbp_residual,
        );
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let note_id =
            crate::db::daily_notes::add_note_backend(backend, &today, "system", &note, "system")?;
        report.journal_note_id = Some(note_id);
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

fn extract_delta_keys(notes: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = notes;
    while let Some(start) = rest.find("[delta:") {
        let tail = &rest[start + 7..];
        if let Some(end) = tail.find(']') {
            out.push(tail[..end].to_string());
            rest = &tail[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn print_report(r: &Report) {
    println!(
        "Delta import ({}) — {} CSV rows, window ends {}",
        r.mode, r.csv_rows, r.window_end
    );
    println!("Cash model: {}", r.cash_model);
    println!(
        "Planned: {} trades, {} sync cash legs, {} external flows, {} fx-conversion legs ({} already imported, skipped)",
        r.planned_trades,
        r.planned_sync_cash_legs,
        r.planned_external_flows,
        r.planned_fx_conversion_legs,
        r.skipped_already_imported
    );
    if !r.fx_pairs.is_empty() {
        println!("FX conversion pairs:");
        for p in &r.fx_pairs {
            println!(
                "  {} {}: GBP {} <-> USD {} (implied {} USD/GBP)",
                p.date, p.direction, p.gbp_amount, p.usd_amount, p.implied_usd_per_gbp
            );
        }
    }
    println!(
        "Reconciliation of existing rows: {} superseded, {} kept, {} conflicts",
        r.superseded_count, r.kept_count, r.conflict_count
    );
    for row in &r.reconciliation {
        println!(
            "  #{} {} {} {} -> {:?}: {}",
            row.id, row.date, row.symbol, row.tx_type, row.classification, row.reason
        );
    }
    let e = &r.usd_equation;
    println!(
        "USD equation: external_in {} - external_out {} + fx_net {} + sync_sell {} - sync_buy {} = {}",
        e.external_in, e.external_out, e.fx_conversion_net, e.sync_sell_proceeds, e.sync_buy_cost, e.csv_usd_balance
    );
    let mut nets: Vec<_> = r.csv_net_quantities.iter().collect();
    nets.sort();
    for (sym, q) in nets {
        println!("CSV net {sym}: {q}");
    }
    println!(
        "External capital contributed (USD-equivalent): {}",
        r.external_capital_usd_equivalent
    );
    println!("GBP residual: {}", r.gbp_residual);
    if let Some(after) = &r.holdings_after {
        println!("Holdings after apply (net quantity by symbol):");
        let mut rows: Vec<_> = after.iter().collect();
        rows.sort();
        for (sym, q) in rows {
            let before = r
                .holdings_before
                .get(sym)
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            println!("  {sym}: {before} -> {q}");
        }
    }
    if let Some(p) = &r.backup_db_path {
        println!("Backup DB: {p}");
    }
    if let Some(p) = &r.backup_transactions_json_path {
        println!("Backup transactions JSON: {p}");
    }
    if let Some(id) = r.journal_note_id {
        println!("Journal note #{id} written (author system, section system)");
    }
    if r.mode == "dry_run" {
        println!("Dry run — no writes performed. Pass --apply to import.");
    }
}

// ── tests (synthetic fixtures only) ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    const HEADER: &str = "\"Date\",\"Way\",\"Base amount\",\"Base currency (name)\",\"Base type\",\"Quote amount\",\"Quote currency\",\"Exchange\",\"Sent/Received from\",\"Sent to\",\"Fee amount\",\"Fee currency (name)\",\"Broker\",\"Notes\",\"Sync Base Holding\",\"Leverage Metadata\"";

    /// Synthetic ledger: one external deposit, one plain trade funded by a
    /// flow row, one sync-paired trade, one GBP->USD conversion pair, one
    /// GBP external deposit.
    fn synthetic_csv() -> String {
        [
            HEADER,
            r#""2026-01-05T10:00:00.000Z","DEPOSIT",1000,"USD (United States dollar)","FIAT",,,,"OTHER","MY_WALLET",,,"",,false,"#,
            r#""2026-01-06T09:00:00.000Z","WITHDRAW",200,"USD (United States dollar)","FIAT",,,,"MY_WALLET","OTHER",,,"",,false,"#,
            r#""2026-01-06T09:00:30.000Z","BUY",2,"GOLD (Gold (Non Expiry))","COMMODITY",198,"USD","eToro",,,,,,,false,"#,
            r#""2026-01-07T12:00:00.000Z","DEPOSIT",500,"GBP (British pound)","FIAT",,,,"OTHER","MY_WALLET",,,"",,false,"#,
            r#""2026-01-08T12:00:00.000Z","WITHDRAW",100,"GBP (British pound)","FIAT",,,,"MY_WALLET","OTHER",,,"",,false,"#,
            r#""2026-01-08T12:00:20.000Z","DEPOSIT",135,"USD (United States dollar)","FIAT",,,,"OTHER","MY_WALLET",,,"",,false,"#,
            r#""2026-01-09T15:00:00.000Z","BUY",0.5,"BTC (Bitcoin)","CRYPTO",450,"USD","eToro",,,,,,,true,"#,
            r#""2026-01-09T15:00:00.000Z","WITHDRAW",450,"USD (United States dollar)","FIAT",,,"eToro","MY_WALLET","OTHER",,,,"SYNC-BASE-HOLDINGS_BUY_BTC/USD",false,"#,
            r#""2026-01-10T15:00:00.000Z","SELL",0.2,"BTC (Bitcoin)","CRYPTO",190,"USD","eToro",,,,,,,true,"#,
            r#""2026-01-10T15:00:00.000Z","DEPOSIT",190,"USD (United States dollar)","FIAT",,,"eToro","OTHER","MY_WALLET",,,,"SYNC-BASE-HOLDINGS_SELL_BTC/USD",false,"#,
        ]
        .join("\n")
    }

    fn write_csv(content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pftui-delta-test-{}-{}.csv",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn rand_suffix() -> u128 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        t.wrapping_mul(1000).wrapping_add(n)
    }

    fn backend() -> BackendConnection {
        let conn = open_in_memory();
        // Mirror production connections: FK enforcement ON, so the
        // paired-leg unlink-before-delete path is exercised.
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        BackendConnection::Sqlite { conn }
    }

    fn opts(path: &std::path::Path, apply: bool) -> Options {
        Options {
            csv_path: path.display().to_string(),
            apply,
            json: true,
            backup: false,
        }
    }

    #[test]
    fn parses_quoted_csv_and_maps_symbols() {
        let path = write_csv(&synthetic_csv());
        let rows = parse_csv(path.to_str().unwrap()).unwrap();
        assert_eq!(rows.len(), 10);
        let plan = build_plan(&rows).unwrap();
        let gold = plan
            .planned
            .iter()
            .find(|p| p.tx.symbol == "GC=F")
            .unwrap();
        assert_eq!(gold.tx.category, AssetCategory::Commodity);
        assert_eq!(gold.tx.quantity, dec!(2));
        assert_eq!(gold.tx.price_per, dec!(99)); // 198 / 2, full-precision Decimal
        assert_eq!(gold.tx.date, "2026-01-06");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sync_trades_become_paired_cash_legs_and_plain_trades_get_none() {
        let path = write_csv(&synthetic_csv());
        let rows = parse_csv(path.to_str().unwrap()).unwrap();
        let plan = build_plan(&rows).unwrap();
        let legs: Vec<_> = plan
            .planned
            .iter()
            .filter(|p| p.kind == PlannedKind::SyncCashLeg)
            .collect();
        assert_eq!(legs.len(), 2);
        for leg in &legs {
            let trade = &plan.planned[leg.pair_with.unwrap()];
            assert_eq!(trade.kind, PlannedKind::Trade);
            assert_eq!(trade.tx.symbol, "BTC");
            // Opposite direction of the trade.
            assert_ne!(
                leg.tx.tx_type.increases_quantity(),
                trade.tx.tx_type.increases_quantity()
            );
        }
        // The non-sync GOLD buy has no cash leg at all.
        let gold_idx = plan
            .planned
            .iter()
            .position(|p| p.tx.symbol == "GC=F")
            .unwrap();
        assert!(plan
            .planned
            .iter()
            .all(|p| p.pair_with != Some(gold_idx)));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fx_conversion_pair_detected_and_excluded_from_external_capital() {
        let path = write_csv(&synthetic_csv());
        let rows = parse_csv(path.to_str().unwrap()).unwrap();
        let plan = build_plan(&rows).unwrap();
        assert_eq!(plan.fx_pairs.len(), 1);
        assert_eq!(plan.fx_pairs[0].direction, "gbp_to_usd");
        // implied 135/100 = 1.35
        assert_eq!(plan.fx_pairs[0].implied_usd_per_gbp, "1.35");
        let to_import: Vec<&PlannedTx> = plan.planned.iter().collect();
        // External: +1000 USD -200 USD +500 GBP @1.35 = 800 + 675 = 1475.
        // The conversion legs (-100 GBP, +135 USD) are excluded.
        assert_eq!(
            external_capital_usd(&plan, &to_import),
            dec!(1475.00)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn usd_balance_invariant_holds_on_synthetic_ledger() {
        let path = write_csv(&synthetic_csv());
        let b = backend();
        run(&b, opts(&path, true)).unwrap();
        let txs = list_transactions_backend(&b).unwrap();
        let nets = net_by_symbol(&txs);
        // USD: +1000 -200 +135 -450 +190 = 675
        assert_eq!(nets.get("USD").copied().unwrap_or_default(), dec!(675));
        // GBP: +500 -100 = 400
        assert_eq!(nets.get("GBP").copied().unwrap_or_default(), dec!(400));
        // BTC: +0.5 -0.2 = 0.3, GOLD: 2
        assert_eq!(nets.get("BTC").copied().unwrap_or_default(), dec!(0.3));
        assert_eq!(nets.get("GC=F").copied().unwrap_or_default(), dec!(2));
        // Paired legs are linked.
        let btc_buy = txs
            .iter()
            .find(|t| t.symbol == "BTC" && t.tx_type == TxType::Buy)
            .unwrap();
        let leg = txs
            .iter()
            .find(|t| t.id == btc_buy.paired_tx_id.unwrap())
            .unwrap();
        assert_eq!(leg.symbol, "USD");
        assert_eq!(leg.tx_type, TxType::Sell);
        assert_eq!(leg.quantity, dec!(450));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn import_is_idempotent() {
        let path = write_csv(&synthetic_csv());
        let b = backend();
        run(&b, opts(&path, true)).unwrap();
        let count1 = list_transactions_backend(&b).unwrap().len();
        run(&b, opts(&path, true)).unwrap();
        let count2 = list_transactions_backend(&b).unwrap().len();
        assert_eq!(count1, count2, "re-running the import must be a no-op");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dry_run_makes_no_writes() {
        let path = write_csv(&synthetic_csv());
        let b = backend();
        run(&b, opts(&path, false)).unwrap();
        assert!(list_transactions_backend(&b).unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reconciliation_classifies_existing_rows() {
        let path = write_csv(&synthetic_csv());
        let b = backend();
        let conn = match &b {
            BackendConnection::Sqlite { conn } => conn,
            _ => unreachable!(),
        };
        // Direct match: same qty/side/symbol near the CSV GOLD fill.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "GC=F".into(),
                category: AssetCategory::Commodity,
                tx_type: TxType::Buy,
                quantity: dec!(2),
                price_per: dec!(101),
                currency: "USD".into(),
                date: "2026-01-06".into(),
                notes: Some("hand entry".into()),
            },
        )
        .unwrap();
        // Aggregate: BTC quantity matching no single fill, but symbol traded.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "BTC".into(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(0.31),
                price_per: dec!(900),
                currency: "USD".into(),
                date: "2026-01-09".into(),
                notes: None,
            },
        )
        .unwrap();
        // Conflict: symbol with no CSV activity.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "AAPL".into(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(100),
                currency: "USD".into(),
                date: "2026-01-05".into(),
                notes: None,
            },
        )
        .unwrap();
        // Kept: post-window row.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".into(),
                category: AssetCategory::Cash,
                tx_type: TxType::Sell,
                quantity: dec!(50),
                price_per: Decimal::ONE,
                currency: "USD".into(),
                date: "2026-02-01".into(),
                notes: Some("post-window spend".into()),
            },
        )
        .unwrap();
        // Kept: in-window deliberate cash flow with no CSV counterpart.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".into(),
                category: AssetCategory::Cash,
                tx_type: TxType::Sell,
                quantity: dec!(77),
                price_per: Decimal::ONE,
                currency: "USD".into(),
                date: "2026-01-08".into(),
                notes: Some("cash spend logged by hand".into()),
            },
        )
        .unwrap();
        // Superseded: set-cash baseline.
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".into(),
                category: AssetCategory::Cash,
                tx_type: TxType::Buy,
                quantity: dec!(5000),
                price_per: Decimal::ONE,
                currency: "USD".into(),
                date: "2026-01-04".into(),
                notes: Some("Set via pftui set-cash".into()),
            },
        )
        .unwrap();

        let rows = parse_csv(path.to_str().unwrap()).unwrap();
        let plan = build_plan(&rows).unwrap();
        let existing = list_transactions_backend(&b).unwrap();
        let recon = classify_existing(&existing, &plan);
        let class_of = |sym: &str, date: &str| {
            recon
                .iter()
                .find(|r| r.symbol == sym && r.date == date)
                .map(|r| r.classification)
                .unwrap()
        };
        assert_eq!(class_of("GC=F", "2026-01-06"), Classification::Superseded);
        assert_eq!(class_of("BTC", "2026-01-09"), Classification::Superseded);
        assert_eq!(class_of("AAPL", "2026-01-05"), Classification::Conflict);
        assert_eq!(class_of("USD", "2026-02-01"), Classification::Kept);
        assert_eq!(class_of("USD", "2026-01-08"), Classification::Kept);
        assert_eq!(class_of("USD", "2026-01-04"), Classification::Superseded);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn apply_deletes_superseded_and_keeps_kept() {
        let path = write_csv(&synthetic_csv());
        let b = backend();
        let conn = match &b {
            BackendConnection::Sqlite { conn } => conn,
            _ => unreachable!(),
        };
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "GC=F".into(),
                category: AssetCategory::Commodity,
                tx_type: TxType::Buy,
                quantity: dec!(2),
                price_per: dec!(101),
                currency: "USD".into(),
                date: "2026-01-06".into(),
                notes: Some("hand entry to supersede".into()),
            },
        )
        .unwrap();
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".into(),
                category: AssetCategory::Cash,
                tx_type: TxType::Sell,
                quantity: dec!(50),
                price_per: Decimal::ONE,
                currency: "USD".into(),
                date: "2026-02-01".into(),
                notes: Some("post-window spend".into()),
            },
        )
        .unwrap();
        // Paired superseded pair: hand BTC trade + its auto cash leg,
        // linked both ways — apply must unlink before deleting (FK is ON).
        let btc_id = insert_transaction(
            conn,
            &NewTransaction {
                symbol: "BTC".into(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(0.5),
                price_per: dec!(900),
                currency: "USD".into(),
                date: "2026-01-09".into(),
                notes: Some("hand BTC entry".into()),
            },
        )
        .unwrap();
        let leg_id = insert_transaction(
            conn,
            &NewTransaction {
                symbol: "USD".into(),
                category: AssetCategory::Cash,
                tx_type: TxType::Sell,
                quantity: dec!(450),
                price_per: Decimal::ONE,
                currency: "USD".into(),
                date: "2026-01-09".into(),
                notes: Some("hand cash leg".into()),
            },
        )
        .unwrap();
        set_paired_transaction(conn, btc_id, Some(leg_id)).unwrap();
        set_paired_transaction(conn, leg_id, Some(btc_id)).unwrap();

        run(&b, opts(&path, true)).unwrap();
        let txs = list_transactions_backend(&b).unwrap();
        assert!(!txs
            .iter()
            .any(|t| t.notes.as_deref() == Some("hand entry to supersede")));
        assert!(!txs.iter().any(|t| t.id == btc_id || t.id == leg_id));
        assert!(txs
            .iter()
            .any(|t| t.notes.as_deref() == Some("post-window spend")));
        // Carry-over: operator context lands on the replacing CSV fill.
        let gold = txs.iter().find(|t| t.symbol == "GC=F").unwrap();
        assert!(gold
            .notes
            .as_deref()
            .unwrap_or_default()
            .contains("operator context"));
        // GOLD position is the CSV's, not doubled.
        assert_eq!(
            net_by_symbol(&txs).get("GC=F").copied().unwrap_or_default(),
            dec!(2)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn extract_delta_keys_finds_markers() {
        let keys = extract_delta_keys(
            "Delta import [delta:BUY|2026-01-06T09:00:30.000Z|GOLD|2] tail [delta:FLOW|x|USD|1]",
        );
        assert_eq!(keys.len(), 2);
        assert!(keys[0].starts_with("BUY|"));
    }

    #[test]
    fn unmatched_sync_leg_fails_loudly() {
        let csv = [
            HEADER,
            r#""2026-01-10T15:00:00.000Z","DEPOSIT",190,"USD (United States dollar)","FIAT",,,"eToro","OTHER","MY_WALLET",,,,"SYNC-BASE-HOLDINGS_SELL_BTC/USD",false,"#,
        ]
        .join("\n");
        let path = write_csv(&csv);
        let rows = parse_csv(path.to_str().unwrap()).unwrap();
        assert!(build_plan(&rows).is_err());
        let _ = std::fs::remove_file(path);
    }
}
