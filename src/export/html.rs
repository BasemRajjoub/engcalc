use std::collections::HashMap;
use crate::calc::{self, Quantity, parse_unit};
use crate::calc::document::{parse_line, Line, PlotData};
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
            if let Some(pd) = cl.plot_data { pending_plots.push(pd); continue; }

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
        body.push_str("<figure class='plot'>");
        body.push_str(&svg);
        body.push_str("</figure>\n");
    }
    plots.clear();
}

fn source_to_latex(src: &str, env: &HashMap<String, Quantity>) -> Option<String> {
    let trimmed = src.trim();
    if trimmed.is_empty() { return None; }
    match parse_line(trimmed).ok()? {
        Line::Assignment { lhs, expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = unit_val(&result, &unit);
            Some(latex::assignment_to_latex(&lhs, &expr, val, &unit))
        }
        Line::Eval { expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = unit_val(&result, &unit);
            Some(latex::eval_to_latex(&expr, val, &unit))
        }
        _ => None,
    }
}

fn unit_val(q: &Quantity, unit: &str) -> f64 {
    if unit.is_empty() { q.si_val() }
    else { parse_unit(unit).map(|(_, s)| q.si_val() / s).unwrap_or(q.val) }
}

fn wrap_html(body: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>eqgui report</title>

<!-- MathJax 3 — renders LaTeX equations -->
<script>
MathJax = {{
  tex: {{
    inlineMath: [['\\(','\\)']],
    displayMath: [['\\[','\\]']],
    tags: 'ams'
  }},
  svg: {{ fontCache: 'global' }},
  startup: {{
    ready() {{
      MathJax.startup.defaultReady();
      // trigger paged.js after MathJax finishes
      MathJax.startup.promise.then(() => {{
        if (window.PagedPolyfill) window.PagedPolyfill.preview();
      }});
    }}
  }}
}};
</script>
<script async src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-svg.js"></script>

<!-- paged.js — A4 pagination, headers, page numbers -->
<script src="https://unpkg.com/pagedjs/dist/paged.polyfill.js"></script>

<style>
/* ── Page layout (paged.js) ─────────────────────────────── */
@page {{
  size: A4;
  margin: 20mm 22mm 25mm 22mm;

  @top-center {{
    content: "eqgui calculation report";
    font-size: 9pt;
    color: #6b7280;
    font-family: 'Segoe UI', sans-serif;
  }}
  @bottom-right {{
    content: "Page " counter(page) " of " counter(pages);
    font-size: 9pt;
    color: #6b7280;
    font-family: 'Segoe UI', sans-serif;
  }}
}}

/* ── Base typography ─────────────────────────────────────── */
body {{
  font-family: 'Segoe UI', system-ui, sans-serif;
  font-size: 11pt;
  line-height: 1.55;
  color: #1a1a1a;
  background: #fff;
  max-width: none;
  margin: 0;
  padding: 0;
}}

h2 {{
  font-size: 14pt;
  font-weight: 700;
  color: #1a3a5c;
  border-bottom: 1.5pt solid #2563eb;
  padding-bottom: 2pt;
  margin: 18pt 0 8pt;
  break-after: avoid;
}}

/* ── Equations ───────────────────────────────────────────── */
.eq {{
  text-align: center;
  margin: 5pt 0;
  break-inside: avoid;
}}

/* ── Plots ───────────────────────────────────────────────── */
figure.plot {{
  margin: 12pt auto;
  max-width: 480pt;
  break-inside: avoid;
  text-align: center;
}}
figure.plot svg {{
  width: 100%;
  height: auto;
  display: block;
}}

/* ── Tables ──────────────────────────────────────────────── */
table {{
  border-collapse: collapse;
  width: 100%;
  margin: 8pt 0;
  font-size: 10pt;
  break-inside: avoid;
}}
th, td {{
  border: 0.5pt solid #d1d5db;
  padding: 4pt 10pt;
  text-align: left;
}}
th {{
  background: #f3f4f6;
  font-weight: 600;
}}
tr:nth-child(even) td {{ background: #f9fafb; }}

/* ── Errors ──────────────────────────────────────────────── */
.error {{ color: #dc2626; font-size: 9pt; }}
</style>
</head>
<body>
{body}
</body>
</html>"##)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
