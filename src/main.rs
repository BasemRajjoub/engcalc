mod calc;
mod export;

use eframe::egui;
use resvg::{tiny_skia, usvg};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, ChildStderr, Command, Stdio};

// ─────────────────────────────────────────────────────────────────────────────
// Persistent Node/MathJax subprocess for LaTeX → SVG
// ─────────────────────────────────────────────────────────────────────────────

struct MjProcess {
    _child:  Child,
    stdin:   ChildStdin,
    stdout:  BufReader<ChildStdout>,
}

impl MjProcess {
    fn start() -> Option<Self> {
        let script = std::env::current_exe().ok()?
            .parent()?
            .join("mj.mjs");
        let script = if script.exists() { script } else {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mj.mjs")
        };
        let mut child = Command::new("node")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn().ok()?;
        let stdin  = child.stdin.take()?;
        let stdout = BufReader::new(child.stdout.take()?);
        let mut stderr = BufReader::new(child.stderr.take()?);

        // Wait for READY signal (MathJax init can take ~2s)
        let mut line = String::new();
        loop {
            line.clear();
            match stderr.read_line(&mut line) {
                Ok(0) => { eprintln!("[mj] process exited before READY"); return None; }
                Ok(_) => { if line.contains("READY") { break; } }
                Err(_) => return None,
            }
        }

        // Drain remaining stderr to avoid blocking
        std::thread::spawn(move || {
            let mut buf = String::new();
            loop {
                buf.clear();
                match stderr.read_line(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });

        Some(MjProcess { _child: child, stdin, stdout })
    }

    fn convert(&mut self, latex: &str) -> Option<String> {
        let line = latex.replace('\n', " ");
        writeln!(self.stdin, "{}", line).ok()?;
        self.stdin.flush().ok()?;
        let mut response = String::new();
        self.stdout.read_line(&mut response).ok()?;
        let response = response.trim_end_matches('\n').trim_end_matches('\r').to_string();
        if response.starts_with("ERROR:") { return None; }
        Some(response)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Math texture rendering via MathJax → SVG → resvg
// ─────────────────────────────────────────────────────────────────────────────

struct RenderedEq {
    texture: egui::TextureHandle,
    width:   f32,
    height:  f32,
}

fn svg_to_texture(ctx: &egui::Context, svg: &str, name: &str, scale: f32) -> Option<RenderedEq> {
    let svg_colored = svg.replace("currentColor", "#1a1a1a");
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&svg_colored, &opt).ok()?;
    let size = tree.size().to_int_size();
    let w = ((size.width()  as f32) * scale) as u32;
    let h = ((size.height() as f32) * scale) as u32;
    if w == 0 || h == 0 { return None; }
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data());
    let tex = ctx.load_texture(name.to_string(), img, egui::TextureOptions::LINEAR);
    Some(RenderedEq { texture: tex, width: w as f32 / scale, height: h as f32 / scale })
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendered preview row
// ─────────────────────────────────────────────────────────────────────────────

enum PreviewRow {
    Equation(RenderedEq),
    Error(String),
    Heading(String),
    PlotGroup(Vec<calc::PlotData>),
    Table(calc::TableData),
    Blank,
}

// ─────────────────────────────────────────────────────────────────────────────
// Default document
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_DOC: &str = "\
# Section properties
b = 200 \"mm\"
h = 400 \"mm\"
A = b * h \"mm^2\"
I = b * h^3 / 12 \"mm^4\"
c = h / 2 \"mm\"
W = I / c \"mm^3\"

# Material & loading
f_y = 250 \"MPa\"
E = 200000 \"MPa\"
L = 6000 \"mm\"
w = 5 \"N/mm\"
F = 45 \"kN\"

# Beam bending
M_max = w * L^2 / 8 \"N·mm\"
M_Ed = F * L / 4 \"kN·m\"
sigma = M_max / W \"MPa\"
delta = 5 * w * L^4 / (384 * E * I) \"mm\"

# Calculus
x = 3
dfdx = diff(x^3 + 2*x, x)
A_circle = integrate(sqrt(1 - x^2), x, -1, 1)
S_squares = sum(k^2, k, 1, 10)

# Plots
plot(sin(x), x, -6.28, 6.28) \"sin(x)\"
plot(cos(x), x, -6.28, 6.28) \"cos(x)\"

# Results table
| Parameter | Value |
|-----------|-------|
| b | 200 mm |
| h | 400 mm |
| A | 80000 mm^2 |
";

// ─────────────────────────────────────────────────────────────────────────────
// App
// ─────────────────────────────────────────────────────────────────────────────

struct App {
    source:      String,
    preview:     Vec<PreviewRow>,
    dirty:       bool,
    tex_counter: usize,
    mj:          Option<MjProcess>,
}

impl App {
    fn new() -> Self {
        Self {
            source: DEFAULT_DOC.to_string(),
            preview: Vec::new(),
            dirty: true,
            tex_counter: 0,
            mj: MjProcess::start(),
        }
    }

    fn recompile(&mut self, ctx: &egui::Context) {
        self.dirty = false;
        let compiled = calc::compile_document(&self.source);
        let mut rows: Vec<PreviewRow> = Vec::new();
        let mut plot_group: Vec<calc::PlotData> = Vec::new();

        let flush_plots = |group: &mut Vec<calc::PlotData>, rows: &mut Vec<PreviewRow>| {
            if group.is_empty() { return; }
            rows.push(PreviewRow::PlotGroup(std::mem::take(group)));
        };

        for cl in compiled {
            if cl.plot_data.is_none() {
                flush_plots(&mut plot_group, &mut rows);
            }

            if let Some(pd) = cl.plot_data {
                plot_group.push(pd);
                continue;
            }

            if let Some(td) = cl.table_data {
                rows.push(PreviewRow::Table(td));
                continue;
            }

            if let Some(text) = cl.comment {
                rows.push(PreviewRow::Heading(text));
                continue;
            }

            if let Some(err) = cl.error {
                rows.push(PreviewRow::Error(err));
                continue;
            }

            if let Some(latex) = cl.latex {
                self.tex_counter += 1;
                let name = format!("eq_{}", self.tex_counter);
                let svg = self.mj.as_mut().and_then(|mj| mj.convert(&latex));
                match svg.and_then(|s| svg_to_texture(ctx, &s, &name, 4.0)) {
                    Some(eq) => rows.push(PreviewRow::Equation(eq)),
                    None => rows.push(PreviewRow::Error(format!("render failed: {latex}"))),
                }
                continue;
            }

            if cl.source_line.trim().is_empty() {
                rows.push(PreviewRow::Blank);
            }
        }
        flush_plots(&mut plot_group, &mut rows);
        self.preview = rows;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(egui::Theme::Light);
        set_light_visuals(ui);

        if self.dirty {
            self.recompile(&ctx);
        }

        // ── Toolbar ──────────────────────────────────────────────────────────
        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("eqgui");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Export HTML").clicked() {
                        let mj = &mut self.mj;
                        let html = export::html::export_html(&self.source, &mut |latex| {
                            mj.as_mut().and_then(|m| m.convert(latex))
                        });
                        save_file(html.into_bytes(), "eqgui_export.html", "HTML file (*.html)|*.html");
                    }
                });
            });
        });

        // ── Split pane ────────────────────────────────────────────────────────
        let available = ui.available_rect_before_wrap();
        let split = available.width() * 0.45;

        // Editor — left panel
        egui::Panel::left("editor_panel")
            .exact_size(split)
            .resizable(true)
            .show_inside(ui, |ui| {
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

        // Preview — right panel
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let panel_w = ui.available_width();
                render_preview(ui, &self.preview, panel_w);
            });
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Preview rendering
// ─────────────────────────────────────────────────────────────────────────────

fn render_preview(ui: &mut egui::Ui, rows: &[PreviewRow], panel_w: f32) {
    let max_eq_w = (panel_w - 16.0).min(640.0);

    for row in rows {
        match row {
            PreviewRow::Blank => { ui.add_space(6.0); }

            PreviewRow::Heading(text) => {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(text).size(15.0).strong().color(egui::Color32::from_rgb(26, 58, 92)));
                ui.add(egui::Separator::default().spacing(4.0));
            }

            PreviewRow::Error(err) => {
                ui.colored_label(egui::Color32::from_rgb(200, 50, 50), format!("⚠ {err}"));
                ui.add_space(2.0);
            }

            PreviewRow::Equation(eq) => {
                let display_w = eq.width.min(max_eq_w);
                let scale = display_w / eq.width;
                let display_h = eq.height * scale;
                ui.add_space(2.0);
                ui.image((eq.texture.id(), egui::Vec2::new(display_w, display_h)));
                ui.add_space(2.0);
            }

            PreviewRow::Table(td) => {
                egui::Grid::new(format!("tbl_{:p}", td))
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for h in &td.header { ui.strong(h); }
                        ui.end_row();
                        for row in &td.rows {
                            for cell in row { ui.label(cell); }
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
            }

            PreviewRow::PlotGroup(plots) => {
                draw_plot(ui, plots, panel_w - 16.0, 200.0);
                ui.add_space(4.0);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Simple painter-based line plot (no egui_plot dependency)
// ─────────────────────────────────────────────────────────────────────────────

fn draw_plot(ui: &mut egui::Ui, plots: &[calc::PlotData], width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(252));
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(200)), egui::StrokeKind::Outside);

    if plots.is_empty() { return; }

    // Compute data bounds across all series
    let mut x_min = f64::MAX;
    let mut x_max = f64::MIN;
    let mut y_min = f64::MAX;
    let mut y_max = f64::MIN;
    for pd in plots {
        for [x, y] in &pd.points {
            if x.is_finite() { x_min = x_min.min(*x); x_max = x_max.max(*x); }
            if y.is_finite() { y_min = y_min.min(*y); y_max = y_max.max(*y); }
        }
    }
    if x_min >= x_max { x_max = x_min + 1.0; }
    if y_min >= y_max { y_max = y_min + 1.0; }
    let pad_y = (y_max - y_min) * 0.08;
    y_min -= pad_y; y_max += pad_y;

    let pad = 8.0_f32;
    let plot_rect = egui::Rect::from_min_max(
        rect.min + egui::Vec2::splat(pad),
        rect.max - egui::Vec2::splat(pad),
    );

    let to_screen = |x: f64, y: f64| -> egui::Pos2 {
        let fx = ((x - x_min) / (x_max - x_min)) as f32;
        let fy = 1.0 - ((y - y_min) / (y_max - y_min)) as f32;
        egui::Pos2::new(
            plot_rect.min.x + fx * plot_rect.width(),
            plot_rect.min.y + fy * plot_rect.height(),
        )
    };

    let colors = [
        egui::Color32::from_rgb(37, 99, 235),
        egui::Color32::from_rgb(220, 38, 38),
        egui::Color32::from_rgb(22, 163, 74),
        egui::Color32::from_rgb(217, 119, 6),
        egui::Color32::from_rgb(147, 51, 234),
        egui::Color32::from_rgb(6, 182, 212),
    ];

    for (i, pd) in plots.iter().enumerate() {
        let color = colors[i % colors.len()];
        let screen_pts: Vec<egui::Pos2> = pd.points.iter()
            .filter(|[x, y]| x.is_finite() && y.is_finite())
            .map(|[x, y]| to_screen(*x, *y))
            .collect();
        if screen_pts.len() >= 2 {
            painter.add(egui::Shape::line(screen_pts, egui::Stroke::new(2.0, color)));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn set_light_visuals(ui: &mut egui::Ui) {
    let white  = egui::Color32::WHITE;
    let near_w = egui::Color32::from_gray(248);
    ui.visuals_mut().panel_fill       = white;
    ui.visuals_mut().extreme_bg_color = near_w;
    ui.visuals_mut().window_fill      = white;
}

fn save_file(bytes: Vec<u8>, default_name: &str, filter: &str) {
    let ps = format!(
        "Add-Type -AssemblyName System.Windows.Forms\n\
         $d = New-Object System.Windows.Forms.SaveFileDialog\n\
         $d.Filter = '{filter}'\n\
         $d.FileName = '{default_name}'\n\
         if ($d.ShowDialog() -eq 'OK') {{ $d.FileName }} else {{ '' }}"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output();

    let path = match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { return; }
            s
        }
        Err(_) => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            format!("{default_name}_{ts}")
        }
    };

    if let Err(e) = std::fs::write(&path, &bytes) {
        eprintln!("Export failed: {e}");
    } else {
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &path]).spawn();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "eqgui",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("eqgui")
                .with_inner_size([1200.0, 800.0]),
            ..Default::default()
        },
        Box::new(|cc| {
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
