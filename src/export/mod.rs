mod omml;
mod docx_writer;

use std::collections::HashMap;
use crate::calc::{self, Quantity, parse_unit};
use crate::calc::document::{parse_line, Line};
use crate::plot_svg::render_plot_svg;
use docx_writer::DocxBuilder;

pub fn export_docx(cells: &[String]) -> Vec<u8> {
    let mut builder = DocxBuilder::new();
    let mut env: HashMap<String, Quantity> = HashMap::new();

    for cell_src in cells {
        // Collect plot data separately for merging
        let mut pending_plots: Vec<calc::document::PlotData> = Vec::new();

        let (compiled, out_env) = calc::compile_document_with_env(cell_src, env.clone());
        env = out_env;

        for cl in compiled {
            // Flush pending plots if non-plot line arrives
            if cl.plot_data.is_none() {
                builder = flush_plots(builder, &mut pending_plots);
            }

            if let Some(pd) = cl.plot_data {
                pending_plots.push(pd);
                continue;
            }

            if let Some(td) = cl.table_data {
                // Simple text table — emit header row then body rows as tab-separated
                let header = td.header.join(" | ");
                builder = builder.text(&header);
                for row in &td.rows {
                    builder = builder.text(&row.join(" | "));
                }
                continue;
            }

            if let Some(text) = cl.comment {
                // Detect markdown heading level from leading #
                let (level, title) = if text.starts_with("## ") {
                    (2u8, text[3..].trim())
                } else if text.starts_with("# ") {
                    (1u8, text[2..].trim())
                } else {
                    (1u8, text.trim())
                };
                builder = builder.heading(title, level);
                continue;
            }

            if let Some(err) = cl.error {
                builder = builder.text(&format!("[Error: {err}]"));
                continue;
            }

            // Math line — re-parse source to get expr + result for OMML
            if cl.typst_src.is_some() {
                if let Some(omml) = source_to_omml(&cl.source_line, &env) {
                    builder = builder.math(omml);
                }
            }
        }

        // Flush any trailing plots
        builder = flush_plots(builder, &mut pending_plots);
    }

    builder.build()
}

fn flush_plots(mut builder: DocxBuilder, plots: &mut Vec<calc::document::PlotData>) -> DocxBuilder {
    if plots.is_empty() { return builder; }
    let refs: Vec<&calc::document::PlotData> = plots.iter().collect();
    let svg = render_plot_svg(&refs);
    if !svg.is_empty() {
        // Our SVG canvas is 520×280 user units — treat as points
        builder = builder.svg_image(svg.into_bytes(), 520, 280);
    }
    plots.clear();
    builder
}

/// Re-parse a source line and build OMML from expr + evaluated result.
fn source_to_omml(src: &str, env: &HashMap<String, Quantity>) -> Option<String> {
    let trimmed = src.trim();
    if trimmed.is_empty() { return None; }

    match parse_line(trimmed).ok()? {
        Line::Assignment { lhs, expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = if unit.is_empty() { result.si_val() } else {
                if let Some((_, scale)) = parse_unit(&unit) {
                    result.si_val() / scale
                } else {
                    result.val
                }
            };
            Some(omml::assignment_to_omml(&lhs, &expr, val, &unit))
        }
        Line::Eval { expr, unit } => {
            let result = calc::eval_quantity(&expr, env).ok()?;
            let val = if unit.is_empty() { result.si_val() } else {
                if let Some((_, scale)) = parse_unit(&unit) {
                    result.si_val() / scale
                } else {
                    result.val
                }
            };
            Some(omml::eval_to_omml(&expr, val, &unit))
        }
        _ => None,
    }
}
