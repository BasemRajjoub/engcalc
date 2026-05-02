mod calc;

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
        let searched = FontSearcher::new().include_system_fonts(false).search();
        let fonts: Vec<Font> = searched.fonts.iter().flat_map(|s| s.get()).collect();
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
    let img = egui::ColorImage {
        size: [w as usize, h as usize],
        source_size: egui::Vec2::new(w as f32, h as f32),
        pixels,
    };
    let tex = ctx.load_texture(id, img, egui::TextureOptions::LINEAR);
    Some((tex, [w as f32 / scale, h as f32 / scale]))
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendered row — one per document line
// ─────────────────────────────────────────────────────────────────────────────

struct RenderedRow {
    texture: Option<(egui::TextureHandle, [f32; 2])>,
    error:   Option<String>,
    comment: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// App
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_DOC: &str = "\
# 1 — Section properties (unit-aware)

b = 200 \"mm\"
h = 400 \"mm\"
A = b * h \"mm^2\"
I = b * h^3 / 12 \"mm^4\"
c = h / 2 \"mm\"
W = I / c \"mm^3\"

# 2 — Material

f_y = 250 \"MPa\"
E = 200000 \"MPa\"

# 3 — Loading

L = 6000 \"mm\"
w = 5 \"N/mm\"
F = 45 \"kN\"

# 4 — Beam bending (mixed units — fully automatic)

M_max = w * L^2 / 8 \"N·mm\"
M_Ed = F * L / 4 \"kN·m\"
sigma = M_max / W \"MPa\"
delta = 5 * w * L^4 / (384 * E * I) \"mm\"

# 5 — Auto unit inference (no declared unit)

A_auto = b * h
I_auto = b * h^3 / 12

# 6 — Symbolic differentiation

x = 3
dfdx = diff(x^3 + 2*x, x)

# 7 — Numeric integration & summation

A_circle = integrate(sqrt(1 - x^2), x, -1, 1)
S_squares = sum(k^2, k, 1, 10)

# 8 — Trig

theta = 0.7854
hyp = sqrt(sin(theta)^2 + cos(theta)^2)
";

struct App {
    world:    MinimalWorld,
    source:   String,
    rows:     Vec<RenderedRow>,
    dirty:    bool,
    md_cache: CommonMarkCache,
}

impl App {
    fn new() -> Self {
        eprintln!("Loading fonts…");
        let world = MinimalWorld::new();
        eprintln!("Fonts loaded.");
        Self {
            world,
            source:   DEFAULT_DOC.to_string(),
            rows:     Vec::new(),
            dirty:    true,
            md_cache: CommonMarkCache::default(),
        }
    }

    fn recompile(&mut self, ctx: &egui::Context) {
        self.dirty = false;
        let compiled = calc::compile_document(&self.source);
        self.rows = compiled.into_iter().enumerate().map(|(i, cl)| {
            if let Some(ref typst_src) = cl.typst_src {
                match compile_typst(&mut self.world, typst_src) {
                    Ok(svg) => {
                        let tex = rasterize(&svg, ctx, &format!("row_{i}"), 2.0);
                        RenderedRow { texture: tex, error: None, comment: None }
                    }
                    Err(e) => RenderedRow { texture: None, error: Some(e), comment: None },
                }
            } else {
                RenderedRow { texture: None, error: cl.error, comment: cl.comment }
            }
        }).collect();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.dirty { self.recompile(&ctx); }

        // ── Left: source editor ──────────────────────────────────────────────
        egui::SidePanel::left("source_panel")
            .resizable(true)
            .default_width(ui.available_width() / 2.0)
            .show_inside(ui, |ui| {
                ui.heading("Source");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut self.source)
                            .desired_width(f32::INFINITY)
                            .desired_rows(40)
                            .font(egui::TextStyle::Monospace),
                    );
                    if resp.changed() { self.dirty = true; }
                });
            });

        // ── Right: rendered rows ─────────────────────────────────────────────
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("Calculation");
            ui.separator();

            let panel_w = ui.available_width();
            let max_w   = 600.0_f32; // cap so wide panels don't over-scale

            egui::ScrollArea::vertical().show(ui, |ui| {
                let source_lines: Vec<&str> = self.source.lines().collect();

                for (i, row) in self.rows.iter().enumerate() {
                    let src_line = source_lines.get(i).copied().unwrap_or("").trim();

                    if src_line.is_empty() {
                        ui.add_space(6.0);
                        continue;
                    }

                    if let Some(ref err) = row.error {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(220, 60, 60),
                                format!("⚠  {src_line}  →  {err}"));
                        });
                        ui.add_space(4.0);
                        continue;
                    }

                    if let Some(ref text) = row.comment {
                        // Render comment lines as markdown (headings, bold, etc.)
                        let md = if src_line.starts_with("##") {
                            format!("## {text}")
                        } else {
                            format!("# {text}")
                        };
                        CommonMarkViewer::new().show(ui, &mut self.md_cache, &md);
                        ui.add_space(2.0);
                        continue;
                    }

                    if let Some((ref tex, size)) = row.texture {
                        let display_w = size[0].min(panel_w).min(max_w);
                        let scale     = display_w / size[0];
                        let display   = egui::Vec2::new(display_w, size[1] * scale);
                        ui.image((tex.id(), display));
                    }
                }
            });
        });
    }
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
                .with_inner_size([1100.0, 700.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            let mut visuals = egui::Visuals::light();
            visuals.text_cursor.stroke.color = egui::Color32::BLACK;
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(App::new()))
        }),
    )
}
