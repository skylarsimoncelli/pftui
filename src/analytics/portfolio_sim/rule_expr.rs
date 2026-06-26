//! A small, dedicated scalar `when` expression layer for the positioning
//! rule engine (POSITIONING-MODELS.md §3.2 step 2). **Fresh** — deliberately
//! NOT the single-asset series DSL in `analytics/strategy/parser.rs`; that
//! language evaluates whole time series and bends awkwardly to a point-in-time,
//! multi-symbol "does this rule fire at date T?" question. This one is tiny:
//!
//! ```text
//!   expr    := or
//!   or      := and ( "or" and )*
//!   and     := not ( "and" not )*
//!   not     := "not" not | cmp
//!   cmp     := primary ( ( ">=" | "<=" | ">" | "<" | "==" | "!=" ) primary )?
//!   primary := number
//!            | "(" expr ")"
//!            | accessor
//!            | param            (a bare identifier resolved from [params])
//!   accessor := ident "(" [ string ( "," string )* ] ")" [ "@" timeframe ]
//!   timeframe := "daily" | "weekly" | "monthly"
//!   string   := "'" ...symbol... "'"
//! ```
//!
//! - A numeric LITERAL or a bare PARAM identifier yields a number.
//! - An `accessor('SYM')` call yields a number (e.g. `cycle_bottom_met` →
//!   `met_count`). See [`super::accessors`].
//! - The **top-level** expression MUST be boolean (a comparison or a boolean
//!   combination). A bare number at top level is rejected at validate time —
//!   a rule whose `when` is not a predicate is a correctness lie.
//!
//! Parsing happens once at spec-resolve time ([`parse_and_validate`]); the
//! validated [`Expr`] is stored on [`super::actions::Condition::Signal`] and
//! evaluated point-in-time at each rebalance date via [`eval_bool`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::accessors::{self, AtDateCtx};
use crate::analytics::cycle_signals::SignalTimeframe;

/// A scalar comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
    Ne,
}

/// The validated `when` expression AST. Params are already folded to [`Expr::Num`]
/// at parse time; accessors are validated (name/arity/symbol) at parse time and
/// looked up again at eval time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// A numeric literal (also the resolved value of a `[params]` reference).
    Num(f64),
    /// A signal accessor call: `name(args…)` with an optional `@timeframe`.
    Accessor {
        name: String,
        args: Vec<String>,
        tf: Option<SignalTimeframe>,
    },
    /// A scalar comparison — the only way to turn numbers into a bool.
    Compare {
        op: CmpOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

/// A runtime value flowing through [`eval`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
}

/// The static type of a (sub)expression, resolved at validate time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Num,
    Bool,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Str(String),
    LParen,
    RParen,
    Comma,
    At,
    Ge,
    Le,
    Gt,
    Lt,
    EqEq,
    Ne,
}

fn tokenize(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '@' => {
                out.push(Tok::At);
                i += 1;
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Le);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            '=' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::EqEq);
                    i += 2;
                } else {
                    bail!("unexpected '=' (did you mean '=='?)");
                }
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Ne);
                    i += 2;
                } else {
                    bail!("unexpected '!' (did you mean '!='?)");
                }
            }
            '\'' | '"' => {
                let quote = c;
                let mut s = String::new();
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == quote {
                        closed = true;
                        i += 1;
                        break;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    bail!("unterminated string literal in `when` expression");
                }
                out.push(Tok::Str(s));
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let mut s = String::new();
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    s.push(chars[i]);
                    i += 1;
                }
                let n: f64 = s
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid number literal '{s}'"))?;
                out.push(Tok::Num(n));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let mut s = String::new();
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(Tok::Ident(s));
            }
            other => bail!("unexpected character '{other}' in `when` expression"),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Parser (recursive descent)
// ---------------------------------------------------------------------------

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    params: &'a BTreeMap<String, f64>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Is the next identifier the keyword `kw`?
    fn peek_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s == kw)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while self.peek_kw("or") {
            self.pos += 1;
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_not()?;
        while self.peek_kw("and") {
            self.pos += 1;
            let rhs = self.parse_not()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek_kw("not") {
            self.pos += 1;
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let lhs = self.parse_primary()?;
        let op = match self.peek() {
            Some(Tok::Ge) => Some(CmpOp::Ge),
            Some(Tok::Le) => Some(CmpOp::Le),
            Some(Tok::Gt) => Some(CmpOp::Gt),
            Some(Tok::Lt) => Some(CmpOp::Lt),
            Some(Tok::EqEq) => Some(CmpOp::Eq),
            Some(Tok::Ne) => Some(CmpOp::Ne),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let rhs = self.parse_primary()?;
            Ok(Expr::Compare {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.parse_or()?;
                if !self.eat(&Tok::RParen) {
                    bail!("expected ')' to close a parenthesized expression");
                }
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if name == "and" || name == "or" || name == "not" {
                    bail!("unexpected keyword '{name}' where a value was expected");
                }
                if self.eat(&Tok::LParen) {
                    // Accessor call.
                    let mut args = Vec::new();
                    if !self.eat(&Tok::RParen) {
                        loop {
                            match self.next() {
                                Some(Tok::Str(s)) => args.push(s),
                                other => bail!(
                                    "accessor '{name}' arguments must be quoted symbols (e.g. '{}'), got {:?}",
                                    "BTC-USD",
                                    other
                                ),
                            }
                            if self.eat(&Tok::Comma) {
                                continue;
                            }
                            if self.eat(&Tok::RParen) {
                                break;
                            }
                            bail!("expected ',' or ')' in accessor '{name}' argument list");
                        }
                    }
                    let tf = if self.eat(&Tok::At) {
                        match self.next() {
                            Some(Tok::Ident(tf)) => Some(SignalTimeframe::parse(&tf)?),
                            other => bail!("expected a timeframe after '@', got {:?}", other),
                        }
                    } else {
                        None
                    };
                    Ok(Expr::Accessor { name, args, tf })
                } else {
                    // A bare identifier MUST be a known param; otherwise it is a
                    // typo'd accessor (missing parens) or an unknown name — reject.
                    match self.params.get(&name) {
                        Some(v) => Ok(Expr::Num(*v)),
                        None => bail!(
                            "unknown identifier '{name}' in `when`: it is neither a declared [params] value nor an accessor call (accessors need parentheses, e.g. `{name}('BTC-USD')`)"
                        ),
                    }
                }
            }
            other => bail!("unexpected token {:?} where a value was expected", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation (type-check + accessor registry check)
// ---------------------------------------------------------------------------

/// Parse `src` against `params`, then validate every accessor against the
/// registry and `universe`, and require the whole expression to be boolean.
/// Returns the validated [`Expr`] ready to store on a [`Condition::Signal`].
pub fn parse_and_validate(
    src: &str,
    params: &BTreeMap<String, f64>,
    universe: &BTreeSet<String>,
) -> Result<Expr> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        bail!("empty `when` expression");
    }
    let mut p = Parser {
        toks,
        pos: 0,
        params,
    };
    let expr = p.parse_or()?;
    if p.pos != p.toks.len() {
        bail!(
            "trailing tokens after a complete `when` expression (near token {})",
            p.pos
        );
    }
    let kind = validate(&expr, universe)?;
    if kind != Kind::Bool {
        bail!(
            "`when` expression must be a boolean predicate (a comparison or and/or/not of comparisons); a bare number/accessor is not a rule condition"
        );
    }
    Ok(expr)
}

fn validate(expr: &Expr, universe: &BTreeSet<String>) -> Result<Kind> {
    match expr {
        Expr::Num(_) => Ok(Kind::Num),
        Expr::Accessor { name, args, .. } => {
            let def = accessors::lookup(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown signal accessor '{name}': known accessors are {}",
                    accessors::known_names().join(", ")
                )
            })?;
            if args.len() != def.arity {
                bail!(
                    "accessor '{name}' takes {} argument(s), got {}",
                    def.arity,
                    args.len()
                );
            }
            for sym in args {
                if !universe.contains(sym) {
                    bail!(
                        "accessor '{name}' references symbol '{sym}', which is not in the model universe"
                    );
                }
            }
            Ok(Kind::Num)
        }
        Expr::Compare { lhs, rhs, .. } => {
            let l = validate(lhs, universe)?;
            let r = validate(rhs, universe)?;
            if l != Kind::Num || r != Kind::Num {
                bail!("comparison operands must be numeric (a number, param, or accessor)");
            }
            Ok(Kind::Bool)
        }
        Expr::And(a, b) | Expr::Or(a, b) => {
            if validate(a, universe)? != Kind::Bool || validate(b, universe)? != Kind::Bool {
                bail!("'and'/'or' operands must be boolean");
            }
            Ok(Kind::Bool)
        }
        Expr::Not(a) => {
            if validate(a, universe)? != Kind::Bool {
                bail!("'not' operand must be boolean");
            }
            Ok(Kind::Bool)
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Evaluate a validated expression to a bool at the date held by `ctx`. The
/// expression is known boolean at the top level (validated at parse time).
///
/// ## Insufficient-history hardening (the `not` footgun, [R])
/// An accessor with too-shallow history returns `NaN`. A bare comparison touching
/// `NaN` is already FALSE ([`compare`]), but `not(<NaN comparison>)` would flip
/// that FALSE to TRUE and fire a rule on *missing data* — exactly backwards. So
/// we track whether ANY accessor evaluated during the `when` was insufficient
/// (`NaN`); if so the whole rule is treated as **not-firing**, regardless of the
/// boolean structure (`not`/`and`/`or`). A rule that cannot be computed never
/// fires. (Trade-off: an `or` whose evaluated branch hit a `NaN` is also
/// suppressed — the conservative, safe-by-default choice.)
pub fn eval_bool(expr: &Expr, ctx: &mut AtDateCtx) -> Result<bool> {
    let mut insufficient = false;
    let v = eval(expr, ctx, &mut insufficient)?;
    let b = match v {
        Value::Bool(b) => b,
        Value::Num(_) => {
            bail!("internal: `when` top-level evaluated to a number (validation bug)")
        }
    };
    // Any insufficient-history accessor reached during evaluation → no fire.
    Ok(if insufficient { false } else { b })
}

fn eval(expr: &Expr, ctx: &mut AtDateCtx, insufficient: &mut bool) -> Result<Value> {
    Ok(match expr {
        Expr::Num(n) => Value::Num(*n),
        Expr::Accessor { name, args, tf } => {
            let n = accessors::eval_accessor(name, args, *tf, ctx)?;
            if n.is_nan() {
                *insufficient = true;
            }
            Value::Num(n)
        }
        Expr::Compare { op, lhs, rhs } => {
            let l = as_num(eval(lhs, ctx, insufficient)?)?;
            let r = as_num(eval(rhs, ctx, insufficient)?)?;
            Value::Bool(compare(*op, l, r))
        }
        Expr::And(a, b) => {
            // Short-circuit so an insufficient-history accessor on the rhs is
            // never reached once the lhs is already false.
            let l = as_bool(eval(a, ctx, insufficient)?)?;
            Value::Bool(l && as_bool(eval(b, ctx, insufficient)?)?)
        }
        Expr::Or(a, b) => {
            let l = as_bool(eval(a, ctx, insufficient)?)?;
            Value::Bool(l || as_bool(eval(b, ctx, insufficient)?)?)
        }
        Expr::Not(a) => Value::Bool(!as_bool(eval(a, ctx, insufficient)?)?),
    })
}

/// Does `expr` reference a regime (macro-series) accessor anywhere? The panel
/// loader uses this to decide whether it must also source the `REGIME_SYMBOLS`
/// macro series for a rule set.
pub fn uses_regime(expr: &Expr) -> bool {
    match expr {
        Expr::Accessor { name, .. } => accessors::is_regime_accessor(name),
        Expr::Num(_) => false,
        Expr::Compare { lhs, rhs, .. } => uses_regime(lhs) || uses_regime(rhs),
        Expr::And(a, b) | Expr::Or(a, b) => uses_regime(a) || uses_regime(b),
        Expr::Not(a) => uses_regime(a),
    }
}

/// Compare two numbers. **Insufficient-history sentinel:** an accessor with too
/// little data returns `NaN`; any comparison touching a `NaN` is FALSE, so a
/// rule that depends on an uncomputable signal does NOT fire (safe default).
fn compare(op: CmpOp, l: f64, r: f64) -> bool {
    if l.is_nan() || r.is_nan() {
        return false;
    }
    match op {
        CmpOp::Ge => l >= r,
        CmpOp::Le => l <= r,
        CmpOp::Gt => l > r,
        CmpOp::Lt => l < r,
        CmpOp::Eq => l == r,
        CmpOp::Ne => l != r,
    }
}

fn as_num(v: Value) -> Result<f64> {
    match v {
        Value::Num(n) => Ok(n),
        Value::Bool(_) => bail!("expected a number, got a boolean"),
    }
}

fn as_bool(v: Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(b),
        Value::Num(_) => bail!("expected a boolean, got a number"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> BTreeMap<String, f64> {
        let mut m = BTreeMap::new();
        m.insert("dip_threshold".to_string(), 5.0);
        m
    }
    fn universe() -> BTreeSet<String> {
        ["BTC-USD".to_string(), "GC=F".to_string()].into_iter().collect()
    }

    #[test]
    fn parses_accessor_comparison() {
        let e = parse_and_validate("cycle_bottom_met('BTC-USD') >= 5", &params(), &universe())
            .unwrap();
        assert!(matches!(e, Expr::Compare { op: CmpOp::Ge, .. }));
    }

    #[test]
    fn param_resolves_on_rhs() {
        let e =
            parse_and_validate("cycle_bottom_met('BTC-USD') >= dip_threshold", &params(), &universe())
                .unwrap();
        if let Expr::Compare { rhs, .. } = e {
            assert_eq!(*rhs, Expr::Num(5.0));
        } else {
            panic!("expected comparison");
        }
    }

    #[test]
    fn parses_timeframe_and_boolean_combo() {
        let e = parse_and_validate(
            "cycle_bottom_met('BTC-USD')@weekly >= 4 and not cycle_top_met('GC=F') >= 5",
            &params(),
            &universe(),
        )
        .unwrap();
        assert!(matches!(e, Expr::And(..)));
    }

    #[test]
    fn rejects_unknown_accessor() {
        let err = parse_and_validate("cyber_dot_up('BTC-USD') >= 1", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown signal accessor"), "got: {err}");
    }

    #[test]
    fn rejects_wrong_arity() {
        let err = parse_and_validate("cycle_bottom_met() >= 5", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("takes 1 argument"), "got: {err}");
    }

    #[test]
    fn rejects_non_universe_symbol() {
        let err = parse_and_validate("cycle_bottom_met('DOGE') >= 5", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("not in the model universe"), "got: {err}");
    }

    #[test]
    fn rejects_non_bool_top_level() {
        let err = parse_and_validate("cycle_bottom_met('BTC-USD')", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be a boolean predicate"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_param_identifier() {
        let err = parse_and_validate("cycle_bottom_met('BTC-USD') >= mystery", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown identifier 'mystery'"), "got: {err}");
    }

    #[test]
    fn accepts_regime_score_accessor() {
        // regime_score() is a known zero-arity accessor → validation accepts it
        // (P3b previously listed only cycle_* and rejected regime rules).
        // (Negative thresholds are carried via [params]; the DSL has no negative
        // literal token, so test acceptance with a non-negative comparison here.)
        let e = parse_and_validate("regime_score() >= 2", &params(), &universe()).unwrap();
        assert!(matches!(e, Expr::Compare { op: CmpOp::Ge, .. }));
        assert!(uses_regime(&e));
    }

    #[test]
    fn rejects_regime_score_with_argument() {
        // Arity 0: a symbol argument to regime_score is a clear error.
        let err = parse_and_validate("regime_score('SPY') >= 1", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("takes 0 argument"), "got: {err}");
    }

    /// The `not` footgun fix: a `not(<insufficient-history comparison>)` must NOT
    /// fire. With shallow BTC-USD history `cycle_bottom_met` is `NaN`; the inner
    /// comparison is FALSE, and naive `not` would flip it TRUE. The insufficient
    /// flag suppresses firing.
    #[test]
    fn not_of_insufficient_history_does_not_fire() {
        use super::super::accessors::{AtDateCtx, Memo};
        use super::super::PricePanel;
        use chrono::{Days, NaiveDate};
        use rust_decimal::Decimal;

        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        // Far too few bars for cycle_bottom_signals → NaN.
        let series: Vec<(NaiveDate, Decimal)> = (0..20)
            .map(|i| (start + Days::new(i), Decimal::from(100 + i as i64)))
            .collect();
        let mut panel = PricePanel::new();
        panel.insert_series("BTC-USD", series);

        let uni: BTreeSet<String> = ["BTC-USD".to_string()].into_iter().collect();
        let expr =
            parse_and_validate("not cycle_bottom_met('BTC-USD') >= 5", &params(), &uni).unwrap();

        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: start + Days::new(19),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        assert!(
            !eval_bool(&expr, &mut ctx).unwrap(),
            "not(insufficient) must NOT fire (NaN suppresses the rule)"
        );
    }

    #[test]
    fn rejects_unquoted_symbol() {
        // A bare (unquoted) symbol in an accessor arg list is rejected.
        let err = parse_and_validate("cycle_bottom_met(BTCUSD) >= 5", &params(), &universe())
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be quoted symbols"), "got: {err}");
    }
}
