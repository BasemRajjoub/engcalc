use std::collections::HashMap;
use crate::calc::{self, Quantity, parse_unit};
use crate::calc::document::{parse_line, Line, PlotData};
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
                body.push_str(&format!("<h2>{}</h2>\n", esc(&text)));
                continue;
            }

            if let Some(err) = cl.error {
                body.push_str(&format!("<p class='error'>&#9888; {}</p>\n", esc(&err)));
                continue;
            }

            if cl.typst_src.is_some() {
                if let Some(tex) = source_to_latex(&cl.source_line, &env) {
                    body.push_str(&format!("<div class='eq'>\\[{}\\]</div>\n", tex));
                }
            }
        }
        flush_plots(&mut pending_plots, &mut body);
    }

    wrap_html(&body)
}

static CHART_COLORS: &[&str] = &[
    "rgb(37,99,235)",
    "rgb(220,38,38)",
    "rgb(22,163,74)",
    "rgb(217,119,6)",
    "rgb(147,51,234)",
    "rgb(6,182,212)",
];

fn flush_plots(plots: &mut Vec<PlotData>, body: &mut String) {
    if plots.is_empty() { return; }

    let chart_id = format!("chart{}", body.len());

    // Build datasets JSON
    let mut datasets = String::from("[");
    for (i, pd) in plots.iter().enumerate() {
        let color = CHART_COLORS[i % CHART_COLORS.len()];
        let pts: String = pd.points.iter()
            .map(|[x, y]| format!("{{x:{},y:{}}}", x, y))
            .collect::<Vec<_>>()
            .join(",");
        if i > 0 { datasets.push(','); }
        datasets.push_str(&format!(
            "{{label:{},borderColor:'{}',backgroundColor:'transparent',data:[{}],pointRadius:0,borderWidth:2,tension:0.3}}",
            serde_json_str(&pd.label), color, pts
        ));
    }
    datasets.push(']');

    body.push_str(&format!(
        "<figure class='plot'><canvas id='{}'></canvas>\
<script>new Chart(document.getElementById('{}'),{{type:'line',data:{{datasets:{}}},\
options:{{animation:false,scales:{{x:{{type:'linear',title:{{display:false}}}},\
y:{{title:{{display:false}}}}}},plugins:{{legend:{{display:{}}}}}}}}});</script></figure>\n",
        chart_id, chart_id, datasets,
        if plots.len() > 1 { "true" } else { "false" }
    ));

    plots.clear();
}

fn serde_json_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "\\'"))
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
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>eqgui report</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js"></script>
<script>document.addEventListener("DOMContentLoaded",function(){{
  renderMathInElement(document.body,{{delimiters:[{{left:"\\\\[",right:"\\\\]",display:true}},{{left:"\\\\(",right:"\\\\)",display:false}}]}});
}});</script>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.3/dist/chart.umd.min.js"></script>
<style>
body {{
  font-family: 'Segoe UI', system-ui, sans-serif;
  font-size: 11pt;
  line-height: 1.6;
  color: #1a1a1a;
  background: #fff;
  max-width: 820px;
  margin: 0 auto;
  padding: 32px 24px;
}}
h2 {{
  font-size: 14pt;
  font-weight: 700;
  color: #1a3a5c;
  border-bottom: 1.5px solid #2563eb;
  padding-bottom: 3px;
  margin: 24px 0 10px;
}}
.eq {{
  text-align: center;
  margin: 6px 0;
}}
figure.plot {{
  margin: 16px auto;
  max-width: 600px;
}}
figure.plot canvas {{
  width: 100%;
}}
table {{
  border-collapse: collapse;
  width: 100%;
  margin: 10px 0;
  font-size: 10pt;
}}
th, td {{
  border: 1px solid #d1d5db;
  padding: 5px 12px;
  text-align: left;
}}
th {{ background: #f3f4f6; font-weight: 600; }}
tr:nth-child(even) td {{ background: #f9fafb; }}
.error {{ color: #dc2626; font-size: 9pt; }}
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
