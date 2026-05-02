use std::collections::HashMap;
use base64::Engine;
use crate::calc::{self, Quantity, parse_unit};
use crate::calc::document::{parse_line, Line, PlotData};
use crate::plot_svg::render_plot_svg;

pub fn export_html(cells: &[String], world: &mut crate::MinimalWorld) -> String {
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
                for h in &td.header {
                    body.push_str(&format!("<th>{}</th>", esc(h)));
                }
                body.push_str("</tr></thead><tbody>");
                for row in &td.rows {
                    body.push_str("<tr>");
                    for cell in row {
                        body.push_str(&format!("<td>{}</td>", esc(cell)));
                    }
                    body.push_str("</tr>");
                }
                body.push_str("</tbody></table>\n");
                continue;
            }

            if let Some(text) = cl.comment {
                let (tag, content) = if text.starts_with("## ") {
                    ("h2", text[3..].trim().to_string())
                } else if text.starts_with("# ") {
                    ("h2", text[2..].trim().to_string())
                } else {
                    ("h2", text.trim().to_string())
                };
                body.push_str(&format!("<{tag}>{}</{tag}>\n", esc(&content)));
                continue;
            }

            if let Some(err) = cl.error {
                body.push_str(&format!("<p class='error'>⚠ {}</p>\n", esc(&err)));
                continue;
            }

            // Math line — render Typst SVG → inline base64 img
            if let Some(ref typst_src) = cl.typst_src {
                match crate::compile_typst(world, typst_src) {
                    Ok(svg) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
                        body.push_str(&format!(
                            "<div class='eq'><img src='data:image/svg+xml;base64,{b64}' alt='equation'/></div>\n"
                        ));
                    }
                    Err(e) => {
                        body.push_str(&format!("<p class='error'>⚠ {}</p>\n", esc(&e)));
                    }
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
        // Inline SVG directly — no encoding needed, scales perfectly
        body.push_str("<div class='plot'>");
        body.push_str(&svg);
        body.push_str("</div>\n");
    }
    plots.clear();
}

fn wrap_html(body: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>eqgui export</title>
<style>
  body {{
    font-family: 'Segoe UI', system-ui, sans-serif;
    max-width: 860px;
    margin: 40px auto;
    padding: 0 24px;
    color: #1a1a1a;
    line-height: 1.6;
    background: #fff;
  }}
  h1, h2, h3 {{ color: #1a3a5c; margin-top: 2em; }}
  h2 {{ font-size: 1.5em; border-bottom: 2px solid #2563eb; padding-bottom: 4px; }}
  .eq {{
    text-align: center;
    margin: 0.6em 0;
  }}
  .eq img {{
    max-width: 100%;
    vertical-align: middle;
  }}
  .plot {{
    margin: 1.5em auto;
    max-width: 600px;
  }}
  .plot svg {{
    width: 100%;
    height: auto;
  }}
  .error {{
    color: #dc2626;
    font-size: 0.9em;
  }}
  table {{
    border-collapse: collapse;
    margin: 1em 0;
    width: 100%;
  }}
  th, td {{
    border: 1px solid #d1d5db;
    padding: 6px 12px;
    text-align: left;
  }}
  th {{
    background: #f3f4f6;
    font-weight: 600;
  }}
  tr:nth-child(even) td {{ background: #f9fafb; }}
  @media print {{
    body {{ max-width: 100%; margin: 0; }}
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
