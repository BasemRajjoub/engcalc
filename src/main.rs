mod calc;
mod plot_svg;

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use resvg::{tiny_skia, usvg};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontSearcher;

// ─────────────────────────────────────────────────────────────────────────────
// Typst World
// ─────────────────────────────────────────────────────────────────────────────

struct MinimalWorld {
    library: LazyHash<Library>,
    book:    LazyHash<FontBook>,
    fonts:   Vec<Font>,
    source:  Source,
}

impl MinimalWorld {
    fn new() -> Self {
        eprintln!("Loading fonts…");
        let searched = FontSearcher::new().include_system_fonts(false).search();
        let fonts: Vec<Font> = searched.fonts.iter().flat_map(|s| s.get()).collect();
        eprintln!("Fonts loaded.");
        let fid = FileId::new(None, VirtualPath::new("/main.typ"));
        Self {
            library: LazyHash::new(Library::default()),
            book:    LazyHash::new(searched.book),
            fonts,
            source:  Source::new(fid, String::new()),
        }
    }
    fn set_source(&mut self, src: &str) {
        self.source.replace(src);
        comemo::evict(30);
    }
}

impl World for MinimalWorld {
    fn library(&self) -> &LazyHash<Library> { &self.library }
    fn book(&self)    -> &LazyHash<FontBook> { &self.book }
    fn main(&self)    -> FileId              { self.source.id() }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() { Ok(self.source.clone()) }
        else { Err(FileError::NotFound(id.vpath().as_rootless_path().into())) }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }
    fn font(&self, index: usize) -> Option<Font> { self.fonts.get(index).cloned() }
    fn today(&self, _: Option<i64>) -> Option<Datetime> { Datetime::from_ymd(2025, 1, 1) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Render helpers
// ─────────────────────────────────────────────────────────────────────────────

fn compile_typst(world: &mut MinimalWorld, src: &str) -> Result<String, String> {
    world.set_source(src);
    let doc = typst::compile::<typst::layout::PagedDocument>(world)
        .output
        .map_err(|errs| errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    Ok(typst_svg::svg_merged(&doc, typst::layout::Abs::pt(0.0)))
}

fn svg_fontdb() -> std::sync::Arc<fontdb::Database> {
    use std::sync::OnceLock;
    static DB: OnceLock<std::sync::Arc<fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        std::sync::Arc::new(db)
    }).clone()
}

fn rasterize(svg: &str, ctx: &egui::Context, id: &str, scale: f32) -> Option<(egui::TextureHandle, [f32; 2])> {
    let mut opts = usvg::Options::default();
    opts.fontdb = svg_fontdb();
    let tree = usvg::Tree::from_str(svg, &opts).ok()?;
    let sz = tree.size();
    let w = (sz.width()  * scale) as u32;
    let h = (sz.height() * scale) as u32;
    if w == 0 || h == 0 { return None; }
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let pixels = pixmap.pixels().iter()
        .map(|p| egui::Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha()))
        .collect();
    let img = egui::ColorImage { size: [w as usize, h as usize], source_size: egui::Vec2::new(w as f32, h as f32), pixels };
    let tex = ctx.load_texture(id, img, egui::TextureOptions::LINEAR);
    Some((tex, [w as f32 / scale, h as f32 / scale]))
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendered row
// ─────────────────────────────────────────────────────────────────────────────

struct RenderedRow {
    texture:    Option<(egui::TextureHandle, [f32; 2])>,
    error:      Option<String>,
    comment:    Option<String>,
    table_data: Option<calc::document::TableData>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Cell — independent calculation block
// ─────────────────────────────────────────────────────────────────────────────

struct Cell {
    source: String,
    rows:   Vec<RenderedRow>,
    dirty:  bool,
}

impl Cell {
    fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), rows: Vec::new(), dirty: true }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Default document — split into cells
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_CELLS: &[&str] = &[
    // Cell 0 — section properties
    "\
# Section properties
b = 200 \"mm\"
h = 400 \"mm\"
A = b * h \"mm^2\"
I = b * h^3 / 12 \"mm^4\"
c = h / 2 \"mm\"
W = I / c \"mm^3\"",

    // Cell 1 — material & loading
    "\
# Material & loading
f_y = 250 \"MPa\"
E = 200000 \"MPa\"
L = 6000 \"mm\"
w = 5 \"N/mm\"
F = 45 \"kN\"",

    // Cell 2 — bending
    "\
# Beam bending
M_max = w * L^2 / 8 \"N·mm\"
M_Ed = F * L / 4 \"kN·m\"
sigma = M_max / W \"MPa\"
delta = 5 * w * L^4 / (384 * E * I) \"mm\"",

    // Cell 3 — calculus
    "\
# Calculus
x = 3
dfdx = diff(x^3 + 2*x, x)
A_circle = integrate(sqrt(1 - x^2), x, -1, 1)
S_squares = sum(k^2, k, 1, 10)",

    // Cell 4 — trig
    "\
# Trig
theta = 0.7854
hyp = sqrt(sin(theta)^2 + cos(theta)^2)",

    // Cell 5 — plot (uses x defined in cell 3, theta from cell 4)
    "\
# Plots
plot(sin(x), x, -6.28, 6.28) \"sin(x)\"
plot(cos(x), x, -6.28, 6.28) \"cos(x)\"
plot(x^2, x, -3, 3) \"x squared\"",

    // Cell 6 — table
    "\
# Results table
| Parameter | Value |
|-----------|-------|
| b | 200 mm |
| h | 400 mm |
| A | 80000 mm² |
| I | 2.133e9 mm⁴ |",
];

// ─────────────────────────────────────────────────────────────────────────────
// App
// ─────────────────────────────────────────────────────────────────────────────

struct App {
    world:       MinimalWorld,
    cells:       Vec<Cell>,
    /// env exported by each cell (index i = env after cell i completes)
    cell_envs:   Vec<std::collections::HashMap<String, calc::Quantity>>,
    md_cache:    CommonMarkCache,
    tex_counter: usize,
}

impl App {
    fn new() -> Self {
        let world = MinimalWorld::new();
        let n = DEFAULT_CELLS.len();
        let cells = DEFAULT_CELLS.iter().map(|s| Cell::new(*s)).collect();
        Self {
            world, cells,
            cell_envs: vec![Default::default(); n],
            md_cache: CommonMarkCache::default(),
            tex_counter: 0,
        }
    }

    /// Recompile cell `ci` and all cells after it (env may have changed).
    fn recompile_from(&mut self, ci: usize, ctx: &egui::Context) {
        // Ensure cell_envs is same length as cells
        self.cell_envs.resize_with(self.cells.len(), Default::default);

        for i in ci..self.cells.len() {
            self.cells[i].dirty = false;

            // Build input env: env exported by previous cell, or empty for cell 0
            let input_env = if i == 0 {
                Default::default()
            } else {
                self.cell_envs[i - 1].clone()
            };

            let (compiled, out_env) =
                calc::compile_document_with_env(&self.cells[i].source, input_env);
            self.cell_envs[i] = out_env;

            let world = &mut self.world;
            let tc    = &mut self.tex_counter;

            // Group consecutive plot lines into one SVG, emit other lines individually
            let mut rows: Vec<RenderedRow> = Vec::new();
            let mut plot_group: Vec<calc::document::PlotData> = Vec::new();

            let flush_plots = |group: &mut Vec<calc::document::PlotData>, rows: &mut Vec<RenderedRow>, tc: &mut usize, ctx: &egui::Context| {
                if group.is_empty() { return; }
                let refs: Vec<&calc::document::PlotData> = group.iter().collect();
                let svg = plot_svg::render_plot_svg(&refs);
                *tc += 1;
                let tex = rasterize(&svg, ctx, &format!("plot_{tc}"), 2.0);
                rows.push(RenderedRow { texture: tex, error: None, comment: None, table_data: None });
                group.clear();
            };

            for cl in compiled {
                if let Some(pd) = cl.plot_data {
                    plot_group.push(pd);
                    continue;
                }
                flush_plots(&mut plot_group, &mut rows, tc, ctx);

                if let Some(td) = cl.table_data {
                    rows.push(RenderedRow { texture: None, error: None, comment: None, table_data: Some(td) });
                } else if let Some(ref typst_src) = cl.typst_src {
                    *tc += 1;
                    match compile_typst(world, typst_src) {
                        Ok(svg) => {
                            let tex = rasterize(&svg, ctx, &format!("tex_{tc}"), 2.0);
                            rows.push(RenderedRow { texture: tex, error: None, comment: None, table_data: None });
                        }
                        Err(e) => rows.push(RenderedRow { texture: None, error: Some(e), comment: None, table_data: None }),
                    }
                } else {
                    rows.push(RenderedRow { texture: None, error: cl.error, comment: cl.comment, table_data: None });
                }
            }
            flush_plots(&mut plot_group, &mut rows, tc, ctx);
            self.cells[i].rows = rows;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Re-assert light theme every frame — prevents OS dark-mode from taking over
        ctx.set_theme(egui::Theme::Light);
        ui.visuals_mut().panel_fill      = egui::Color32::WHITE;
        ui.visuals_mut().extreme_bg_color = egui::Color32::from_gray(248);
        ui.visuals_mut().window_fill     = egui::Color32::WHITE;

        // Recompile from the first dirty cell onward (env chains)
        if let Some(first_dirty) = (0..self.cells.len()).find(|&i| self.cells[i].dirty) {
            self.recompile_from(first_dirty, &ctx);
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("⊞  Add cell").clicked() {
                    self.cells.push(Cell::new("# New cell\n"));
                    self.cell_envs.push(Default::default());
                }
                ui.label(format!("  {} cells", self.cells.len()));
            });
        });

        // ── Main area: cells stacked vertically ──────────────────────────────
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut to_delete: Option<usize> = None;
            let mut swap: Option<(usize, usize)> = None;
            let n = self.cells.len();

            for ci in 0..n {
                // Cell container
                let frame = egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(200)))
                    .inner_margin(egui::Margin::same(6))
                    .outer_margin(egui::Margin::symmetric(0, 4));

                frame.show(ui, |ui| {
                    // ── Cell header bar ──────────────────────────────────────
                    ui.horizontal(|ui| {
                        ui.label(format!("▦  Cell {}", ci + 1));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("🗑").on_hover_text("Delete cell").clicked() {
                                to_delete = Some(ci);
                            }
                            if ci + 1 < n && ui.small_button("⬇").on_hover_text("Move down").clicked() {
                                swap = Some((ci, ci + 1));
                            }
                            if ci > 0 && ui.small_button("⬆").on_hover_text("Move up").clicked() {
                                swap = Some((ci - 1, ci));
                            }
                        });
                    });
                    ui.separator();

                    // ── Split: editor left, output right ─────────────────────
                    ui.columns(2, |cols| {
                        let resp = cols[0].add(
                            egui::TextEdit::multiline(&mut self.cells[ci].source)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                        if resp.changed() { self.cells[ci].dirty = true; }

                        let panel_w = cols[1].available_width();
                        render_rows(&mut cols[1], &self.cells[ci].rows, &mut self.md_cache, panel_w, ci);
                    });
                });
            }

            if let Some(i) = to_delete {
                if self.cells.len() > 1 {
                    self.cells.remove(i);
                    self.cell_envs.remove(i);
                    // mark from deletion point onward
                    for j in i..self.cells.len() { self.cells[j].dirty = true; }
                }
            }
            if let Some((a, b)) = swap {
                self.cells.swap(a, b);
                self.cell_envs.swap(a, b);
                for j in a..self.cells.len() { self.cells[j].dirty = true; }
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Render rows for one cell's output panel
// ─────────────────────────────────────────────────────────────────────────────

fn render_rows(
    ui: &mut egui::Ui,
    rows: &[RenderedRow],
    md_cache: &mut CommonMarkCache,
    panel_w: f32,
    cell_idx: usize,
) {
    let max_w = 600.0_f32;

    for (i, row) in rows.iter().enumerate() {
        let _ = (i, cell_idx);

        // Table
        if let Some(ref td) = row.table_data {
            render_table(ui, td);
            continue;
        }

        // Error
        if let Some(ref err) = row.error {
            ui.colored_label(egui::Color32::from_rgb(200, 50, 50), format!("⚠ {err}"));
            ui.add_space(2.0);
            continue;
        }

        // Comment / heading
        if let Some(ref text) = row.comment {
            let src = text.as_str();
            let md = if src.starts_with('#') { text.clone() } else { format!("# {text}") };
            CommonMarkViewer::new().show(ui, md_cache, &md);
            ui.add_space(2.0);
            continue;
        }

        // Math texture
        if let Some((ref tex, size)) = row.texture {
            let display_w = size[0].min(panel_w).min(max_w);
            let scale     = display_w / size[0];
            let display   = egui::Vec2::new(display_w, size[1] * scale);
            ui.image((tex.id(), display));
        }
    }
}

fn render_table(ui: &mut egui::Ui, td: &calc::document::TableData) {
    egui::Grid::new(format!("tbl_{:p}", td))
        .striped(true)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            // Header
            for h in &td.header {
                ui.strong(h);
            }
            ui.end_row();
            // Body
            for row in &td.rows {
                for cell in row {
                    ui.label(cell);
                }
                ui.end_row();
            }
        });
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "eqgui — live calc",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("eqgui — live calc")
                .with_inner_size([1200.0, 800.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            // Force light theme — overrides OS dark mode preference
            cc.egui_ctx.set_theme(egui::Theme::Light);

            let white  = egui::Color32::WHITE;
            let near_w = egui::Color32::from_gray(248);
            cc.egui_ctx.style_mut_of(egui::Theme::Light, |style| {
                style.visuals.text_cursor.stroke.color         = egui::Color32::BLACK;
                style.visuals.text_cursor.stroke.width         = 2.0;
                style.visuals.panel_fill                       = white;
                style.visuals.window_fill                      = white;
                style.visuals.extreme_bg_color                 = near_w;
                style.visuals.faint_bg_color                   = near_w;
                style.visuals.code_bg_color                    = near_w;
                style.visuals.widgets.noninteractive.bg_fill   = white;
                style.visuals.widgets.inactive.bg_fill         = near_w;
                style.visuals.widgets.open.bg_fill             = white;
            });
            Ok(Box::new(App::new()))
        }),
    )
}
