pub mod units;
pub mod ast;
pub mod eval;
pub mod typst_codegen;
pub mod document;

// Re-export the public API used by main.rs
pub use document::{compile_document, compile_document_with_env, CompiledLine, PlotData, TableData};
pub use units::{Quantity, parse_unit};
pub use eval::eval_q as eval_quantity;

#[cfg(test)]
mod tests {
    use super::ast::parse_expr;
    use super::document::{compile_document, parse_line, Line};
    use super::units::{parse_unit, infer_display_unit, Dim, Quantity};
    use super::eval::eval_q;
    use std::collections::HashMap;

    fn parse(u: &str) -> (Dim, f64) {
        parse_unit(u).unwrap_or_else(|| panic!("parse_unit({u:?}) returned None"))
    }

    // ── unit parser ───────────────────────────────────────────────────────────

    #[test]
    fn unit_simple_atoms() {
        assert_eq!(parse("mm").0,  Dim::M);
        assert_eq!(parse("kN").0,  Dim::N);
        assert_eq!(parse("MPa").0, Dim::PA);
        assert!((parse("mm").1 - 1e-3).abs() < 1e-15);
        assert!((parse("MPa").1 - 1e6).abs() < 1.0);
    }

    #[test]
    fn unit_composite_mul() {
        let (d, s) = parse("kN\u{00B7}m");
        assert_eq!(d, Dim::J);
        assert!((s - 1e3).abs() < 1.0, "kN·m scale={s}");
    }

    #[test]
    fn unit_composite_div() {
        let (d, s) = parse("N/mm\u{00B2}");
        assert_eq!(d, Dim::PA);
        assert!((s - 1e6).abs() < 1.0, "N/mm² scale={s}");
    }

    #[test]
    fn unit_caret_exponent() {
        let (d, s) = parse("mm^4");
        assert_eq!(d, Dim::M4);
        assert!((s - 1e-12).abs() < 1e-20, "mm^4 scale={s}");
    }

    #[test]
    fn unit_superscript_exponent() {
        let (d, s) = parse("mm\u{2074}");
        assert_eq!(d, Dim::M4);
        assert!((s - 1e-12).abs() < 1e-20, "mm⁴ scale={s}");
    }

    #[test]
    fn unit_composite_stress() {
        let (d, s) = parse("kN/m\u{00B2}");
        assert_eq!(d, Dim::PA);
        assert!((s - 1e3).abs() < 1.0, "kN/m² scale={s}");
    }

    #[test]
    fn unit_infer_display() {
        let q = Quantity { val: 0.5, dim: Dim::M, scale: 1.0 };
        let (label, v) = infer_display_unit(&q).unwrap();
        assert!(label == "cm" || label == "mm", "unexpected label {label}");
        assert!(v > 1.0 && v < 1000.0, "display val {v} out of [1,1000)");

        let q2 = Quantity { val: 5000.0, dim: Dim::M, scale: 1.0 };
        let (label2, _) = infer_display_unit(&q2).unwrap();
        assert_eq!(label2, "km");

        let q3 = Quantity { val: 2e-5, dim: Dim::M, scale: 1.0 };
        assert!(infer_display_unit(&q3).is_some());
    }

    #[test]
    fn unit_infer_large_moment() {
        let q = Quantity { val: 22_500_000.0, dim: Dim::J, scale: 1e-3 };
        let si = q.si_val();
        let q_si = Quantity { val: si, dim: Dim::J, scale: 1.0 };
        let (label, _v) = infer_display_unit(&q_si).unwrap();
        assert!(label == "kN·m" || label == "N·m", "got {label}");
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn assert_all_ok(src: &str) {
        let lines = compile_document(src);
        for (i, line) in lines.iter().enumerate() {
            assert!(
                line.error.is_none(),
                "line {i} ({:?}) error: {:?}",
                line.source_line, line.error
            );
        }
    }

    fn apply_unit(mut r: Quantity, unit: &str) -> Quantity {
        if !unit.is_empty() {
            if let Some((dim, scale)) = parse_unit(unit) {
                r = if r.dim.is_dimensionless() {
                    Quantity { val: r.val, dim, scale }
                } else {
                    let si = r.si_val();
                    Quantity { val: si / scale, dim, scale }
                };
            }
        }
        r
    }

    fn eval_one(line: &str) -> Quantity { eval_with_env("", line) }

    fn eval_with_env(setup: &str, line: &str) -> Quantity {
        let mut env: HashMap<String, Quantity> = HashMap::new();
        for raw in setup.lines() {
            if let Ok(Line::Assignment { lhs, expr, unit }) = parse_line(raw) {
                if let Ok(r) = eval_q(&expr, &env) {
                    env.insert(lhs, apply_unit(r, &unit));
                }
            }
        }
        match parse_line(line) {
            Ok(Line::Assignment { expr, unit, .. }) => {
                apply_unit(eval_q(&expr, &env).expect("eval failed"), &unit)
            }
            Ok(Line::Eval { expr, unit }) => {
                apply_unit(eval_q(&expr, &env).expect("eval failed"), &unit)
            }
            other => panic!("not an assignment/eval: {line:?} → {other:?}"),
        }
    }

    // ── unit stamping ─────────────────────────────────────────────────────────

    #[test]
    fn unit_stamp_preserves_value() {
        let q = eval_one("f_y = 250 \"MPa\"");
        assert!((q.val - 250.0).abs() < 1e-9, "expected 250, got {}", q.val);
        assert_eq!(q.dim, Dim::PA);
    }

    #[test]
    fn unit_length_preserved() {
        let q = eval_one("b = 200 \"mm\"");
        assert!((q.val - 200.0).abs() < 1e-9, "expected 200 mm, got {} mm", q.val);
    }

    #[test]
    fn unit_conversion_result() {
        let q = eval_with_env(
            "L = 6000 \"mm\"\nw = 5 \"N/mm\"",
            "M = w * L^2 / 8 \"N·mm\""
        );
        assert!((q.val - 22_500_000.0).abs() < 1.0, "M got {}", q.val);
    }

    // ── unit-aware arithmetic ─────────────────────────────────────────────────

    #[test]
    fn unit_mul_same_units() {
        let q = eval_with_env("b = 200 \"mm\"\nh = 400 \"mm\"", "A = b * h \"mm^2\"");
        assert!((q.val - 80_000.0).abs() < 1.0, "b*h in mm² = {}", q.val);
    }

    #[test]
    fn unit_mul_mixed_units() {
        let q = eval_with_env("b = 0.2 \"m\"\nh = 400 \"mm\"", "A = b * h \"m^2\"");
        assert!((q.val - 0.08).abs() < 1e-9, "mixed mul in m² = {}", q.val);
    }

    #[test]
    fn unit_pow_mm4() {
        let q = eval_with_env("h = 400 \"mm\"", "h4 = h^4 \"mm^4\"");
        assert!((q.val - 25_600_000_000.0_f64).abs() < 1e3, "h^4 = {}", q.val);
    }

    #[test]
    fn unit_section_modulus() {
        let q = eval_with_env(
            "b = 200 \"mm\"\nh = 400 \"mm\"",
            "I = b * h^3 / 12 \"mm^4\""
        );
        let expected = 200.0 * 400.0_f64.powi(3) / 12.0;
        assert!((q.val - expected).abs() / expected < 1e-6, "I = {}, expected {expected}", q.val);
    }

    #[test]
    fn unit_stress_calculation() {
        let setup = "b = 200 \"mm\"\nh = 400 \"mm\"\nI = b * h^3 / 12 \"mm^4\"\nc = 200 \"mm\"\nW = I / c \"mm^3\"\nM = 22500000 \"N·mm\"";
        let q = eval_with_env(setup, "sigma = M / W \"MPa\"");
        let expected = 22_500_000.0 / (200.0 * 400.0_f64.powi(3) / 12.0 / 200.0);
        assert!((q.val - expected).abs() / expected < 1e-5, "sigma = {}, expected {expected}", q.val);
    }

    #[test]
    fn unit_beam_deflection() {
        let setup = "E = 200000 \"MPa\"\nb = 200 \"mm\"\nh = 400 \"mm\"\nI = b * h^3 / 12 \"mm^4\"\nL = 6000 \"mm\"\nw = 5 \"N/mm\"";
        let q = eval_with_env(setup, "delta = 5 * w * L^4 / (384 * E * I) \"mm\"");
        assert!((q.val - 0.3955).abs() < 0.01, "delta = {} mm (expected ~0.396)", q.val);
    }

    #[test]
    fn unit_dimensionless_ops() {
        let q = eval_with_env("x = 3", "y = x^2 + 2*x + 1");
        assert!((q.val - 16.0).abs() < 1e-9, "y = {}", q.val);
    }

    #[test]
    fn unit_infer_mm2_from_mul() {
        let lines = compile_document("b = 200 \"mm\"\nh = 400 \"mm\"\nA = b * h");
        let a_line = &lines[2];
        assert!(a_line.error.is_none(), "A=b*h error: {:?}", a_line.error);
        let src = a_line.typst_src.as_deref().unwrap_or("");
        assert!(src.contains("mm") || src.contains("cm") || src.contains("m"),
            "expected area unit in typst, got: {src}");
    }

    // ── variable names ────────────────────────────────────────────────────────

    #[test]
    fn multi_char_var_ff() {
        let q = eval_with_env("f = 3", "ff = f^2");
        assert!((q.val - 9.0).abs() < 1e-9, "ff=f^2 at f=3 should be 9, got {}", q.val);
    }

    #[test]
    fn underscore_var_m_ed() {
        let q = eval_with_env("shear = 5", "M_Ed = shear * 2");
        assert!((q.val - 10.0).abs() < 1e-9, "M_Ed got {}", q.val);
    }

    #[test]
    fn var_s_squares_no_split() {
        assert_all_ok("S_squares = 5\ny = S_squares + 1");
    }

    // ── bare expression ───────────────────────────────────────────────────────

    #[test]
    fn bare_expression() {
        let lines = compile_document("h = 66\nh^2");
        assert!(lines[1].error.is_none(), "h^2 bare expr error: {:?}", lines[1].error);
        assert!(lines[1].typst_src.is_some());
    }

    // ── symbolic differentiation ──────────────────────────────────────────────

    #[test]
    fn diff_polynomial() {
        let q = eval_with_env("x = 3", "dfdx = diff(x^3 + 2*x, x)");
        assert!((q.val - 29.0).abs() < 1e-9, "diff got {}", q.val);
    }

    #[test]
    fn diff_sin() {
        let q = eval_with_env("x = 0", "r = diff(sin(x), x)");
        assert!((q.val - 1.0).abs() < 1e-9, "d/dx sin(x) at 0 = {}", q.val);
    }

    #[test]
    fn diff_product_rule() {
        let pi_half = std::f64::consts::PI / 2.0;
        let q = eval_with_env(&format!("x = {pi_half}"), "r = diff(x * sin(x), x)");
        assert!((q.val - 1.0).abs() < 1e-6, "product rule got {}", q.val);
    }

    // ── numeric integration ───────────────────────────────────────────────────

    #[test]
    fn integrate_semicircle() {
        let q = eval_one("A = integrate(sqrt(1 - x^2), x, -1, 1)");
        let pi_half = std::f64::consts::PI / 2.0;
        assert!((q.val - pi_half).abs() < 1e-3, "semicircle area got {}", q.val);
    }

    #[test]
    fn integrate_polynomial() {
        let q = eval_one("I = integrate(x^2, x, 0, 1)");
        assert!((q.val - 1.0/3.0).abs() < 1e-9, "∫x² got {}", q.val);
    }

    #[test]
    fn integrate_uses_env() {
        let q = eval_with_env("a = 2", "I = integrate(x, x, 0, a)");
        assert!((q.val - 2.0).abs() < 1e-9, "∫_0^a x dx got {}", q.val);
    }

    // ── summation ─────────────────────────────────────────────────────────────

    #[test]
    fn sum_squares() {
        let q = eval_one("S = sum(k^2, k, 1, 10)");
        assert!((q.val - 385.0).abs() < 1e-9, "sum k² got {}", q.val);
    }

    #[test]
    fn sum_geometric() {
        let q = eval_one("S = sum(2^k, k, 0, 8)");
        assert!((q.val - 511.0).abs() < 1e-9, "sum 2^k got {}", q.val);
    }

    // ── trig ─────────────────────────────────────────────────────────────────

    #[test]
    fn trig_identity() {
        let q = eval_with_env("theta = 0.7854", "hyp = sqrt(sin(theta)^2 + cos(theta)^2)");
        assert!((q.val - 1.0).abs() < 1e-9, "sin²+cos²=1 got {}", q.val);
    }

    // ── full default doc ──────────────────────────────────────────────────────

    #[test]
    fn default_doc_no_errors() {
        let doc = "\
# 1 — Basic variables & units
f_y = 250 \"MPa\"
E = 200000 \"MPa\"
b = 200 \"mm\"
h = 400 \"mm\"
# 2 — Expressions & multi-char variable names
I = b * h^3 / 12 \"mm^4\"
shear = 45 \"kN\"
M_Ed = shear * 3 \"kN·m\"
# 3 — Bare expression (no assignment)
b * h
# 4 — Symbolic differentiation   diff(expr, var)
x = 3
dfdx = diff(x^3 + 2*x, x)
# 5 — Numeric integration   integrate(expr, var, a, b)
pi = 3.14159265358979
A_circle = integrate(sqrt(1 - x^2), x, -1, 1)
# 6 — Summation   sum(expr, var, a, b)
S_squares = sum(k^2, k, 1, 10)
S_geo = sum(2^k, k, 0, 8)
# 7 — Trig & standard functions
theta = 0.7854
sin_t = sin(theta)
cos_t = cos(theta)
hyp = sqrt(sin_t^2 + cos_t^2)
# 8 — Beam bending check
L = 6000 \"mm\"
w = 5 \"N/mm\"
M_max = w * L^2 / 8 \"N·mm\"
delta = 5 * w * L^4 / (384 * E * I) \"mm\"";
        assert_all_ok(doc);
    }

    // ── plot parsing ──────────────────────────────────────────────────────────

    #[test]
    fn plot_line_parses() {
        match parse_line("plot(sin(x), x, -3, 3)").unwrap() {
            Line::Plot { var, .. } => {
                assert_eq!(var, "x");
            }
            other => panic!("expected Plot, got {other:?}"),
        }
    }

    #[test]
    fn plot_samples_correctly() {
        let lines = compile_document("plot(x^2, x, 0, 1)");
        assert_eq!(lines.len(), 1);
        let pd = lines[0].plot_data.as_ref().expect("expected plot_data");
        // f(0)=0, f(1)=1
        let first = pd.points.first().unwrap();
        let last  = pd.points.last().unwrap();
        assert!(first[1].abs() < 1e-9, "f(0)={}", first[1]);
        assert!((last[1] - 1.0).abs() < 1e-6, "f(1)={}", last[1]);
    }

    // ── table parsing ─────────────────────────────────────────────────────────

    #[test]
    fn table_rows_parse() {
        let src = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let lines = compile_document(src);
        // All table rows should collapse into one CompiledLine with table_data
        let td_line = lines.iter().find(|l| l.table_data.is_some())
            .expect("no table_data");
        let td = td_line.table_data.as_ref().unwrap();
        assert_eq!(td.header, vec!["A", "B"]);
        assert_eq!(td.rows.len(), 2);
        assert_eq!(td.rows[0], vec!["1", "2"]);
    }

    #[test]
    fn table_no_separator_required() {
        let src = "| X | Y |\n| 10 | 20 |";
        let lines = compile_document(src);
        let td = lines.iter().find(|l| l.table_data.is_some())
            .and_then(|l| l.table_data.as_ref()).expect("no table");
        assert_eq!(td.header, vec!["X", "Y"]);
        assert_eq!(td.rows[0], vec!["10", "20"]);
    }
}
