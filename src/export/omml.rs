use crate::calc::ast::{BinOp, Expr};
use crate::calc::typst_codegen::fmt_num;

const NS: &str = "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"";

pub fn assignment_to_omml(lhs: &str, expr: &Expr, result: f64, unit: &str) -> String {
    let parts = format!(
        "{}{}{}{}{}",
        ident_run(lhs),
        op_run("="),
        node(expr, false),
        op_run("="),
        result_run(result, unit),
    );
    format!("<m:oMath {NS}>{parts}</m:oMath>")
}

pub fn eval_to_omml(expr: &Expr, result: f64, unit: &str) -> String {
    let parts = format!(
        "{}{}{}",
        node(expr, false),
        op_run("="),
        result_run(result, unit),
    );
    format!("<m:oMath {NS}>{parts}</m:oMath>")
}

// ─── node → OMML ────────────────────────────────────────────────────────────

fn node(expr: &Expr, paren: bool) -> String {
    let inner = match expr {
        Expr::Num(v)              => num_run(*v),
        Expr::Var(name)           => ident_run(name),
        Expr::UnaryMinus(e)       => format!("{}{}", op_run("\u{2212}"), node(e, needs_paren(e))),
        Expr::BinOp(l, op, r)    => binop(l, *op, r),
        Expr::Pow(base, exp)      => power(base, exp),
        Expr::Call(name, args)    => call(name, args),
    };
    if paren {
        // Use m:d for proper stretchy parens
        format!("<m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/></m:dPr><m:e>{inner}</m:e></m:d>")
    } else {
        inner
    }
}

fn needs_paren(e: &Expr) -> bool {
    matches!(e, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _) | Expr::UnaryMinus(_))
}

fn binop(l: &Expr, op: BinOp, r: &Expr) -> String {
    match op {
        BinOp::Div => format!(
            "<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>",
            node(l, false), node(r, false)
        ),
        BinOp::Mul => {
            let lp = matches!(l, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _));
            let rp = matches!(r, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _));
            format!("{}{}{}", node(l, lp), op_run("\u{22C5}"), node(r, rp))
        }
        BinOp::Add => format!("{}{}{}", node(l, false), op_run("+"), node(r, false)),
        BinOp::Sub => {
            let rp = matches!(r, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _));
            format!("{}{}{}", node(l, false), op_run("\u{2212}"), node(r, rp))
        }
    }
}

fn power(base: &Expr, exp: &Expr) -> String {
    let bp = matches!(base, Expr::BinOp(..) | Expr::UnaryMinus(_));
    format!(
        "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
        node(base, bp), node(exp, false)
    )
}

fn call(name: &str, args: &[Expr]) -> String {
    match name {
        "sqrt" if args.len() == 1 => format!(
            "<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>",
            node(&args[0], false)
        ),
        "diff" if args.len() == 2 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            let num = format!("{}{}", upright_run("d"), node(&args[0], false));
            let den = format!("{}{}", upright_run("d"), ident_run(&var));
            format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>")
        }
        "integrate" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            format!(
                "<m:nary><m:naryPr><m:chr m:val=\"\u{222B}\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr>\
                 <m:sub>{}</m:sub><m:sup>{}</m:sup>\
                 <m:e>{}{}{}</m:e></m:nary>",
                node(&args[2], false),
                node(&args[3], false),
                node(&args[0], false),
                upright_run("d"),
                ident_run(&var),
            )
        }
        "sum" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "k".into() };
            format!(
                "<m:nary><m:naryPr><m:chr m:val=\"\u{2211}\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr>\
                 <m:sub>{}{}{}  </m:sub><m:sup>{}</m:sup>\
                 <m:e>{}</m:e></m:nary>",
                ident_run(&var),
                op_run("="),
                node(&args[2], false),
                node(&args[3], false),
                node(&args[0], false),
            )
        }
        // sin, cos, tan, ln, log, exp, abs → upright function name + paren arg
        _ => {
            let arg_omml: String = args.iter().enumerate()
                .map(|(i, a)| {
                    let s = node(a, false);
                    if i + 1 < args.len() { format!("{s}{}", op_run(",\u{2009}")) } else { s }
                })
                .collect();
            format!(
                "{}<m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/></m:dPr><m:e>{arg_omml}</m:e></m:d>",
                upright_run(name)
            )
        }
    }
}

// ─── run helpers ─────────────────────────────────────────────────────────────

/// Italic math variable — single or multi-char with subscript
fn ident_run(name: &str) -> String {
    if let Some(idx) = name.find('_') {
        let base = &name[..idx];
        let sub  = &name[idx+1..];
        format!(
            "<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>",
            italic_run(base), italic_run(sub)
        )
    } else {
        italic_run(name)
    }
}

/// Italic math run — for variables and numbers in expressions
fn italic_run(text: &str) -> String {
    format!(
        "<m:r><m:rPr><m:sty m:val=\"i\"/></m:rPr><m:t>{}</m:t></m:r>",
        xml_esc(text)
    )
}

/// Upright (roman) run — for units, function names, d in d/dx
fn upright_run(text: &str) -> String {
    format!(
        "<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>{}</m:t></m:r>",
        xml_esc(text)
    )
}

/// Operator run — +, −, =, ·  (no special math style needed)
fn op_run(text: &str) -> String {
    format!("<m:r><m:t xml:space=\"preserve\"> {} </m:t></m:r>", xml_esc(text))
}

/// Number run — upright digits
fn num_run(v: f64) -> String {
    format!(
        "<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>{}</m:t></m:r>",
        fmt_num(v)
    )
}

/// Result + unit — number upright, unit in thin-spaced upright text
fn result_run(val: f64, unit: &str) -> String {
    let num = num_run(val);
    if unit.is_empty() {
        num
    } else {
        // Parse unit for superscripts: mm^2 → mm²
        let unit_omml = unit_to_omml(unit);
        format!("{}<m:r><m:t xml:space=\"preserve\"> </m:t></m:r>{unit_omml}", num)
    }
}

/// Convert unit string to OMML — handles ^N exponents
fn unit_to_omml(unit: &str) -> String {
    // Split on · and / keeping delimiters
    let mut result = String::new();
    let mut chars = unit.chars().peekable();
    let mut current = String::new();

    let flush = |s: &str, r: &mut String| {
        if s.is_empty() { return; }
        // Check for ^exponent
        if let Some(hat) = s.find('^') {
            let base = &s[..hat];
            let exp  = &s[hat+1..];
            r.push_str(&format!(
                "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
                upright_run(base), upright_run(exp)
            ));
        } else {
            r.push_str(&upright_run(s));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '\u{00B7}' | '\u{22C5}' | '*' => {
                flush(&current, &mut result);
                current.clear();
                result.push_str(&upright_run("\u{22C5}"));
            }
            '/' => {
                let num_part = current.clone();
                current.clear();
                let mut den_part = String::new();
                while let Some(&nc) = chars.peek() {
                    den_part.push(nc);
                    chars.next();
                }
                let mut num_omml = String::new();
                flush(&num_part, &mut num_omml);
                let mut den_omml = String::new();
                flush(&den_part, &mut den_omml);
                result.push_str(&format!("<m:f><m:num>{num_omml}</m:num><m:den>{den_omml}</m:den></m:f>"));
                return result;
            }
            _ => current.push(c),
        }
    }
    flush(&current, &mut result);
    result
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
