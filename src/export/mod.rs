mod omml;
mod docx_writer;

use std::collections::HashMap;
use resvg::{tiny_skia, usvg};
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
    if let Some((png, w, h)) = svg_to_png(&svg, 2.0) {
        // Scale down for document: max 450px wide at 96dpi
        let max_w = 450u32;
        let dw = w.min(max_w);
        let dh = (h as f64 * dw as f64 / w as f64) as u32;
        builder = builder.image(png, dw, dh);
    }
    plots.clear();
    builder
}

fn svg_to_png(svg: &str, scale: f32) -> Option<(Vec<u8>, u32, u32)> {
    let mut opts = usvg::Options::default();
    opts.fontdb = crate::svg_fontdb();
    let tree = usvg::Tree::from_str(svg, &opts).ok()?;
    let sz = tree.size();
    let w = (sz.width()  * scale) as u32;
    let h = (sz.height() * scale) as u32;
    if w == 0 || h == 0 { return None; }
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let png = pixmap.encode_png().ok()?;
    // Return logical (unscaled) dimensions for EMU calculation
    Some((png, (sz.width() as u32), (sz.height() as u32)))
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
