/// Convert our Expr AST to OMML (Office Math Markup Language) XML string.
/// The caller wraps it in <m:oMath xmlns:m="...">...</m:oMath>.

use crate::calc::ast::{BinOp, Expr};
use crate::calc::typst_codegen::fmt_num;

const NS: &str = "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"";

pub fn expr_to_omml(expr: &Expr) -> String {
    format!("<m:oMath {NS}>{}</m:oMath>", node(expr, false))
}

/// Full equation line: lhs = rhs = numeric_result [unit]
pub fn assignment_to_omml(lhs: &str, expr: &Expr, result: f64, unit: &str) -> String {
    let lhs_omml = ident_run(lhs);
    let eq = run("=");
    let rhs = node(expr, false);
    let eq2 = run("=");
    let num = run(&fmt_num(result));
    let unit_part = if unit.is_empty() {
        String::new()
    } else {
        format!("{}{}", run("\u{2009}"), unit_run(unit))
    };
    format!("<m:oMath {NS}>{lhs_omml}{eq}{rhs}{eq2}{num}{unit_part}</m:oMath>")
}

pub fn eval_to_omml(expr: &Expr, result: f64, unit: &str) -> String {
    let rhs = node(expr, false);
    let eq = run("=");
    let num = run(&fmt_num(result));
    let unit_part = if unit.is_empty() {
        String::new()
    } else {
        format!("{}{}", run("\u{2009}"), unit_run(unit))
    };
    format!("<m:oMath {NS}>{rhs}{eq}{num}{unit_part}</m:oMath>")
}

// ─── internal helpers ────────────────────────────────────────────────────────

fn node(expr: &Expr, paren: bool) -> String {
    let inner = match expr {
        Expr::Num(v) => run(&fmt_num(*v)),
        Expr::Var(name) => ident_run(name),
        Expr::UnaryMinus(e) => format!("{}{}", run("\u{2212}"), node(e, needs_paren(e))),
        Expr::BinOp(l, op, r) => binop(l, *op, r),
        Expr::Pow(base, exp) => power(base, exp),
        Expr::Call(name, args) => call(name, args),
    };
    if paren {
        format!("{}{inner}{}", delim("("), delim(")"))
    } else {
        inner
    }
}

fn needs_paren(e: &Expr) -> bool {
    matches!(e, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _) | Expr::UnaryMinus(_))
}

fn binop(l: &Expr, op: BinOp, r: &Expr) -> String {
    match op {
        BinOp::Div => {
            format!(
                "<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>",
                node(l, false),
                node(r, false)
            )
        }
        BinOp::Mul => {
            let lp = needs_paren_mul(l);
            let rp = needs_paren_mul(r);
            format!(
                "{}{}{}",
                node(l, lp),
                run("\u{22C5}"),  // ⋅ dot
                node(r, rp)
            )
        }
        BinOp::Add => format!("{}{}{}", node(l, false), run("+"), node(r, false)),
        BinOp::Sub => {
            let rp = matches!(r, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _));
            format!("{}{}{}", node(l, false), run("\u{2212}"), node(r, rp))
        }
    }
}

fn needs_paren_mul(e: &Expr) -> bool {
    matches!(e, Expr::BinOp(_, BinOp::Add | BinOp::Sub, _))
}

fn power(base: &Expr, exp: &Expr) -> String {
    let bp = matches!(base, Expr::BinOp(..) | Expr::UnaryMinus(_));
    format!(
        "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
        node(base, bp),
        node(exp, false)
    )
}

fn call(name: &str, args: &[Expr]) -> String {
    match name {
        "sqrt" if args.len() == 1 => {
            format!("<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>",
                node(&args[0], false))
        }
        "diff" if args.len() == 2 => {
            // d/dx (expr)
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            let num = format!("<m:r><m:t>d</m:t></m:r>{}", node(&args[0], false));
            let den = format!("<m:r><m:t>d</m:t></m:r>{}", ident_run(&var));
            format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>")
        }
        "integrate" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "x".into() };
            format!(
                "<m:nary><m:naryPr><m:chr m:val=\"∫\"/></m:naryPr>\
                 <m:sub>{}</m:sub><m:sup>{}</m:sup>\
                 <m:e>{}{}</m:e></m:nary>",
                node(&args[2], false),
                node(&args[3], false),
                node(&args[0], false),
                format!("<m:r><m:t>d{var}</m:t></m:r>")
            )
        }
        "sum" if args.len() == 4 => {
            let var = match &args[1] { Expr::Var(v) => v.clone(), _ => "k".into() };
            let lower = format!("{}={}", ident_run(&var), node(&args[2], false));
            format!(
                "<m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr>\
                 <m:sub>{lower}</m:sub><m:sup>{}</m:sup>\
                 <m:e>{}</m:e></m:nary>",
                node(&args[3], false),
                node(&args[0], false),
            )
        }
        // trig / etc — render as function name + arg in parens
        _ => {
            let fn_run = format!("<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>{name}</m:t></m:r>");
            let args_omml: String = args.iter().enumerate()
                .map(|(i, a)| {
                    let s = node(a, false);
                    if i + 1 < args.len() { format!("{s}{}", run(",")) } else { s }
                })
                .collect();
            format!("{fn_run}{}{args_omml}{}", delim("("), delim(")"))
        }
    }
}

/// Identifier with subscript support: f_y → f subscript y
fn ident_run(name: &str) -> String {
    if let Some(idx) = name.find('_') {
        let base = &name[..idx];
        let sub  = &name[idx+1..];
        format!(
            "<m:sSub><m:e><m:r><m:t>{}</m:t></m:r></m:e><m:sub><m:r><m:t>{}</m:t></m:r></m:sub></m:sSub>",
            xml_esc(base), xml_esc(sub)
        )
    } else {
        format!("<m:r><m:t>{}</m:t></m:r>", xml_esc(name))
    }
}

/// Unit string as upright (roman) text
fn unit_run(unit: &str) -> String {
    format!("<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>{}</m:t></m:r>", xml_esc(unit))
}

fn run(text: &str) -> String {
    format!("<m:r><m:t>{}</m:t></m:r>", xml_esc(text))
}

fn delim(s: &str) -> String {
    // Use m:r for simple open/close chars (lighter than m:d)
    run(s)
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
