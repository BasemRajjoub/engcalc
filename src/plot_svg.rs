use crate::calc::document::PlotData;

const W: f64 = 520.0;
const H: f64 = 280.0;
const PAD_L: f64 = 54.0;
const PAD_R: f64 = 16.0;
const PAD_T: f64 = 16.0;
const PAD_B: f64 = 42.0;

const PLOT_W: f64 = W - PAD_L - PAD_R;
const PLOT_H: f64 = H - PAD_T - PAD_B;

const COLORS: &[&str] = &[
    "#2563eb", "#dc2626", "#16a34a", "#d97706", "#7c3aed", "#0891b2",
];

pub fn render_plot_svg(series: &[&PlotData]) -> String {
    if series.is_empty() { return String::new(); }

    let (x_min, x_max) = series.iter()
        .flat_map(|s| s.points.iter().map(|p| p[0]))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let (y_min, y_max) = series.iter()
        .flat_map(|s| s.points.iter().map(|p| p[1]))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));

    let x_min = if x_min.is_finite() { x_min } else { -1.0 };
    let x_max = if x_max.is_finite() && x_max > x_min { x_max } else { x_min + 1.0 };
    let y_min = if y_min.is_finite() { y_min } else { -1.0 };
    let y_max = if y_max.is_finite() && y_max > y_min { y_max } else { y_min + 1.0 };

    let y_span = y_max - y_min;
    let y_lo = y_min - y_span * 0.05;
    let y_hi = y_max + y_span * 0.05;

    let to_px = |x: f64, y: f64| -> (f64, f64) {
        let px = PAD_L + (x - x_min) / (x_max - x_min) * PLOT_W;
        let py = PAD_T + (1.0 - (y - y_lo) / (y_hi - y_lo)) * PLOT_H;
        (px, py)
    };

    let mut s = String::with_capacity(8192);

    // SVG uses single-quoted attributes throughout to avoid raw-string delimiter clashes
    s.push_str(&format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{H}' style='background:#fff;font-family:sans-serif'>"
    ));

    let x_ticks = nice_ticks(x_min, x_max, 6);
    let y_ticks = nice_ticks(y_lo, y_hi, 5);

    s.push_str(&format!(
        "<clipPath id='cp'><rect x='{PAD_L}' y='{PAD_T}' width='{PLOT_W}' height='{PLOT_H}'/></clipPath>"
    ));

    // Grid verticals
    let grid_y2 = PAD_T + PLOT_H;
    for &xv in &x_ticks {
        let (px, _) = to_px(xv, y_lo);
        s.push_str(&format!(
            "<line x1='{px:.1}' y1='{PAD_T}' x2='{px:.1}' y2='{grid_y2}' stroke='#e5e7eb' stroke-width='1'/>"
        ));
    }
    // Grid horizontals
    let grid_x2 = PAD_L + PLOT_W;
    for &yv in &y_ticks {
        let (_, py) = to_px(x_min, yv);
        s.push_str(&format!(
            "<line x1='{PAD_L}' y1='{py:.1}' x2='{grid_x2}' y2='{py:.1}' stroke='#e5e7eb' stroke-width='1'/>"
        ));
    }

    // x-axis (at y=0 if visible, else bottom)
    let y_zero = y_lo.max(0f64.min(y_hi));
    let (_, y_zero_px) = to_px(x_min, y_zero);
    s.push_str(&format!(
        "<line x1='{PAD_L}' y1='{y_zero_px:.1}' x2='{grid_x2}' y2='{y_zero_px:.1}' stroke='#374151' stroke-width='1.5'/>"
    ));
    // y-axis (at x=0 if visible, else left)
    let x_zero = x_min.max(0f64.min(x_max));
    let (x_zero_px, _) = to_px(x_zero, y_lo);
    let axis_y2 = PAD_T + PLOT_H;
    s.push_str(&format!(
        "<line x1='{x_zero_px:.1}' y1='{PAD_T}' x2='{x_zero_px:.1}' y2='{axis_y2}' stroke='#374151' stroke-width='1.5'/>"
    ));

    // X tick labels
    let tick_y = PAD_T + PLOT_H + 14.0;
    for &xv in &x_ticks {
        let (px, _) = to_px(xv, y_lo);
        let label = fmt_tick(xv);
        s.push_str(&format!(
            "<text x='{px:.1}' y='{tick_y}' text-anchor='middle' font-size='11' fill='#6b7280'>{label}</text>"
        ));
    }
    // Y tick labels
    let tick_x = PAD_L - 5.0;
    for &yv in &y_ticks {
        let (_, py) = to_px(x_min, yv);
        let label = fmt_tick(yv);
        s.push_str(&format!(
            "<text x='{tick_x}' y='{py:.1}' text-anchor='end' dominant-baseline='middle' font-size='11' fill='#6b7280'>{label}</text>"
        ));
    }

    // Series paths
    for (si, sr) in series.iter().enumerate() {
        let color = COLORS[si % COLORS.len()];
        if sr.points.is_empty() { continue; }
        let mut d = String::new();
        for (i, &[x, y]) in sr.points.iter().enumerate() {
            let (px, py) = to_px(x, y);
            if i == 0 { d.push_str(&format!("M{px:.2},{py:.2}")) }
            else       { d.push_str(&format!(" L{px:.2},{py:.2}")) }
        }
        s.push_str(&format!(
            "<path d='{d}' fill='none' stroke='{color}' stroke-width='2' stroke-linejoin='round' clip-path='url(#cp)'/>"
        ));
    }

    // Legend
    let lx = PAD_L + 8.0;
    let ly0 = PAD_T + 8.0;
    for (si, sr) in series.iter().enumerate() {
        if sr.label.is_empty() { continue; }
        let color = COLORS[si % COLORS.len()];
        let ly = ly0 + si as f64 * 18.0;
        let lx2 = lx + 20.0;
        s.push_str(&format!(
            "<line x1='{lx}' y1='{ly}' x2='{lx2}' y2='{ly}' stroke='{color}' stroke-width='2'/>"
        ));
        let label = escape_xml(&sr.label);
        let tx = lx + 26.0;
        s.push_str(&format!(
            "<text x='{tx}' y='{ly}' dominant-baseline='middle' font-size='12' fill='#111827'>{label}</text>"
        ));
    }

    // Border rect
    s.push_str(&format!(
        "<rect x='{PAD_L}' y='{PAD_T}' width='{PLOT_W}' height='{PLOT_H}' fill='none' stroke='#d1d5db' stroke-width='1'/>"
    ));

    s.push_str("</svg>");
    s
}

fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    let span = hi - lo;
    if span <= 0.0 || !span.is_finite() { return vec![lo]; }
    let step_raw = span / target as f64;
    let mag = step_raw.log10().floor();
    let pow = 10f64.powf(mag);
    let step = [1.0, 2.0, 5.0, 10.0].iter()
        .map(|&s| s * pow)
        .min_by(|a, b| (a - step_raw).abs().partial_cmp(&(b - step_raw).abs()).unwrap())
        .unwrap_or(step_raw);
    let first = (lo / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = first;
    while v <= hi + step * 1e-9 {
        ticks.push(v);
        v += step;
        if ticks.len() > 20 { break; }
    }
    ticks
}

fn fmt_tick(v: f64) -> String {
    if v == 0.0 { return "0".into(); }
    let abs = v.abs();
    if abs >= 1e4 || abs < 1e-3 { format!("{v:.2e}") }
    else if abs >= 100.0         { format!("{v:.0}") }
    else if abs >= 1.0           { format!("{v:.2}") }
    else                         { format!("{v:.3}") }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
