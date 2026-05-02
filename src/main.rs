mod calc;

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_plot::{Line, Plot, PlotPoints};
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

fn rasterize(svg: &str, ctx: &egui::Context, id: &str, scale: f32) -> Option<(egui::TextureHandle, [f32; 2])> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
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
    plot_data:  Option<calc::document::PlotData>,
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

    // Cell 5 — plot
    "\
# Plot: sin and cos
plot(sin(x), x, -6.28, 6.28)
plot(cos(x), x, -6.28, 6.28)",

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
    world:        MinimalWorld,
    cells:        Vec<Cell>,
    md_cache:     CommonMarkCache,
    drag_src:     Option<usize>,  // index being dragged
    tex_counter:  usize,          // unique texture IDs
}

impl App {
    fn new() -> Self {
        let world = MinimalWorld::new();
        let cells = DEFAULT_CELLS.iter().map(|s| Cell::new(*s)).collect();
        Self { world, cells, md_cache: CommonMarkCache::default(), drag_src: None, tex_counter: 0 }
    }

    fn recompile_cell(&mut self, ci: usize, ctx: &egui::Context) {
        self.cells[ci].dirty = false;
        let compiled = calc::compile_document(&self.cells[ci].source);
        let tc = &mut self.tex_counter;
        let world = &mut self.world;
        self.cells[ci].rows = compiled.into_iter().map(|cl| {
            // Plot data — pass through
            if let Some(pd) = cl.plot_data {
                return RenderedRow { texture: None, error: None, comment: None,
                    plot_data: Some(pd), table_data: None };
            }
            // Table data — pass through
            if let Some(td) = cl.table_data {
                return RenderedRow { texture: None, error: None, comment: None,
                    plot_data: None, table_data: Some(td) };
            }
            if let Some(ref typst_src) = cl.typst_src {
                *tc += 1;
                match compile_typst(world, typst_src) {
                    Ok(svg) => {
                        let tex = rasterize(&svg, ctx, &format!("tex_{tc}"), 2.0);
                        RenderedRow { texture: tex, error: None, comment: None, plot_data: None, table_data: None }
                    }
                    Err(e) => RenderedRow { texture: None, error: Some(e), comment: None, plot_data: None, table_data: None },
                }
            } else {
                RenderedRow { texture: None, error: cl.error, comment: cl.comment, plot_data: None, table_data: None }
            }
        }).collect();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Recompile dirty cells
        for i in 0..self.cells.len() {
            if self.cells[i].dirty {
                self.recompile_cell(i, &ctx);
            }
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("＋ Add cell").clicked() {
                    self.cells.push(Cell::new("# New cell\n"));
                }
                ui.label(format!("{} cells", self.cells.len()));
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
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(200)))
                    .inner_margin(egui::Margin::same(6))
                    .outer_margin(egui::Margin::symmetric(0, 4));

                frame.show(ui, |ui| {
                    // ── Cell header bar ──────────────────────────────────────
                    ui.horizontal(|ui| {
                        ui.label(format!("▦ Cell {}", ci + 1));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Delete cell").clicked() {
                                to_delete = Some(ci);
                            }
                            if ci + 1 < n && ui.small_button("↓").on_hover_text("Move down").clicked() {
                                swap = Some((ci, ci + 1));
                            }
                            if ci > 0 && ui.small_button("↑").on_hover_text("Move up").clicked() {
                                swap = Some((ci - 1, ci));
                            }
                        });
                    });
                    ui.separator();

                    // ── Split: editor left, output right ─────────────────────
                    ui.columns(2, |cols| {
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .id_salt(format!("ed_{ci}"))
                            .show(&mut cols[0], |ui| {
                                let resp = ui.add(
                                    egui::TextEdit::multiline(&mut self.cells[ci].source)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                );
                                if resp.changed() { self.cells[ci].dirty = true; }
                            });

                        let panel_w = cols[1].available_width();
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .id_salt(format!("out_{ci}"))
                            .show(&mut cols[1], |ui| {
                                render_rows(ui, &self.cells[ci].rows, &mut self.md_cache, panel_w, ci);
                            });
                    });
                });
            }

            if let Some(i) = to_delete {
                if self.cells.len() > 1 { self.cells.remove(i); }
            }
            if let Some((a, b)) = swap {
                self.cells.swap(a, b);
                // Mark both dirty so textures regenerate with correct IDs
                self.cells[a].dirty = true;
                self.cells[b].dirty = true;
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
        // Plot
        if let Some(ref pd) = row.plot_data {
            let plot_points: PlotPoints = pd.points.iter().map(|&[x, y]| [x, y]).collect();
            let line = Line::new(format!("{}_{}", cell_idx, i), plot_points);
            Plot::new(format!("plot_{}_{}", cell_idx, i))
                .height(180.0)
                .width(panel_w.min(500.0))
                .show(ui, |plot_ui| { plot_ui.line(line); });
            continue;
        }

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
            let mut visuals = egui::Visuals::light();
            visuals.text_cursor.stroke.color = egui::Color32::BLACK;
            visuals.text_cursor.stroke.width = 2.0;
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(App::new()))
        }),
    )
}
