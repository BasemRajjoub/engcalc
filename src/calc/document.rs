use std::collections::HashMap;
use super::ast::{parse_expr, err, ParseError, Expr};
use super::units::{parse_unit, infer_display_unit, Quantity};
use super::eval::eval_q;

// ─────────────────────────────────────────────────────────────────────────────
// Document line types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Line {
    Comment(String),
    Blank,
    Assignment { lhs: String, expr: Expr, unit: String },
    Eval { expr: Expr, unit: String },
    TableRow(Vec<String>),
    Plot { expr: Expr, var: String, a_expr: Expr, b_expr: Expr, label: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Compiled output
// ─────────────────────────────────────────────────────────────────────────────

pub struct CompiledLine {
    pub source_line: String,
    pub latex:       Option<String>,
    pub error:       Option<String>,
    pub comment:     Option<String>,
    pub plot_data:   Option<PlotData>,
    pub table_data:  Option<TableData>,
}

pub struct PlotData {
    pub label: String,
    pub points: Vec<[f64; 2]>,
    pub x_range: [f64; 2],
}

pub struct TableData {
    pub header: Vec<String>,
    pub rows:   Vec<Vec<String>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Parse one source line
// ─────────────────────────────────────────────────────────────────────────────

pub fn parse_line(src: &str) -> Result<Line, ParseError> {
    let trimmed = src.trim();
    if trimmed.is_empty() { return Ok(Line::Blank); }
    if trimmed.starts_with('#') { return Ok(Line::Comment(trimmed[1..].trim().to_string())); }

    if trimmed.starts_with('|') {
        let cells: Vec<String> = trimmed
            .split('|')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        return Ok(Line::TableRow(cells));
    }

    let no_comment = strip_comment(trimmed).trim();
    let eq_pos = find_assignment_eq(no_comment);

    if eq_pos.is_none() {
        let (rhs, unit) = extract_unit(no_comment);
        let rhs = rhs.trim();

        let (plot_rhs, plot_label) = extract_unit(rhs);
        let plot_rhs = plot_rhs.trim();
        if let Ok(Expr::Call(name, args)) = parse_expr(plot_rhs) {
            if name == "plot" && args.len() == 4 {
                let var = match &args[1] {
                    Expr::Var(v) => v.clone(),
                    _ => return Err(err("plot: second arg must be variable name")),
                };
                let label = if plot_label.is_empty() {
                    plot_rhs.to_string()
                } else {
                    plot_label.clone()
                };
                return Ok(Line::Plot {
                    expr: args[0].clone(), var,
                    a_expr: args[2].clone(), b_expr: args[3].clone(),
                    label,
                });
            }
        }

        let expr = parse_expr(rhs)?;
        return Ok(Line::Eval { expr, unit });
    }
    let eq_pos = eq_pos.unwrap();

    let lhs = no_comment[..eq_pos].trim().to_string();
    if lhs.is_empty() { return Err(err("missing variable name")); }
    {
        let mut chars = lhs.chars();
        let first_ok = chars.next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false);
        let rest_ok  = chars.all(|c| c.is_alphanumeric() || c == '_');
        if !first_ok || !rest_ok {
            return Err(err(format!("invalid variable name: '{lhs}'")));
        }
    }

    let rhs_full = no_comment[eq_pos + 1..].trim();
    let (rhs, unit) = extract_unit(rhs_full);
    let expr = parse_expr(rhs.trim())?;
    Ok(Line::Assignment { lhs, expr, unit })
}

fn strip_comment(s: &str) -> &str {
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            '#' if !in_quote => return &s[..i],
            _ => {}
        }
    }
    s
}

fn find_assignment_eq(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            if bytes.get(i + 1) == Some(&b'=') { continue; }
            if i > 0 && (bytes[i-1] == b'<' || bytes[i-1] == b'>') { continue; }
            return Some(i);
        }
    }
    None
}

fn extract_unit(rhs: &str) -> (&str, String) {
    let rhs = rhs.trim();
    if rhs.ends_with('"') {
        let inner = &rhs[..rhs.len()-1];
        if let Some(start) = inner.rfind('"') {
            let unit = inner[start+1..].to_string();
            let expr = inner[..start].trim();
            return (expr, unit);
        }
    }
    (rhs, String::new())
}

// ─────────────────────────────────────────────────────────────────────────────
// Compile full document
// ─────────────────────────────────────────────────────────────────────────────

pub fn compile_document_with_env(
    source: &str,
    initial_env: HashMap<String, Quantity>,
) -> (Vec<CompiledLine>, HashMap<String, Quantity>) {
    let mut env = initial_env;
    let mut out = Vec::new();
    compile_into(&mut env, &mut out, source);
    (out, env)
}

pub fn compile_document(source: &str) -> Vec<CompiledLine> {
    let mut env: HashMap<String, Quantity> = HashMap::new();
    let mut out = Vec::new();
    compile_into(&mut env, &mut out, source);
    out
}

fn blank_line(src: String) -> CompiledLine {
    CompiledLine { source_line: src, latex: None, error: None, comment: None, plot_data: None, table_data: None }
}

fn compile_into(env: &mut HashMap<String, Quantity>, out: &mut Vec<CompiledLine>, source: &str) {
    let mut pending_table: Vec<Vec<String>> = Vec::new();

    let flush_table = |pending: &mut Vec<Vec<String>>, out: &mut Vec<CompiledLine>| {
        if pending.is_empty() { return; }
        let rows = std::mem::take(pending);
        let (header, body) = if rows.len() > 1 {
            (rows[0].clone(), rows[1..].to_vec())
        } else {
            (rows[0].clone(), vec![])
        };
        out.push(CompiledLine {
            source_line: String::new(),
            latex: None, error: None, comment: None, plot_data: None,
            table_data: Some(TableData { header, rows: body }),
        });
    };

    for raw_line in source.lines() {
        let src = raw_line.to_string();
        match parse_line(raw_line) {
            Ok(Line::TableRow(cells)) => {
                if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
                    continue;
                }
                pending_table.push(cells);
                continue;
            }
            other => {
                flush_table(&mut pending_table, &mut *out);
                match other {
                    Ok(Line::Blank) => {
                        out.push(blank_line(src));
                    }
                    Ok(Line::Comment(text)) => {
                        out.push(CompiledLine { source_line: src, latex: None, error: None, comment: Some(text), plot_data: None, table_data: None });
                    }
                    Ok(Line::Plot { expr, var, a_expr, b_expr, label }) => {
                        let a_res = eval_q(&a_expr, env).map(|q| q.si_val());
                        let b_res = eval_q(&b_expr, env).map(|q| q.si_val());
                        match (a_res, b_res) {
                            (Ok(a), Ok(b)) => {
                                match sample_plot(&expr, &var, a, b, env, 200) {
                                    Ok(pts) => {
                                        out.push(CompiledLine {
                                            source_line: src, latex: None, error: None, comment: None,
                                            plot_data: Some(PlotData { label, points: pts, x_range: [a, b] }),
                                            table_data: None,
                                        });
                                    }
                                    Err(e) => {
                                        out.push(CompiledLine { source_line: src, latex: None, error: Some(e), comment: None, plot_data: None, table_data: None });
                                    }
                                }
                            }
                            _ => {
                                out.push(CompiledLine { source_line: src, latex: None, error: Some("plot: could not evaluate range".into()), comment: None, plot_data: None, table_data: None });
                            }
                        }
                    }
                    Ok(Line::Eval { expr, unit }) => {
                        match eval_q(&expr, &env) {
                            Ok(mut result) => {
                                if !unit.is_empty() {
                                    if let Some((dim, scale)) = parse_unit(&unit) {
                                        result = if result.dim.is_dimensionless() {
                                            Quantity { val: result.val, dim, scale }
                                        } else {
                                            let si = result.si_val();
                                            Quantity { val: si / scale, dim, scale }
                                        };
                                    }
                                }
                                let val = result_display_val(&result, &unit);
                                let tex = crate::export::latex::eval_to_latex(&expr, val, &unit);
                                out.push(CompiledLine { source_line: src, latex: Some(tex), error: None, comment: None, plot_data: None, table_data: None });
                            }
                            Err(e) => {
                                out.push(CompiledLine { source_line: src, latex: None, error: Some(e), comment: None, plot_data: None, table_data: None });
                            }
                        }
                    }
                    Ok(Line::Assignment { lhs, expr, unit }) => {
                        match eval_q(&expr, &env) {
                            Ok(mut result) => {
                                let display_unit: String;
                                if !unit.is_empty() {
                                    if let Some((dim, scale)) = parse_unit(&unit) {
                                        result = if result.dim.is_dimensionless() {
                                            Quantity { val: result.val, dim, scale }
                                        } else {
                                            let si = result.si_val();
                                            Quantity { val: si / scale, dim, scale }
                                        };
                                    }
                                    display_unit = unit.clone();
                                } else if let Some((label, val)) = infer_display_unit(&result) {
                                    result.val = val;
                                    display_unit = label.to_string();
                                } else {
                                    display_unit = String::new();
                                }
                                let val = result.val;
                                let tex = crate::export::latex::assignment_to_latex(&lhs, &expr, val, &display_unit);
                                env.insert(lhs, result);
                                out.push(CompiledLine { source_line: src, latex: Some(tex), error: None, comment: None, plot_data: None, table_data: None });
                            }
                            Err(e) => {
                                out.push(CompiledLine { source_line: src, latex: None, error: Some(e), comment: None, plot_data: None, table_data: None });
                            }
                        }
                    }
                    Err(e) => {
                        out.push(CompiledLine { source_line: src, latex: None, error: Some(e.0), comment: None, plot_data: None, table_data: None });
                    }
                    Ok(Line::TableRow(_)) => unreachable!(),
                }
            }
        }
    }
    flush_table(&mut pending_table, out);
}

fn result_display_val(result: &Quantity, unit: &str) -> f64 {
    if unit.is_empty() { result.si_val() } else { result.val }
}

fn sample_plot(
    expr: &Expr, var: &str, a: f64, b: f64,
    env: &HashMap<String, Quantity>, n: usize,
) -> Result<Vec<[f64; 2]>, String> {
    let mut pts = Vec::with_capacity(n);
    for i in 0..=n {
        let x = a + (b - a) * (i as f64) / (n as f64);
        let mut local = env.clone();
        local.insert(var.to_string(), Quantity::dimensionless(x));
        let y = eval_q(expr, &local)?.si_val();
        if y.is_finite() {
            pts.push([x, y]);
        }
    }
    Ok(pts)
}
