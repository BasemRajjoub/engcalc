use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// SI dimension vector [m, kg, s, A, K, mol, cd]
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dim(pub [i8; 7]);

impl Dim {
    pub const NONE: Dim = Dim([0,0,0,0,0,0,0]);
    pub const M:    Dim = Dim([1,0,0,0,0,0,0]);
    pub const KG:   Dim = Dim([0,1,0,0,0,0,0]);
    pub const S:    Dim = Dim([0,0,1,0,0,0,0]);
    pub const N:    Dim = Dim([1,1,-2,0,0,0,0]);   // kg·m/s²
    pub const PA:   Dim = Dim([-1,1,-2,0,0,0,0]);  // N/m²
    pub const J:    Dim = Dim([2,1,-2,0,0,0,0]);   // N·m
    pub const M2:   Dim = Dim([2,0,0,0,0,0,0]);
    pub const M3:   Dim = Dim([3,0,0,0,0,0,0]);
    pub const M4:   Dim = Dim([4,0,0,0,0,0,0]);

    pub fn mul(self, rhs: Dim) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 { d[i] = self.0[i] + rhs.0[i]; }
        Dim(d)
    }
    pub fn div(self, rhs: Dim) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 { d[i] = self.0[i] - rhs.0[i]; }
        Dim(d)
    }
    pub fn pow_int(self, n: i32) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 { d[i] = (self.0[i] as i32 * n) as i8; }
        Dim(d)
    }
    pub fn is_dimensionless(self) -> bool { self == Dim::NONE }
}

#[derive(Debug, Clone, Copy)]
pub struct Quantity {
    pub val: f64,
    pub dim: Dim,
    pub scale: f64,  // SI multiplier (e.g. mm → 0.001, MPa → 1e6)
}

impl Quantity {
    pub fn dimensionless(v: f64) -> Self { Self { val: v, dim: Dim::NONE, scale: 1.0 } }
    pub fn si_val(self) -> f64 { self.val * self.scale }
}

// ─────────────────────────────────────────────────────────────────────────────
// Base unit atom table  (symbol, Dim, SI_scale)
// ─────────────────────────────────────────────────────────────────────────────

pub fn base_unit_atoms() -> &'static [(&'static str, Dim, f64)] {
    &[
        ("mm",  Dim::M,   1e-3),
        ("cm",  Dim::M,   1e-2),
        ("m",   Dim::M,   1.0),
        ("km",  Dim::M,   1e3),
        ("in",  Dim::M,   0.0254),
        ("ft",  Dim::M,   0.3048),
        ("g",   Dim::KG,  1e-3),
        ("kg",  Dim::KG,  1.0),
        ("t",   Dim::KG,  1e3),
        ("ms",  Dim::S,   1e-3),
        ("s",   Dim::S,   1.0),
        ("min", Dim::S,   60.0),
        ("hr",  Dim::S,   3600.0),
        ("N",   Dim::N,   1.0),
        ("kN",  Dim::N,   1e3),
        ("MN",  Dim::N,   1e6),
        ("lbf", Dim::N,   4.448222),
        ("kip", Dim::N,   4448.222),
        ("Pa",  Dim::PA,  1.0),
        ("kPa", Dim::PA,  1e3),
        ("MPa", Dim::PA,  1e6),
        ("GPa", Dim::PA,  1e9),
        ("psi", Dim::PA,  6894.757),
        ("ksi", Dim::PA,  6.895e6),
        ("J",   Dim::J,   1.0),
        ("kJ",  Dim::J,   1e3),
        ("MJ",  Dim::J,   1e6),
        ("%",   Dim::NONE, 1.0),
        ("rad", Dim::NONE, 1.0),
        ("deg", Dim::NONE, std::f64::consts::PI / 180.0),
    ]
}

/// Parse a unit expression: atoms, ·/* (mul), / (div), ^ or superscript exponents.
pub fn parse_unit(u: &str) -> Option<(Dim, f64)> {
    let u = u.trim();
    if u.is_empty() { return Some((Dim::NONE, 1.0)); }
    if let Some(&(_, dim, scale)) = base_unit_atoms().iter().find(|&&(n,_,_)| n == u) {
        return Some((dim, scale));
    }
    parse_unit_expr(u)
}

fn parse_unit_expr(src: &str) -> Option<(Dim, f64)> {
    let s = src.replace('*', "\u{00B7}").replace('\u{22C5}', "\u{00B7}");
    let toks = unit_tokenise(&s)?;
    let mut dim   = Dim::NONE;
    let mut scale = 1.0_f64;
    let mut dividing = false;
    for tok in toks {
        match tok {
            UnitTok::Mul => { dividing = false; }
            UnitTok::Div => { dividing = true;  }
            UnitTok::Atom(name, exp) => {
                let (adim, ascale) = resolve_atom(&name, exp)?;
                if dividing { dim = dim.div(adim); scale /= ascale; dividing = false; }
                else        { dim = dim.mul(adim); scale *= ascale; }
            }
        }
    }
    Some((dim, scale))
}

#[derive(Debug)]
enum UnitTok { Mul, Div, Atom(String, i32) }

fn unit_tokenise(s: &str) -> Option<Vec<UnitTok>> {
    let ch: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < ch.len() {
        match ch[i] {
            '\u{00B7}' => { toks.push(UnitTok::Mul); i += 1; }
            '/' => { toks.push(UnitTok::Div); i += 1; }
            ' ' | '\t' => { i += 1; }
            c if c.is_alphabetic() || c == '%' => {
                let mut name = String::new();
                while i < ch.len() && (ch[i].is_alphabetic() || ch[i] == '%') {
                    name.push(ch[i]); i += 1;
                }
                let exp = parse_exponent(&ch, &mut i);
                toks.push(UnitTok::Atom(name, exp));
            }
            _ => return None,
        }
    }
    Some(toks)
}

fn parse_exponent(ch: &[char], i: &mut usize) -> i32 {
    if *i < ch.len() && ch[*i] == '^' {
        *i += 1;
        let neg = *i < ch.len() && ch[*i] == '-';
        if neg { *i += 1; }
        let mut ds = String::new();
        while *i < ch.len() && ch[*i].is_ascii_digit() { ds.push(ch[*i]); *i += 1; }
        let n: i32 = ds.parse().unwrap_or(1);
        return if neg { -n } else { n };
    }
    let mut exp_s = String::new();
    let mut neg = false;
    while *i < ch.len() {
        match ch[*i] {
            '\u{207B}' => { neg = true; *i += 1; }
            '\u{2070}' => { exp_s.push('0'); *i += 1; }
            '\u{00B9}' => { exp_s.push('1'); *i += 1; }
            '\u{00B2}' => { exp_s.push('2'); *i += 1; }
            '\u{00B3}' => { exp_s.push('3'); *i += 1; }
            '\u{2074}' => { exp_s.push('4'); *i += 1; }
            '\u{2075}' => { exp_s.push('5'); *i += 1; }
            '\u{2076}' => { exp_s.push('6'); *i += 1; }
            '\u{2077}' => { exp_s.push('7'); *i += 1; }
            '\u{2078}' => { exp_s.push('8'); *i += 1; }
            '\u{2079}' => { exp_s.push('9'); *i += 1; }
            _ => break,
        }
    }
    if exp_s.is_empty() { 1 } else {
        let n: i32 = exp_s.parse().unwrap_or(1);
        if neg { -n } else { n }
    }
}

fn resolve_atom(name: &str, exp: i32) -> Option<(Dim, f64)> {
    base_unit_atoms().iter()
        .find(|&&(n,_,_)| n == name)
        .map(|&(_, dim, scale)| (dim.pow_int(exp), scale.powi(exp)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Named display unit table + auto-inference
// ─────────────────────────────────────────────────────────────────────────────

fn named_display_units() -> &'static [(&'static str, Dim, f64)] {
    &[
        ("mm",   Dim::M,   1e-3),
        ("cm",   Dim::M,   1e-2),
        ("m",    Dim::M,   1.0),
        ("km",   Dim::M,   1e3),
        ("mm²",  Dim::M2,  1e-6),
        ("cm²",  Dim::M2,  1e-4),
        ("m²",   Dim::M2,  1.0),
        ("mm³",  Dim::M3,  1e-9),
        ("cm³",  Dim::M3,  1e-6),
        ("m³",   Dim::M3,  1.0),
        ("mm⁴",  Dim::M4,  1e-12),
        ("cm⁴",  Dim::M4,  1e-8),
        ("m⁴",   Dim::M4,  1.0),
        ("g",    Dim::KG,  1e-3),
        ("kg",   Dim::KG,  1.0),
        ("t",    Dim::KG,  1e3),
        ("N",    Dim::N,   1.0),
        ("kN",   Dim::N,   1e3),
        ("MN",   Dim::N,   1e6),
        ("N·mm", Dim::J,   1e-3),
        ("N·m",  Dim::J,   1.0),
        ("kN·m", Dim::J,   1e3),
        ("MN·m", Dim::J,   1e6),
        ("Pa",   Dim::PA,  1.0),
        ("kPa",  Dim::PA,  1e3),
        ("MPa",  Dim::PA,  1e6),
        ("GPa",  Dim::PA,  1e9),
        ("N/m",  Dim([0,1,-2,0,0,0,0]), 1.0),
        ("kN/m", Dim([0,1,-2,0,0,0,0]), 1e3),
        ("N/mm", Dim([0,1,-2,0,0,0,0]), 1e3),
    ]
}

/// Given a Quantity with no declared unit, infer the best display unit.
pub fn infer_display_unit(q: &Quantity) -> Option<(&'static str, f64)> {
    if q.dim.is_dimensionless() { return None; }
    let si = q.si_val();
    let candidates: Vec<_> = named_display_units().iter()
        .filter(|&&(_, d, _)| d == q.dim)
        .collect();
    if candidates.is_empty() { return None; }
    let score = |v: f64| -> f64 {
        let a = v.abs();
        if a == 0.0 { return f64::MAX; }
        let l = a.log10();
        if l >= 0.0 && l < 3.0 { l } else if l < 0.0 { -l * 10.0 } else { (l - 3.0) * 10.0 }
    };
    let best = candidates.iter().min_by(|&&(_, _, sa), &&(_, _, sb)| {
        score(si / sa).partial_cmp(&score(si / sb)).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    Some((best.0, si / best.2))
}
