use crate::calc::ast::{BinOp, Expr};

pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e10 {
        format!("{}", v as i64)
    } else {
        format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn expr_to_latex(expr: &Expr) -> String {
    node(expr, Prec::Top)
}

pub fn assignment_to_latex(lhs: &str, expr: &Expr, result: f64, unit: &str) -> String {
    let rhs = node(expr, Prec::Top);
    let res = fmt_num(result);
    let u = if unit.is_empty() { String::new() } else { format!("\\;\\mathrm{{{}}}", latex_unit(unit)) };
    format!("{} = {} = {}{}", ident(lhs), rhs, res, u)
}

pub fn eval_to_latex(expr: &Expr, result: f64, unit: &str) -> String {
    let rhs = node(expr, Prec::Top);
    let res = fmt_num(result);
    let u = if unit.is_empty() { String::new() } else { format!("\\;\\mathrm{{{}}}", latex_unit(unit)) };
    format!("{} = {}{}", rhs, res, u)
}

// ─── precedence ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Prec { Top, Add, Mul, Unary }

fn node(expr: &Expr, ctx: Prec) -> String {
    match expr {
        Expr::Num(v)           => fmt_num(*v),
        Expr::Var(name)        => ident(name),
        Expr::UnaryMinus(e)    => {
            let inner = node(e, Prec::Unary);
            let s = format!("-{inner}");
            if ctx > Prec::Add { format!("\\left({s}\\right)") } else { s }
        }
        Expr::BinOp(l, op, r) => binop(l, *op, r, ctx),
        Expr::Pow(base, exp)   => power(base, exp),
        Expr::Call(name, args) => call(name, args),
    }
}

fn binop(l: &Expr, op: BinOp, r: &Expr, ctx: Prec) -> String {
    match op {
        BinOp::Div => {
            format!("\\frac{{{}}}{{{}}}", node(l, Prec::Top), node(r, Prec::Top))
        }
        BinOp::Mul => {
            let ls = node(l, Prec::Mul);
            let rs = node(r, Prec::Mul);
            let inner = format!("{ls} \\cdot {rs}");
            if ctx > Prec::Mul { format!("\\left({inner}\\right)") } else { inner }
        }
        BinOp::Add => {
            let inner = format!("{} + {}", node(l, Prec::Add), node(r, Prec::Add));
            if ctx > Prec::Add { format!("\\left({inner}\\right)") } else { inner }
        }
        BinOp::Sub => {
            let rhs = match r {
                Expr::BinOp(_, BinOp::Add | BinOp::Sub, _) => {
                    format!("\\left({}\\right)", node(r, Prec::Top))
                }
                _ => node(r, Prec::Add),
            };
            let inner = format!("{} - {rhs}", node(l, Prec::Add));
            if ctx > Prec::Add { format!("\\left({inner}\\right)") } else { inner }
        }
    }
}

fn power(base: &Expr, exp: &Expr) -> String {
    let b = match base {
        Expr::BinOp(..) | Expr::UnaryMinus(_) => {
            format!("\\left({}\\right)", node(base, Prec::Top))
        }
        _ => node(base, Prec::Unary),
    };
    let e = node(exp, Prec::Top);
    format!("{b}^{{{e}}}")
}

fn call(name: &str, args: &[Expr]) -> String {
    match name {
        "sqrt" if args.len() == 1 =>
            format!("\\sqrt{{{}}}", node(&args[0], Prec::Top)),
        "diff" if args.len() == 2 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            format!("\\frac{{d}}{{d{}}} {}", ident(&var), node(&args[0], Prec::Top))
        }
        "integrate" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            format!("\\int_{{{}}}^{{{}}} {} \\, d{}",
                node(&args[2], Prec::Top),
                node(&args[3], Prec::Top),
                node(&args[0], Prec::Top),
                ident(&var))
        }
        "sum" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "k".into() };
            format!("\\sum_{{{}={}}}^{{{}}} {}",
                ident(&var),
                node(&args[2], Prec::Top),
                node(&args[3], Prec::Top),
                node(&args[0], Prec::Top))
        }
        "abs" if args.len() == 1 =>
            format!("\\left|{}\\right|", node(&args[0], Prec::Top)),
        "sin" | "cos" | "tan" | "ln" | "log" | "exp" if args.len() == 1 =>
            format!("\\{name}\\left({}\\right)", node(&args[0], Prec::Top)),
        _ => {
            let a = args.iter().map(|a| node(a, Prec::Top)).collect::<Vec<_>>().join(", ");
            format!("\\operatorname{{{name}}}\\left({a}\\right)")
        }
    }
}

fn ident(name: &str) -> String {
    if let Some(idx) = name.find('_') {
        let base = &name[..idx];
        let sub  = &name[idx+1..];
        format!("{}_{{{}}}", ident_base(base), ident_base(sub))
    } else {
        ident_base(name)
    }
}

fn ident_base(s: &str) -> String {
    if s.len() == 1 { s.to_string() }
    else { format!("\\mathrm{{{s}}}") }
}

fn latex_unit(unit: &str) -> String {
    let s = unit.replace('·', "\u{00B7}").replace('*', "\u{00B7}");
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    let mut current = String::new();

    let flush = |tok: &str, out: &mut String| {
        if tok.is_empty() { return; }
        if let Some(hat) = tok.find('^') {
            let base = &tok[..hat];
            let exp  = &tok[hat+1..];
            out.push_str(&format!("{{\\text{{{}}}}}^{{{}}}", base, exp));
        } else {
            out.push_str(&format!("{{\\text{{{}}}}}", tok));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '\u{00B7}' => {
                flush(&current, &mut result);
                current.clear();
                result.push_str("{\\cdot}");
            }
            '/' => {
                let num = current.clone();
                current.clear();
                let den: String = chars.collect();
                let mut n = String::new(); flush(&num, &mut n);
                let mut d = String::new(); flush(&den, &mut d);
                return format!("\\frac{{{n}}}{{{d}}}");
            }
            _ => current.push(c),
        }
    }
    flush(&current, &mut result);
    result
}
