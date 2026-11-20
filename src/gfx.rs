//! Anti-aliased 2D drawing on a premultiplied RGBA pixmap (tiny-skia) plus
//! text via ab_glyph using a system font. All coordinates are in *screen*
//! pixels; `ox/oy` is the screen position of the pixmap's top-left corner.

use crate::util::{Col, P};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use std::sync::OnceLock;
use tiny_skia::*;

static FONT: OnceLock<Option<FontArc>> = OnceLock::new();

pub fn font() -> Option<&'static FontArc> {
    FONT.get_or_init(|| {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        for f in ["segoeuib.ttf", "seguisb.ttf", "arialbd.ttf", "segoeui.ttf", "arial.ttf", "tahoma.ttf"] {
            let p = format!("{}\\Fonts\\{}", windir, f);
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(font) = FontArc::try_from_vec(bytes) {
                    return Some(font);
                }
            }
        }
        None
    })
    .as_ref()
}

pub fn text_width(size: f32, s: &str) -> f32 {
    let Some(f) = font() else { return s.len() as f32 * size * 0.55 };
    let sf = f.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in s.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

pub struct Gfx {
    pub pm: Pixmap,
    pub ox: f32,
    pub oy: f32,
    pub alpha: f32,
}

impl Gfx {
    pub fn new(w: u32, h: u32) -> Gfx {
        Gfx { pm: Pixmap::new(w.max(1), h.max(1)).unwrap(), ox: 0.0, oy: 0.0, alpha: 1.0 }
    }

    pub fn reset(&mut self, ox: f32, oy: f32, w: u32, h: u32, bg: Option<Col>) {
        if self.pm.width() != w || self.pm.height() != h {
            self.pm = Pixmap::new(w.max(1), h.max(1)).unwrap();
        }
        match bg {
            Some(c) => self.pm.fill(Color::from_rgba8(c.r, c.g, c.b, c.a)),
            None => self.pm.fill(Color::TRANSPARENT),
        }
        self.ox = ox;
        self.oy = oy;
        self.alpha = 1.0;
    }

    pub fn w(&self) -> u32 {
        self.pm.width()
    }
    pub fn h(&self) -> u32 {
        self.pm.height()
    }

    fn tf(&self) -> Transform {
        Transform::from_translate(-self.ox, -self.oy)
    }

    fn paint(&self, c: Col) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(c.r, c.g, c.b, (c.a as f32 * self.alpha).clamp(0.0, 255.0) as u8);
        p.anti_alias = true;
        p
    }

    fn stroke(w: f32) -> Stroke {
        Stroke { width: w.max(0.5), line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() }
    }

    pub fn line(&mut self, a: P, b: P, w: f32, c: Col) {
        self.polyline(&[a, b], w, c);
    }

    pub fn polyline(&mut self, pts: &[P], w: f32, c: Col) {
        if pts.len() < 2 || c.a == 0 {
            return;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        if let Some(path) = pb.finish() {
            let paint = self.paint(c);
            self.pm.stroke_path(&path, &paint, &Self::stroke(w), self.tf(), None);
        }
    }

    /// Polyline whose width tapers from w0 to w1 (whips, ribbons, speed lines).
    pub fn taper(&mut self, pts: &[P], w0: f32, w1: f32, c: Col) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        for i in 0..n - 1 {
            let t = i as f32 / (n - 1) as f32;
            self.line(pts[i], pts[i + 1], w0 + (w1 - w0) * t, c);
        }
    }

    pub fn circle(&mut self, c: P, r: f32, col: Col) {
        if r <= 0.1 || col.a == 0 {
            return;
        }
        if let Some(path) = PathBuilder::from_circle(c.0, c.1, r) {
            let paint = self.paint(col);
            self.pm.fill_path(&path, &paint, FillRule::Winding, self.tf(), None);
        }
    }

    pub fn ring(&mut self, c: P, r: f32, w: f32, col: Col) {
        if r <= 0.1 || col.a == 0 {
            return;
        }
        if let Some(path) = PathBuilder::from_circle(c.0, c.1, r) {
            let paint = self.paint(col);
            self.pm.stroke_path(&path, &paint, &Self::stroke(w), self.tf(), None);
        }
    }

    fn path(pts: &[P]) -> Option<Path> {
        if pts.len() < 3 {
            return None;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
        pb.finish()
    }

    pub fn poly(&mut self, pts: &[P], col: Col) {
        if col.a == 0 {
            return;
        }
        if let Some(path) = Self::path(pts) {
            let paint = self.paint(col);
            self.pm.fill_path(&path, &paint, FillRule::Winding, self.tf(), None);
        }
    }

    pub fn poly_stroke(&mut self, pts: &[P], w: f32, col: Col) {
        if let Some(path) = Self::path(pts) {
            let paint = self.paint(col);
            self.pm.stroke_path(&path, &paint, &Self::stroke(w), self.tf(), None);
        }
    }

    /// Points of an ellipse centred at c, radii rx/ry, rotated by `rot` radians.
    pub fn ellipse_pts(c: P, rx: f32, ry: f32, rot: f32) -> Vec<P> {
        let n = 28;
        let (s, co) = rot.sin_cos();
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                let (x, y) = (a.cos() * rx, a.sin() * ry);
                (c.0 + x * co - y * s, c.1 + x * s + y * co)
            })
            .collect()
    }

    pub fn ellipse(&mut self, c: P, rx: f32, ry: f32, rot: f32, col: Col) {
        let pts = Self::ellipse_pts(c, rx, ry, rot);
        self.poly(&pts, col);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, col: Col) {
        self.poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], col);
    }

    /// Rotated rectangle given its centre, along direction `ang` (radians).
    pub fn rrect(&mut self, c: P, len: f32, thick: f32, ang: f32, col: Col) {
        let (s, co) = ang.sin_cos();
        let (hx, hy) = (co * len / 2.0, s * len / 2.0);
        let (px, py) = (-s * thick / 2.0, co * thick / 2.0);
        self.poly(
            &[
                (c.0 - hx - px, c.1 - hy - py),
                (c.0 + hx - px, c.1 + hy - py),
                (c.0 + hx + px, c.1 + hy + py),
                (c.0 - hx + px, c.1 - hy + py),
            ],
            col,
        );
    }

    /// Draw text. `y` is the vertical centre of the line, `align` -1/0/1.
    pub fn text(&mut self, x: f32, y: f32, size: f32, col: Col, s: &str, align: i8) {
        let Some(f) = font() else { return };
        let scale = PxScale::from(size);
        let sf = f.as_scaled(scale);
        let w = text_width(size, s);
        let mut cx = match align {
            -1 => x,
            0 => x - w / 2.0,
            _ => x - w,
        } - self.ox;
        let base = y - self.oy + (sf.ascent() + sf.descent()) / 2.0;
        let (pw, ph) = (self.pm.width() as i32, self.pm.height() as i32);
        let a_mul = col.a as f32 / 255.0 * self.alpha;
        let pixels = self.pm.pixels_mut();
        let mut prev = None;
        for ch in s.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                cx += sf.kern(p, id);
            }
            let g = id.with_scale_and_position(scale, point(cx, base));
            cx += sf.h_advance(id);
            prev = Some(id);
            if let Some(og) = f.outline_glyph(g) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px < 0 || py < 0 || px >= pw || py >= ph {
                        return;
                    }
                    let a = (cov * a_mul).clamp(0.0, 1.0);
                    if a <= 0.0 {
                        return;
                    }
                    let d = &mut pixels[(py * pw + px) as usize];
                    let inv = 1.0 - a;
                    let r = (col.r as f32 * a + d.red() as f32 * inv) as u8;
                    let gg = (col.g as f32 * a + d.green() as f32 * inv) as u8;
                    let bb = (col.b as f32 * a + d.blue() as f32 * inv) as u8;
                    let aa = (255.0 * a + d.alpha() as f32 * inv) as u8;
                    if let Some(c) = PremultipliedColorU8::from_rgba(r.min(aa), gg.min(aa), bb.min(aa), aa) {
                        *d = c;
                    }
                });
            }
        }
    }

    /// Filled rounded rectangle in local coordinates mapped by `tf` (which
    /// must already include the pixmap offset).
    pub fn round_rect_t(&mut self, tf: Transform, x: f32, y: f32, w: f32, h: f32, r: f32, c: Col) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
        if let Some(path) = pb.finish() {
            let paint = self.paint(c);
            self.pm.fill_path(&path, &paint, FillRule::Winding, tf, None);
        }
    }

    /// Comic-style text with a dark outline.
    pub fn text_outlined(&mut self, x: f32, y: f32, size: f32, fill: Col, outline: Col, s: &str) {
        let o = (size / 12.0).max(1.0);
        for (dx, dy) in [(-o, 0.0), (o, 0.0), (0.0, -o), (0.0, o), (-o, -o), (o, o), (-o, o), (o, -o)] {
            self.text(x + dx, y + dy, size, outline, s, 0);
        }
        self.text(x, y, size, fill, s, 0);
    }

    /// Draw something into an offscreen layer covering `rect`, then blend
    /// the whole layer with `opacity` (no seams between overlapping shapes).
    pub fn layer(&mut self, rect: (f32, f32, f32, f32), opacity: f32, f: impl FnOnce(&mut Gfx)) {
        if opacity >= 0.995 {
            f(self);
            return;
        }
        let (l, t) = (rect.0.floor(), rect.1.floor());
        let w = (rect.2 - l).ceil().max(1.0) as u32;
        let h = (rect.3 - t).ceil().max(1.0) as u32;
        let mut sub = Gfx::new(w, h);
        sub.ox = l;
        sub.oy = t;
        f(&mut sub);
        let paint = PixmapPaint { opacity: opacity.clamp(0.0, 1.0) * self.alpha, ..Default::default() };
        self.pm.draw_pixmap((l - self.ox) as i32, (t - self.oy) as i32, sub.pm.as_ref(), &paint, Transform::identity(), None);
    }

    /// Copy into a BGRA premultiplied buffer (what Win32 layered windows want).
    pub fn copy_bgra(&self, dst: &mut [u8]) {
        let src = self.pm.data();
        let n = src.len().min(dst.len());
        let mut i = 0;
        while i + 3 < n {
            dst[i] = src[i + 2];
            dst[i + 1] = src[i + 1];
            dst[i + 2] = src[i];
            dst[i + 3] = src[i + 3];
            i += 4;
        }
    }

    /// Fill fully transparent pixels in a rect with alpha=1 so the area is
    /// clickable (invisible to the eye, but not click-through).
    pub fn hit_area(&mut self, l: f32, t: f32, r: f32, b: f32) {
        let (pw, ph) = (self.pm.width() as i32, self.pm.height() as i32);
        let x0 = ((l - self.ox) as i32).clamp(0, pw);
        let x1 = ((r - self.ox) as i32).clamp(0, pw);
        let y0 = ((t - self.oy) as i32).clamp(0, ph);
        let y1 = ((b - self.oy) as i32).clamp(0, ph);
        let px = self.pm.pixels_mut();
        let one = PremultipliedColorU8::from_rgba(0, 0, 0, 1).unwrap();
        for y in y0..y1 {
            for x in x0..x1 {
                let d = &mut px[(y * pw + x) as usize];
                if d.alpha() == 0 {
                    *d = one;
                }
            }
        }
    }
}
