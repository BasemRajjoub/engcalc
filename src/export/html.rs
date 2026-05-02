use crate::calc;
use crate::calc::document::PlotData;

/// Export HTML with all equations pre-rendered to inline SVG.
/// `latex_to_svg`: closure that converts a LaTeX string to an SVG string.
pub fn export_html(source: &str, latex_to_svg: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let mut body = String::new();
    let mut pending_plots: Vec<PlotData> = Vec::new();

    let compiled = calc::compile_document(source);

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

        if let Some(latex) = cl.latex {
            match latex_to_svg(&latex) {
                Some(svg) => {
                    // Strip the outer SVG vertical-align style — let CSS handle centering
                    let svg = svg.replace(r#"style="vertical-align:"#, r#"style="display:block;margin:auto;vertical-align:"#);
                    body.push_str(&format!("<div class='eq'>{svg}</div>\n"));
                }
                None => {
                    body.push_str(&format!("<p class='error'>&#9888; render failed: {}</p>\n", esc(&latex)));
                }
            }
        }
    }
    flush_plots(&mut pending_plots, &mut body);

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

fn wrap_html(body: &str) -> String {
    // Chart.js from CDN — only needed if plots exist; tracking prevention only affects cookies/storage,
    // not script loading. But to be safe, embed Chart.js inline if needed.
    // For now use CDN; equations are inline SVG so no CDN needed for math.
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>eqgui report</title>
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
.eq svg {{
  max-width: 100%;
  height: auto;
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
