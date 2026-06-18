//! Evaluator — walk an [`Expr`] against a [`Resolver`] producing either a
//! numeric series or a boolean series, both aligned to the master daily axis.
//!
//! Types are checked here (one untyped parse tree, two runtime value kinds):
//! arithmetic/indicators yield `Num`, comparisons/crossings/logic yield
//! `Bool`. Missing data propagates as `None` (an unknown bar never fires a
//! signal and never enters a return window).

use anyhow::{bail, Result};

use super::parser::{ArithOp, CmpOp, CrossDir, Expr, PriceField, Timeframe};
use super::resolver::Resolver;

#[derive(Debug, Clone)]
pub enum Val {
    Num(Vec<Option<f64>>),
    Bool(Vec<Option<bool>>),
}

impl Val {
    fn into_num(self) -> Result<Vec<Option<f64>>> {
        match self {
            Val::Num(v) => Ok(v),
            Val::Bool(_) => bail!("expected a numeric series but found a boolean condition"),
        }
    }
    fn into_bool(self) -> Result<Vec<Option<bool>>> {
        match self {
            Val::Bool(v) => Ok(v),
            Val::Num(_) => bail!("expected a boolean condition but found a numeric series"),
        }
    }
}

/// Evaluate `expr` into a boolean condition series. Convenience wrapper for
/// the engine, which always wants a condition.
pub fn eval_condition(expr: &Expr, resolver: &mut Resolver) -> Result<Vec<Option<bool>>> {
    eval(expr, Timeframe::Daily, resolver)?.into_bool()
}

pub fn eval(expr: &Expr, tf: Timeframe, resolver: &mut Resolver) -> Result<Val> {
    let n = resolver.master_len();
    match expr {
        Expr::Num(v) => Ok(Val::Num(vec![Some(*v); n])),
        Expr::Field { field, symbol } => Ok(Val::Num(resolver.field_series(
            symbol.as_deref(),
            *field,
            tf,
        )?)),
        Expr::Indicator {
            kind,
            input,
            period,
        } => {
            let (symbol, field) = as_field(input)?;
            Ok(Val::Num(resolver.indicator_series(
                *kind,
                symbol.as_deref(),
                field,
                *period,
                tf,
            )?))
        }
        Expr::Timeframed(inner, tf2) => eval(inner, *tf2, resolver),
        Expr::Neg(inner) => {
            let v = eval(inner, tf, resolver)?.into_num()?;
            Ok(Val::Num(v.into_iter().map(|x| x.map(|y| -y)).collect()))
        }
        Expr::Arith(op, l, r) => {
            let a = eval(l, tf, resolver)?.into_num()?;
            let b = eval(r, tf, resolver)?.into_num()?;
            Ok(Val::Num(zip_num(&a, &b, |x, y| arith(*op, x, y))))
        }
        Expr::Cmp(op, l, r) => {
            let a = eval(l, tf, resolver)?.into_num()?;
            let b = eval(r, tf, resolver)?.into_num()?;
            Ok(Val::Bool(zip_bool_from_num(&a, &b, |x, y| cmp(*op, x, y))))
        }
        Expr::Cross(dir, l, r) => {
            let a = eval(l, tf, resolver)?.into_num()?;
            let b = eval(r, tf, resolver)?.into_num()?;
            Ok(Val::Bool(cross(*dir, &a, &b)))
        }
        Expr::And(l, r) => {
            let a = eval(l, tf, resolver)?.into_bool()?;
            let b = eval(r, tf, resolver)?.into_bool()?;
            Ok(Val::Bool(zip_bool(&a, &b, |x, y| Some(x? && y?))))
        }
        Expr::Or(l, r) => {
            let a = eval(l, tf, resolver)?.into_bool()?;
            let b = eval(r, tf, resolver)?.into_bool()?;
            Ok(Val::Bool(zip_bool(&a, &b, |x, y| Some(x? || y?))))
        }
        Expr::Not(inner) => {
            let v = eval(inner, tf, resolver)?.into_bool()?;
            Ok(Val::Bool(v.into_iter().map(|x| x.map(|b| !b)).collect()))
        }
    }
}

fn as_field(e: &Expr) -> Result<(Option<String>, PriceField)> {
    match e {
        Expr::Field { field, symbol } => Ok((symbol.clone(), *field)),
        _ => bail!("indicator argument must be a price field (e.g. close, close(BTC)) in v1"),
    }
}

fn arith(op: ArithOp, x: f64, y: f64) -> Option<f64> {
    match op {
        ArithOp::Add => Some(x + y),
        ArithOp::Sub => Some(x - y),
        ArithOp::Mul => Some(x * y),
        ArithOp::Div => {
            if y == 0.0 {
                None
            } else {
                Some(x / y)
            }
        }
    }
}

fn cmp(op: CmpOp, x: f64, y: f64) -> bool {
    match op {
        CmpOp::Gt => x > y,
        CmpOp::Lt => x < y,
        CmpOp::Ge => x >= y,
        CmpOp::Le => x <= y,
        CmpOp::Eq => (x - y).abs() < f64::EPSILON,
    }
}

/// Strict crossing: the relation must flip between the previous and current
/// bar. `crosses_above` requires `lhs <= rhs` on bar `i-1` and `lhs > rhs` on
/// bar `i`. Bars with any missing operand (including the first bar) are
/// `None`, never a firing.
fn cross(dir: CrossDir, a: &[Option<f64>], b: &[Option<f64>]) -> Vec<Option<bool>> {
    let n = a.len().min(b.len());
    let mut out = vec![None; a.len()];
    for i in 1..n {
        let (p_a, p_b, c_a, c_b) = match (a[i - 1], b[i - 1], a[i], b[i]) {
            (Some(pa), Some(pb), Some(ca), Some(cb)) => (pa, pb, ca, cb),
            _ => continue,
        };
        out[i] = Some(match dir {
            CrossDir::Above => p_a <= p_b && c_a > c_b,
            CrossDir::Below => p_a >= p_b && c_a < c_b,
        });
    }
    out
}

fn zip_num<F: Fn(f64, f64) -> Option<f64>>(
    a: &[Option<f64>],
    b: &[Option<f64>],
    f: F,
) -> Vec<Option<f64>> {
    let n = a.len().min(b.len());
    (0..n)
        .map(|i| match (a[i], b[i]) {
            (Some(x), Some(y)) => f(x, y),
            _ => None,
        })
        .collect()
}

fn zip_bool_from_num<F: Fn(f64, f64) -> bool>(
    a: &[Option<f64>],
    b: &[Option<f64>],
    f: F,
) -> Vec<Option<bool>> {
    let n = a.len().min(b.len());
    (0..n)
        .map(|i| match (a[i], b[i]) {
            (Some(x), Some(y)) => Some(f(x, y)),
            _ => None,
        })
        .collect()
}

fn zip_bool<F: Fn(Option<bool>, Option<bool>) -> Option<bool>>(
    a: &[Option<bool>],
    b: &[Option<bool>],
    f: F,
) -> Vec<Option<bool>> {
    let n = a.len().min(b.len());
    (0..n).map(|i| f(a[i], b[i])).collect()
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::super::resolver::{Resolver, SeriesLoader};
    use super::*;
    use std::collections::HashMap;

    struct MapLoader(HashMap<String, Vec<(String, f64)>>);
    impl SeriesLoader for MapLoader {
        fn load(&self, symbol: &str, _f: PriceField) -> Result<Vec<(String, f64)>> {
            Ok(self.0.get(symbol).cloned().unwrap_or_default())
        }
    }

    fn series(vals: &[f64]) -> Vec<(String, f64)> {
        vals.iter()
            .enumerate()
            .map(|(i, v)| (format!("2021-02-{:02}", i + 1), *v))
            .collect()
    }

    fn resolver_for<'a>(loader: &'a MapLoader, dates: Vec<String>) -> Resolver<'a> {
        Resolver::new(dates, "X", loader)
    }

    #[test]
    fn comparison_evaluates_elementwise() {
        let raw = series(&[10.0, 20.0, 30.0]);
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = resolver_for(&loader, dates);
        let e = parse("close > 15").unwrap();
        let b = eval_condition(&e, &mut r).unwrap();
        assert_eq!(b, vec![Some(false), Some(true), Some(true)]);
    }

    #[test]
    fn crossing_fires_only_on_flip() {
        // close crosses above a flat threshold of 15 between bar 0 and 1.
        let raw = series(&[10.0, 20.0, 25.0, 12.0, 18.0]);
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = resolver_for(&loader, dates);
        let e = parse("close crosses_above 15").unwrap();
        let b = eval_condition(&e, &mut r).unwrap();
        // bar0 None; 10->20 cross up = true; 20->25 no; 25->12 no; 12->18 up=true
        assert_eq!(
            b,
            vec![None, Some(true), Some(false), Some(false), Some(true)]
        );
    }

    #[test]
    fn type_mismatch_errors() {
        let raw = series(&[1.0, 2.0]);
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = resolver_for(&loader, dates);
        // `close` alone is numeric, not a condition.
        let e = parse("close").unwrap();
        assert!(eval_condition(&e, &mut r).is_err());
    }

    #[test]
    fn boolean_and_combines() {
        let raw = series(&[10.0, 20.0, 30.0]);
        let dates: Vec<String> = raw.iter().map(|(d, _)| d.clone()).collect();
        let mut map = HashMap::new();
        map.insert("X".to_string(), raw);
        let loader = MapLoader(map);
        let mut r = resolver_for(&loader, dates);
        let e = parse("close > 5 and close < 25").unwrap();
        let b = eval_condition(&e, &mut r).unwrap();
        assert_eq!(b, vec![Some(true), Some(true), Some(false)]);
    }
}
