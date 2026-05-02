use std::collections::HashMap;
use super::ast::{BinOp, Expr};
use super::units::Quantity;

// ─────────────────────────────────────────────────────────────────────────────
// Number formatting
// ─────────────────────────────────────────────────────────────────────────────

pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e10 {
        format!("{}", v as i64)
    } else {
        format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Identifier → Typst math
// ─────────────────────────────────────────────────────────────────────────────

/// Single char: render as-is (italic). Multi-char or subscript: upright("...").
pub fn typst_ident(name: &str) -> String {
    if let Some(idx) = name.find('_') {
        let base = &name[..idx];
        let sub  = &name[idx+1..];
        let base_t = if base.len() == 1 { base.to_string() } else { format!("upright(\"{}\")", base) };
        let sub_t  = if sub.len()  == 1 { sub.to_string()  } else { format!("upright(\"{}\")", sub) };
        format!("{base_t}_{sub_t}")
    } else if name.len() > 1 {
        format!("upright(\"{}\")", name)
    } else {
        name.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit string → Typst math
// Examples:  mm^4 → upright("mm")^4
//            N·mm → upright("N") thin upright("mm")
//            MPa  → upright("MPa")
//            N/mm^2 → upright("N")/upright("mm")^2
// ─────────────────────────────────────────────────────────────────────────────

pub fn format_unit_typst(unit: &str) -> String {
    if unit.is_empty() { return String::new(); }

    // Tokenise: atom + optional exponent, separated by ·/* (mul) or / (div)
    let s = unit.replace('*', "\u{00B7}").replace('\u{22C5}', "\u{00B7}");
    let mut result = String::new();
    let mut dividing = false;
    let ch: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < ch.len() {
        match ch[i] {
            '\u{00B7}' => {
                result.push_str(" thin ");
                dividing = false;
                i += 1;
            }
            '/' => {
                result.push('/');
                dividing = true;
                i += 1;
            }
            ' ' | '\t' => { i += 1; }
            c if c.is_alphabetic() || c == '%' => {
                let mut name = String::new();
                while i < ch.len() && (ch[i].is_alphabetic() || ch[i] == '%') {
                    name.push(ch[i]); i += 1;
                }
                // collect exponent: ^N or unicode superscripts
                let (exp_str, new_i) = collect_typst_exp(&ch, i);
                i = new_i;

                let atom = format!("upright(\"{}\")", name);
                if exp_str.is_empty() {
                    result.push_str(&atom);
                } else {
                    result.push_str(&format!("{}^{}", atom, exp_str));
                }
                dividing = false;
            }
            _ => { i += 1; }
        }
    }
    result
}

/// Parse exponent starting at position i in ch.
/// Returns (typst_exp_string, new_i). Empty string = exponent 1 (omit).
fn collect_typst_exp(ch: &[char], mut i: usize) -> (String, usize) {
    if i < ch.len() && ch[i] == '^' {
        i += 1;
        let neg = i < ch.len() && ch[i] == '-';
        if neg { i += 1; }
        let mut ds = String::new();
        while i < ch.len() && ch[i].is_ascii_digit() { ds.push(ch[i]); i += 1; }
        if ds.is_empty() { return (String::new(), i); }
        let exp_s = if neg { format!("(-{})", ds) } else { ds };
        return (exp_s, i);
    }
    // Unicode superscripts
    let mut exp_s = String::new();
    let mut neg = false;
    while i < ch.len() {
        match ch[i] {
            '\u{207B}' => { neg = true; i += 1; }
            '\u{2070}' => { exp_s.push('0'); i += 1; }
            '\u{00B9}' => { exp_s.push('1'); i += 1; }
            '\u{00B2}' => { exp_s.push('2'); i += 1; }
            '\u{00B3}' => { exp_s.push('3'); i += 1; }
            '\u{2074}' => { exp_s.push('4'); i += 1; }
            '\u{2075}' => { exp_s.push('5'); i += 1; }
            '\u{2076}' => { exp_s.push('6'); i += 1; }
            '\u{2077}' => { exp_s.push('7'); i += 1; }
            '\u{2078}' => { exp_s.push('8'); i += 1; }
            '\u{2079}' => { exp_s.push('9'); i += 1; }
            _ => break,
        }
    }
    if exp_s.is_empty() {
        (String::new(), i)
    } else {
        let s = if neg { format!("(-{})", exp_s) } else { exp_s };
        (s, i)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Typst expression renderer
// ─────────────────────────────────────────────────────────────────────────────

pub fn to_typst_sym(expr: &Expr) -> String { typst_expr(expr, None) }

pub fn to_typst_sub(expr: &Expr, env: &HashMap<String, Quantity>) -> String {
    typst_expr(expr, Some(env))
}

fn typst_expr(expr: &Expr, env: Option<&HashMap<String, Quantity>>) -> String {
    let rec = |e: &Expr| typst_expr(e, env);
    match expr {
        Expr::Num(v) => fmt_num(*v),
        Expr::Var(n) => {
            if let Some(e) = env {
                e.get(n).map(|q| fmt_num(q.val)).unwrap_or_else(|| typst_ident(n))
            } else {
                typst_ident(n)
            }
        }
        Expr::UnaryMinus(e) => format!("(-{})", rec(e)),
        Expr::BinOp(l, op, r) => {
            let ls = rec(l);
            let rs = rec(r);
            match op {
                BinOp::Add => format!("{ls} + {rs}"),
                BinOp::Sub => format!("{ls} - {rs}"),
                BinOp::Mul => format!("{ls} dot.op {rs}"),
                BinOp::Div => format!("({ls})/({rs})"),
            }
        }
        Expr::Pow(b, e) => format!("{}^({})", rec(b), rec(e)),
        Expr::Call(name, args) => {
            match name.as_str() {
                "diff" if args.len() == 2 => {
                    let var = match &args[1] { Expr::Var(v) => typst_ident(v), _ => "?".into() };
                    let f = rec(&args[0]);
                    format!("(d {f})/(d {var})")
                }
                "integrate" if args.len() == 4 => {
                    let f   = rec(&args[0]);
                    let var = match &args[1] { Expr::Var(v) => typst_ident(v), _ => "?".into() };
                    let a   = rec(&args[2]);
                    let b   = rec(&args[3]);
                    format!("integral_({a})^({b}) {f} thin d {var}")
                }
                "sum" if args.len() == 4 => {
                    let f   = rec(&args[0]);
                    let var = match &args[1] { Expr::Var(v) => typst_ident(v), _ => "?".into() };
                    let a   = rec(&args[2]);
                    let b   = rec(&args[3]);
                    format!("sum_({var}={a})^({b}) {f}")
                }
                "sqrt"  if args.len() == 1 => format!("sqrt({})",   rec(&args[0])),
                "sin"   if args.len() == 1 => format!("sin({})",    rec(&args[0])),
                "cos"   if args.len() == 1 => format!("cos({})",    rec(&args[0])),
                "tan"   if args.len() == 1 => format!("tan({})",    rec(&args[0])),
                "abs"   if args.len() == 1 => format!("|{}|",       rec(&args[0])),
                "ln"    if args.len() == 1 => format!("ln({})",     rec(&args[0])),
                "log"   if args.len() == 1 => format!("log({})",    rec(&args[0])),
                "exp"   if args.len() == 1 => format!("e^({})",     rec(&args[0])),
                "asin"  if args.len() == 1 => format!("arcsin({})", rec(&args[0])),
                "acos"  if args.len() == 1 => format!("arccos({})", rec(&args[0])),
                "atan"  if args.len() == 1 => format!("arctan({})", rec(&args[0])),
                "ceil"  if args.len() == 1 => format!("ceil({})",   rec(&args[0])),
                "floor" if args.len() == 1 => format!("floor({})",  rec(&args[0])),
                other => {
                    let a: Vec<String> = args.iter().map(|a| rec(a)).collect();
                    format!("{}({})", other, a.join(", "))
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Typst math line builders
// ─────────────────────────────────────────────────────────────────────────────

pub fn build_typst_line(
    lhs: &str, expr: &Expr, env: &HashMap<String, Quantity>,
    result: &Quantity, unit: &str,
) -> String {
    let sym = to_typst_sym(expr);
    let result_val = fmt_num(result.val);
    let unit_str = if unit.is_empty() {
        String::new()
    } else {
        format!(" thin {}", format_unit_typst(unit))
    };
    let lhs_t = typst_ident(lhs);

    if matches!(expr, Expr::Num(_)) {
        return format!("{lhs_t} &= {result_val}{unit_str}");
    }
    let sub = to_typst_sub(expr, env);
    if sym == sub {
        return format!("{lhs_t} &= {sym} &= {result_val}{unit_str}");
    }
    format!("{lhs_t} &= {sym} &= {sub} &= {result_val}{unit_str}")
}

pub fn build_typst_eval(
    expr: &Expr, env: &HashMap<String, Quantity>,
    result: &Quantity, unit: &str,
) -> String {
    let sym = to_typst_sym(expr);
    let result_val = fmt_num(result.val);
    let unit_str = if unit.is_empty() {
        String::new()
    } else {
        format!(" thin {}", format_unit_typst(unit))
    };

    if matches!(expr, Expr::Num(_)) {
        return format!("{result_val}{unit_str}");
    }
    let sub = to_typst_sub(expr, env);
    if sym == sub {
        return format!("{sym} &= {result_val}{unit_str}");
    }
    format!("{sym} &= {sub} &= {result_val}{unit_str}")
}
