use std::collections::HashMap;
use super::ast::{parse_expr, err, ParseError, Expr};
use super::units::{parse_unit, infer_display_unit, Quantity};
use super::eval::eval_q;
use super::typst_codegen::{build_typst_line, build_typst_eval};

// ─────────────────────────────────────────────────────────────────────────────
// Document line types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Line {
    Comment(String),
    Blank,
    Assignment { lhs: String, expr: Expr, unit: String },
    Eval { expr: Expr, unit: String },
    /// | col1 | col2 | ...  — markdown-style table row
    TableRow(Vec<String>),
    /// plot(expr, var, a, b)
    Plot { expr: Expr, var: String, a: f64, b: f64 },
}

// ─────────────────────────────────────────────────────────────────────────────
// Compiled output
// ─────────────────────────────────────────────────────────────────────────────

pub struct CompiledLine {
    pub source_line: String,
    pub typst_src:   Option<String>,
    pub error:       Option<String>,
    pub comment:     Option<String>,
    /// (x, y) point series for plot lines
    pub plot_data:   Option<PlotData>,
    /// rows × cols string table
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

    // Table row: starts and ends with |
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

        // Check for plot(expr, var, a, b) bare call
        if let Ok(Expr::Call(name, args)) = parse_expr(rhs) {
            if name == "plot" && args.len() == 4 {
                let var = match &args[1] {
                    Expr::Var(v) => v.clone(),
                    _ => return Err(err("plot: second arg must be variable name")),
                };
                // a and b must be numeric literals or simple exprs — eval with empty env
                let empty: HashMap<String, Quantity> = HashMap::new();
                let a = eval_q(&args[2], &empty).map_err(|e| err(e))?.val;
                let b = eval_q(&args[3], &empty).map_err(|e| err(e))?.val;
                return Ok(Line::Plot { expr: args[0].clone(), var, a, b });
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

pub fn compile_document(source: &str) -> Vec<CompiledLine> {
    let mut env: HashMap<String, Quantity> = HashMap::new();
    let mut out = Vec::new();
    // Accumulate table rows until a non-table line breaks the sequence
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
            typst_src: None, error: None, comment: None, plot_data: None,
            table_data: Some(TableData { header, rows: body }),
        });
    };

    for raw_line in source.lines() {
        let src = raw_line.to_string();
        match parse_line(raw_line) {
            Ok(Line::TableRow(cells)) => {
                // Skip separator rows like |---|---|
                if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
                    continue;
                }
                pending_table.push(cells);
                continue; // don't push to out yet
            }
            other => {
                flush_table(&mut pending_table, &mut out);
                match other {
                    Ok(Line::Blank) => {
                        out.push(CompiledLine { source_line: src, typst_src: None, error: None, comment: None, plot_data: None, table_data: None });
                    }
                    Ok(Line::Comment(text)) => {
                        out.push(CompiledLine { source_line: src, typst_src: None, error: None, comment: Some(text), plot_data: None, table_data: None });
                    }
                    Ok(Line::Plot { expr, var, a, b }) => {
                        let points = sample_plot(&expr, &var, a, b, &env, 200);
                        match points {
                            Ok(pts) => {
                                let label = src.clone();
                                out.push(CompiledLine {
                                    source_line: src, typst_src: None, error: None, comment: None,
                                    plot_data: Some(PlotData { label, points: pts, x_range: [a, b] }),
                                    table_data: None,
                                });
                            }
                            Err(e) => {
                                out.push(CompiledLine { source_line: src, typst_src: None, error: Some(e), comment: None, plot_data: None, table_data: None });
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
                                let math_line = build_typst_eval(&expr, &env, &result, &unit);
                                let typst = wrap_typst(&math_line);
                                out.push(CompiledLine { source_line: src, typst_src: Some(typst), error: None, comment: None, plot_data: None, table_data: None });
                            }
                            Err(e) => {
                                out.push(CompiledLine { source_line: src, typst_src: None, error: Some(e), comment: None, plot_data: None, table_data: None });
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
                                let math_line = build_typst_line(&lhs, &expr, &env, &result, &display_unit);
                                let typst = wrap_typst(&math_line);
                                env.insert(lhs, result);
                                out.push(CompiledLine { source_line: src, typst_src: Some(typst), error: None, comment: None, plot_data: None, table_data: None });
                            }
                            Err(e) => {
                                out.push(CompiledLine { source_line: src, typst_src: None, error: Some(e), comment: None, plot_data: None, table_data: None });
                            }
                        }
                    }
                    Err(e) => {
                        out.push(CompiledLine { source_line: src, typst_src: None, error: Some(e.0), comment: None, plot_data: None, table_data: None });
                    }
                    Ok(Line::TableRow(_)) => unreachable!(),
                }
            }
        }
    }
    flush_table(&mut pending_table, &mut out);
    out
}

fn wrap_typst(math: &str) -> String {
    format!(
        "#set page(width: auto, height: auto, margin: (x: 8pt, y: 4pt))\n\
         #set text(size: 14pt)\n\
         $ {math} $"
    )
}

/// Sample expr over [a,b] with n_points, substituting var in env.
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
