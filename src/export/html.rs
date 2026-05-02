use std::collections::HashMap;
use crate::calc::{self, Quantity, parse_unit};
use crate::calc::document::{PlotData};
use crate::plot_svg::render_plot_svg;
use super::latex;

pub fn export_html(cells: &[String]) -> String {
    let mut body = String::new();
    let mut env: HashMap<String, Quantity> = HashMap::new();

    for cell_src in cells {
        let mut pending_plots: Vec<PlotData> = Vec::new();
        let (compiled, out_env) = calc::compile_document_with_env(cell_src, env.clone());
        env = out_env;

        for cl in compiled {
            if cl.plot_data.is_none() {
                flush_plots(&mut pending_plots, &mut body);
            }

            if let Some(pd) = cl.plot_data {
                pending_plots.push(pd);
                continue;
            }

            if let Some(td) = cl.table_data {
                body.push_str("<table><thead><tr>");
                for h in &td.header { body.push_str(&format!("<th>{}</th>", esc(h))); }
                body.push_str("</tr></thead><tbody>");
                for row in &td.rows {
                    body.push_str("<tr>");
                    for cell in row { body.push_str(&format!("<td>{}</td>", esc(cell))); }
                    body.push_str("</tr>");
                }
                body.push_str("</tbody></table>\n");
                continue;
            }

            if let Some(text) = cl.comment {
                let clean = text.trim_start_matches('#').trim().to_string();
                body.push_str(&format!("<h2>{}</h2>\n", esc(&clean)));
                continue;
            }

            if let Some(err) = cl.error {
                body.push_str(&format!("<p class='error'>⚠ {}</p>\n", esc(&err)));
                continue;
            }

            // Math line — re-parse for LaTeX
            if cl.typst_src.is_some() {
                if let Some(tex) = source_to_latex(&cl.source_line, &env) {
                    body.push_str(&format!("<div class='eq'>\\[{tex}\\]</div>\n"));
                }
            }
        }
        flush_plots(&mut pending_plots, &mut body);
    }

    wrap_html(&body)
}

fn flush_plots(plots: &mut Vec<PlotData>, body: &mut String) {
    if plots.is_empty() { return; }
    let refs: Vec<&PlotData> = plots.iter().collect();
    let svg = render_plot_svg(&refs);
    if !svg.is_empty() {
        body.push_str("<div class='plot'>");
        body.push_str(&svg);
        body.push_str("</div>\n");
    }
    plots.clear();
}

fn source_to_latex(src: &str, env: &HashMap<String, Quantity>) -> Option<String> {
    use crate::calc::document::parse_line;
    use crate::calc::document::Line;
    let trimmed = src.trim();
    if trimmed.is_empty() { return None; }
    match parse_line(trimmed).ok()? {
        Line::Assignment { lhs, expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = if unit.is_empty() { result.si_val() }
                      else { parse_unit(&unit).map(|(_, s)| result.si_val() / s).unwrap_or(result.val) };
            Some(latex::assignment_to_latex(&lhs, &expr, val, &unit))
        }
        Line::Eval { expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = if unit.is_empty() { result.si_val() }
                      else { parse_unit(&unit).map(|(_, s)| result.si_val() / s).unwrap_or(result.val) };
            Some(latex::eval_to_latex(&expr, val, &unit))
        }
        _ => None,
    }
}

fn wrap_html(body: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>eqgui export</title>
<script>
MathJax = {{
  tex: {{ inlineMath: [['\\(','\\)']], displayMath: [['\\[','\\]']] }},
  options: {{ skipHtmlTags: ['script','noscript','style','textarea'] }}
}};
</script>
<script async src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-svg.js"></script>
<style>
* {{ box-sizing: border-box; }}
body {{
  font-family: 'Segoe UI', system-ui, sans-serif;
  max-width: 860px;
  margin: 40px auto;
  padding: 0 24px 60px;
  color: #1a1a1a;
  line-height: 1.6;
  background: #fff;
}}
h2 {{
  color: #1a3a5c;
  font-size: 1.45em;
  border-bottom: 2px solid #2563eb;
  padding-bottom: 3px;
  margin-top: 2em;
}}
.eq {{
  text-align: center;
  margin: 0.5em 0;
  overflow-x: auto;
}}
.plot {{
  margin: 1.5em auto;
  max-width: 600px;
}}
.plot svg {{ width: 100%; height: auto; display: block; }}
.error {{ color: #dc2626; font-size: 0.9em; }}
table {{
  border-collapse: collapse;
  margin: 1em 0;
  width: 100%;
  font-size: 0.95em;
}}
th, td {{ border: 1px solid #d1d5db; padding: 6px 14px; text-align: left; }}
th {{ background: #f3f4f6; font-weight: 600; }}
tr:nth-child(even) td {{ background: #f9fafb; }}
@media print {{
  body {{ max-width: 100%; margin: 0; padding: 16px; }}
  script {{ display: none; }}
}}
</style>
</head>
<body>
{body}
</body>
</html>"#)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
