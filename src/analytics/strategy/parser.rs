//! Strategy-expression DSL — AST, tokenizer, and a Pratt (precedence-climbing)
//! parser.
//!
//! The language describes trade conditions over time series. Two value kinds
//! flow through one untyped `Expr` tree; the type (numeric series vs boolean
//! series) is resolved at evaluation time so we get one parser and clean
//! type errors.
//!
//! Grammar (informal):
//!
//! ```text
//!   expr        := or_expr
//!   or_expr     := and_expr ( "or" and_expr )*
//!   and_expr    := not_expr ( "and" not_expr )*
//!   not_expr    := "not" not_expr | cmp_expr
//!   cmp_expr    := add_expr ( (">"|"<"|">="|"<="|"=="
//!                            |"crosses_above"|"crosses_below") add_expr )?
//!   add_expr    := mul_expr ( ("+"|"-") mul_expr )*
//!   mul_expr    := unary    ( ("*"|"/") unary )*
//!   unary       := "-" unary | postfix
//!   postfix     := atom ( "@" timeframe )*
//!   atom        := number | "(" expr ")" | call | ident
//!   call        := ident "(" [ arg ( "," arg )* ] ")"
//!   timeframe   := "daily" | "weekly" | "monthly"
//! ```
//!
//! - A bare field name (`close`, `open`, `high`, `low`, `volume`) means that
//!   field of the backtest's primary `--asset`.
//! - `close(SYM)` selects another symbol's field. `SYM` is an alias or ticker
//!   alias resolved by the [`super::resolver`].
//! - A bare identifier that is not a field/keyword/function (e.g. `us10y`,
//!   `fed_funds`) means `close(that-symbol)` — the symbol's level.
//! - Indicator calls: `sma(series, period)`, `ema(series, period)`,
//!   `rsi(period)` or `rsi(series, period)`.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeframe {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceField {
    Close,
    Open,
    High,
    Low,
    Volume,
}

impl PriceField {
    fn from_name(name: &str) -> Option<PriceField> {
        match name {
            "close" => Some(PriceField::Close),
            "open" => Some(PriceField::Open),
            "high" => Some(PriceField::High),
            "low" => Some(PriceField::Low),
            "volume" => Some(PriceField::Volume),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossDir {
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorKind {
    Sma,
    Ema,
    Rsi,
}

/// Window transforms over an already-evaluated numeric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// Rolling maximum over the last `n` bars.
    Highest,
    /// Rolling minimum over the last `n` bars.
    Lowest,
    /// The value `n` bars ago (lag).
    Ago,
    /// Percent change over `n` bars: 100·(x[i]/x[i-n] − 1).
    PctChange,
}

/// One untyped expression node. Numeric and boolean operators share the tree;
/// the evaluator enforces types.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    /// A price/volume field of a symbol (None = the primary asset).
    Field {
        field: PriceField,
        symbol: Option<String>,
    },
    Indicator {
        kind: IndicatorKind,
        input: Box<Expr>,
        period: usize,
    },
    /// A window transform (highest/lowest/ago/pct_change) over a numeric series.
    Window {
        kind: WindowKind,
        input: Box<Expr>,
        n: usize,
    },
    /// Absolute value of a numeric series.
    Abs(Box<Expr>),
    Neg(Box<Expr>),
    Arith(ArithOp, Box<Expr>, Box<Expr>),
    Timeframed(Box<Expr>, Timeframe),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    Cross(CrossDir, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

// ----------------------------------------------------------------------------
// Tokenizer
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Gt,
    Lt,
    Ge,
    Le,
    EqEq,
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    At,
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
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
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
                    bail!("unexpected '='; use '==' for equality");
                }
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let n: f64 = s
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid number: {s}"))?;
                out.push(Tok::Num(n));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                out.push(Tok::Ident(s));
            }
            _ => bail!("unexpected character: '{c}'"),
        }
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
// Parser (precedence climbing)
// ----------------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
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
    fn eat(&mut self, t: &Tok) -> Result<()> {
        match self.next() {
            Some(ref got) if got == t => Ok(()),
            other => bail!("expected {t:?}, found {other:?}"),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::Ident(k)) if k == "or") {
            self.next();
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_not()?;
        while matches!(self.peek(), Some(Tok::Ident(k)) if k == "and") {
            self.next();
            let rhs = self.parse_not()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Some(Tok::Ident(k)) if k == "not") {
            self.next();
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let lhs = self.parse_add()?;
        match self.peek() {
            Some(Tok::Gt) => {
                self.next();
                Ok(Expr::Cmp(CmpOp::Gt, Box::new(lhs), Box::new(self.parse_add()?)))
            }
            Some(Tok::Lt) => {
                self.next();
                Ok(Expr::Cmp(CmpOp::Lt, Box::new(lhs), Box::new(self.parse_add()?)))
            }
            Some(Tok::Ge) => {
                self.next();
                Ok(Expr::Cmp(CmpOp::Ge, Box::new(lhs), Box::new(self.parse_add()?)))
            }
            Some(Tok::Le) => {
                self.next();
                Ok(Expr::Cmp(CmpOp::Le, Box::new(lhs), Box::new(self.parse_add()?)))
            }
            Some(Tok::EqEq) => {
                self.next();
                Ok(Expr::Cmp(CmpOp::Eq, Box::new(lhs), Box::new(self.parse_add()?)))
            }
            Some(Tok::Ident(k)) if k == "crosses_above" => {
                self.next();
                Ok(Expr::Cross(
                    CrossDir::Above,
                    Box::new(lhs),
                    Box::new(self.parse_add()?),
                ))
            }
            Some(Tok::Ident(k)) if k == "crosses_below" => {
                self.next();
                Ok(Expr::Cross(
                    CrossDir::Below,
                    Box::new(lhs),
                    Box::new(self.parse_add()?),
                ))
            }
            _ => Ok(lhs),
        }
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Tok::Plus) => {
                    self.next();
                    lhs = Expr::Arith(ArithOp::Add, Box::new(lhs), Box::new(self.parse_mul()?));
                }
                Some(Tok::Minus) => {
                    self.next();
                    lhs = Expr::Arith(ArithOp::Sub, Box::new(lhs), Box::new(self.parse_mul()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Tok::Star) => {
                    self.next();
                    lhs = Expr::Arith(ArithOp::Mul, Box::new(lhs), Box::new(self.parse_unary()?));
                }
                Some(Tok::Slash) => {
                    self.next();
                    lhs = Expr::Arith(ArithOp::Div, Box::new(lhs), Box::new(self.parse_unary()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Some(Tok::Minus)) {
            self.next();
            return Ok(Expr::Neg(Box::new(self.parse_unary()?)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut e = self.parse_atom()?;
        while matches!(self.peek(), Some(Tok::At)) {
            self.next();
            let tf = match self.next() {
                Some(Tok::Ident(name)) => match name.as_str() {
                    "daily" => Timeframe::Daily,
                    "weekly" => Timeframe::Weekly,
                    "monthly" => Timeframe::Monthly,
                    other => bail!("unknown timeframe '@{other}' (use daily|weekly|monthly)"),
                },
                other => bail!("expected timeframe after '@', found {other:?}"),
            };
            e = Expr::Timeframed(Box::new(e), tf);
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.parse_expr()?;
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.parse_call(&name)
                } else if let Some(field) = PriceField::from_name(&name) {
                    Ok(Expr::Field {
                        field,
                        symbol: None,
                    })
                } else if is_reserved(&name) {
                    bail!("unexpected keyword '{name}'")
                } else {
                    // Bare symbol → its close level.
                    Ok(Expr::Field {
                        field: PriceField::Close,
                        symbol: Some(name),
                    })
                }
            }
            other => bail!("unexpected token: {other:?}"),
        }
    }

    fn parse_call(&mut self, name: &str) -> Result<Expr> {
        self.eat(&Tok::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek(), Some(Tok::RParen)) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek(), Some(Tok::Comma)) {
                    self.next();
                } else {
                    break;
                }
            }
        }
        self.eat(&Tok::RParen)?;

        // Field accessor: close(SYM), high(SYM), ...
        if let Some(field) = PriceField::from_name(name) {
            let symbol = match args.as_slice() {
                [Expr::Field {
                    field: PriceField::Close,
                    symbol: Some(sym),
                }] => Some(sym.clone()),
                [] => None,
                _ => bail!("{name}(SYM) takes a single symbol argument"),
            };
            return Ok(Expr::Field { field, symbol });
        }

        // Window transforms: highest/lowest/ago/pct_change(series, n) and abs(series).
        if name == "abs" {
            return match args.as_slice() {
                [input] => Ok(Expr::Abs(Box::new(input.clone()))),
                _ => bail!("abs(series) takes one argument"),
            };
        }
        if let Some(wkind) = match name {
            "highest" => Some(WindowKind::Highest),
            "lowest" => Some(WindowKind::Lowest),
            "ago" => Some(WindowKind::Ago),
            "pct_change" => Some(WindowKind::PctChange),
            _ => None,
        } {
            return match args.as_slice() {
                [input, n] => Ok(Expr::Window {
                    kind: wkind,
                    input: Box::new(input.clone()),
                    n: expect_period(n)?,
                }),
                _ => bail!("{name}(series, n) takes a series and a positive integer"),
            };
        }

        // Indicators.
        let kind = match name {
            "sma" => IndicatorKind::Sma,
            "ema" => IndicatorKind::Ema,
            "rsi" => IndicatorKind::Rsi,
            other => bail!("unknown function '{other}'"),
        };
        let (input, period) = match kind {
            IndicatorKind::Sma | IndicatorKind::Ema => match args.as_slice() {
                [input, period] => (input.clone(), expect_period(period)?),
                _ => bail!("{name}(series, period) takes exactly two arguments"),
            },
            IndicatorKind::Rsi => match args.as_slice() {
                // rsi(period) defaults the input to the primary asset's close.
                [period] => (
                    Expr::Field {
                        field: PriceField::Close,
                        symbol: None,
                    },
                    expect_period(period)?,
                ),
                [input, period] => (input.clone(), expect_period(period)?),
                _ => bail!("rsi(period) or rsi(series, period)"),
            },
        };
        Ok(Expr::Indicator {
            kind,
            input: Box::new(input),
            period,
        })
    }
}

fn expect_period(e: &Expr) -> Result<usize> {
    match e {
        Expr::Num(n) if *n >= 1.0 && n.fract() == 0.0 => Ok(*n as usize),
        _ => bail!("period must be a positive integer literal"),
    }
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "and" | "or" | "not" | "crosses_above" | "crosses_below" | "daily" | "weekly" | "monthly"
    )
}

/// Parse a full strategy expression string into an [`Expr`].
pub fn parse(src: &str) -> Result<Expr> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        bail!("empty expression");
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.parse_expr()?;
    if p.pos != p.toks.len() {
        bail!("unexpected trailing tokens after expression");
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_field() {
        assert_eq!(
            parse("close").unwrap(),
            Expr::Field {
                field: PriceField::Close,
                symbol: None
            }
        );
    }

    #[test]
    fn parses_symbol_field() {
        assert_eq!(
            parse("close(BTC)").unwrap(),
            Expr::Field {
                field: PriceField::Close,
                symbol: Some("BTC".to_string())
            }
        );
    }

    #[test]
    fn bare_symbol_is_close() {
        assert_eq!(
            parse("us10y").unwrap(),
            Expr::Field {
                field: PriceField::Close,
                symbol: Some("us10y".to_string())
            }
        );
    }

    #[test]
    fn parses_sma_with_timeframe() {
        let e = parse("sma(close, 200) @weekly").unwrap();
        match e {
            Expr::Timeframed(inner, Timeframe::Weekly) => match *inner {
                Expr::Indicator {
                    kind: IndicatorKind::Sma,
                    period: 200,
                    ..
                } => {}
                other => panic!("unexpected inner: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_crossing() {
        let e = parse("close crosses_above sma(close, 200) @weekly").unwrap();
        assert!(matches!(e, Expr::Cross(CrossDir::Above, _, _)));
    }

    #[test]
    fn parses_rsi_monthly_threshold() {
        let e = parse("rsi(14) @monthly < 90").unwrap();
        assert!(matches!(e, Expr::Cmp(CmpOp::Lt, _, _)));
    }

    #[test]
    fn parses_boolean_combination() {
        let e = parse("close > sma(close, 50) and rsi(14) < 70 and not (close < open)").unwrap();
        assert!(matches!(e, Expr::And(_, _)));
    }

    #[test]
    fn precedence_and_binds_tighter_than_or() {
        // a or b and c  ==  a or (b and c)
        let e = parse("close > open or close > high and close > low").unwrap();
        assert!(matches!(e, Expr::Or(_, _)));
    }

    #[test]
    fn rejects_unknown_function() {
        assert!(parse("frobnicate(close, 10)").is_err());
    }

    #[test]
    fn rejects_trailing_tokens() {
        assert!(parse("close 200").is_err());
    }

    #[test]
    fn rejects_single_equals() {
        assert!(parse("close = 5").is_err());
    }
}
