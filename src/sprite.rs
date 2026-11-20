//! Animated GIF characters. A pack can set `"gif": "file.gif"` (relative to
//! the packs folder) and the character is drawn as that animation instead of
//! a stick figure, while doing everything the stick figure does.
//!
//! Backgrounds:
//!  * GIFs with transparency are drawn as they are;
//!  * `"gif_key": "auto"` (or "#rrggbb") removes a plain background colour
//!    (taken from the corners for "auto"), e.g. a green screen;
//!  * anything else is shown as a "sticker": rounded corners, white border.

use crate::gfx::Gfx;
use crate::util::{packs_dir, Col, P};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// Frames are scaled down to at most this height when loaded (memory/CPU).
const MAX_H: u32 = 200;
/// Longer GIFs keep every n-th frame (memory: ~150 KB per frame).
const MAX_FRAMES: usize = 90;

pub struct Sprite {
    /// (frame, how long it shows in seconds)
    pub frames: Vec<(Pixmap, f32)>,
    pub total: f32,
    pub w: u32,
    pub h: u32,
    pub sticker: bool,
}

impl std::fmt::Debug for Sprite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sprite({}x{}, {} frames, sticker={})", self.w, self.h, self.frames.len(), self.sticker)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bg {
    /// keep pixels as they are (transparent GIF)
    Keep,
    /// remove this colour (with soft edges)
    Key(Col),
    /// key colour taken from the corners of the first frame
    AutoKey,
    /// rounded sticker with a white border
    Sticker,
}

impl Bg {
    pub fn parse(key: &str, sticker: bool) -> Bg {
        let k = key.trim().to_lowercase();
        if k == "auto" {
            Bg::AutoKey
        } else if let Some(c) = Col::hex(&k) {
            Bg::Key(c)
        } else if sticker {
            Bg::Sticker
        } else {
            Bg::Keep
        }
    }
}

pub fn resolve(file: &str) -> PathBuf {
    let p = PathBuf::from(file);
    if p.is_absolute() {
        p
    } else {
        packs_dir().join(p)
    }
}

type Cache = Mutex<HashMap<(PathBuf, String), (SystemTime, Arc<Sprite>)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load (or reuse) a GIF. None if it can't be read.
pub fn load(path: &Path, bg: Bg) -> Option<Arc<Sprite>> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let key = (path.to_path_buf(), format!("{:?}", bg));
    if let Ok(c) = cache().lock() {
        if let Some((t, s)) = c.get(&key) {
            if *t == mtime {
                return Some(s.clone());
            }
        }
    }
    let s = Arc::new(decode(path, bg)?);
    if let Ok(mut c) = cache().lock() {
        c.insert(key, (mtime, s.clone()));
    }
    Some(s)
}

/// Raw RGBA frames (straight alpha) + delays.
fn read_frames(path: &Path) -> Option<Vec<(image::RgbaImage, f32)>> {
    use image::AnimationDecoder;
    let f = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let dec = image::codecs::gif::GifDecoder::new(f).ok()?;
    let mut out = vec![];
    for fr in dec.into_frames().take(1000) {
        let fr = fr.ok()?;
        let (n, d) = fr.delay().numer_denom_ms();
        let ms = if d == 0 { 100.0 } else { n as f32 / d as f32 };
        // browsers treat tiny delays as 100 ms; so do we
        let secs = if ms < 20.0 { 0.1 } else { ms / 1000.0 };
        out.push((fr.into_buffer(), secs));
    }
    if out.len() > MAX_FRAMES {
        // keep every n-th frame; it shows for the dropped frames' time too
        let step = out.len().div_ceil(MAX_FRAMES);
        let mut thin = vec![];
        for chunk in out.chunks(step) {
            let secs: f32 = chunk.iter().map(|c| c.1).sum();
            thin.push((chunk[0].0.clone(), secs));
        }
        out = thin;
    }
    (!out.is_empty()).then_some(out)
}

fn corner_key(img: &image::RgbaImage) -> Option<Col> {
    let (w, h) = img.dimensions();
    if w < 4 || h < 4 {
        return None;
    }
    let px = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        Col::rgba(p[0], p[1], p[2], p[3])
    };
    let cs = [px(1, 1), px(w - 2, 1), px(1, h - 2), px(w - 2, h - 2)];
    let close = |a: Col, b: Col| dist(a, b) < 40.0;
    // at least three corners must agree
    cs.into_iter().find(|&c| cs.iter().filter(|o| close(c, **o)).count() >= 3)
}

fn dist(a: Col, b: Col) -> f32 {
    let d = |x: u8, y: u8| (x as f32 - y as f32).powi(2);
    (d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)).sqrt()
}

/// What background handling suits this GIF? (for the importer)
pub fn analyze(path: &Path) -> Option<(Bg, u32, u32, usize)> {
    let frames = read_frames(path)?;
    let first = &frames[0].0;
    let (w, h) = first.dimensions();
    let transparent = first.pixels().any(|p| p.0[3] < 128);
    let bg = if transparent {
        Bg::Keep
    } else if corner_key(first).is_some() && frames.iter().take(10).all(|f| corner_key(&f.0).is_some()) {
        Bg::AutoKey
    } else {
        Bg::Sticker
    };
    Some((bg, w, h, frames.len()))
}

fn decode(path: &Path, bg: Bg) -> Option<Sprite> {
    let frames = read_frames(path)?;
    let key = match bg {
        Bg::Key(c) => Some(c),
        Bg::AutoKey => corner_key(&frames[0].0),
        _ => None,
    };
    let (w0, h0) = frames[0].0.dimensions();
    let k = (MAX_H as f32 / h0 as f32).min(1.0);
    let (w, h) = (((w0 as f32 * k).round() as u32).max(1), ((h0 as f32 * k).round() as u32).max(1));
    let sticker = bg == Bg::Sticker;
    let radius = w.min(h) as f32 * 0.14;
    let mut out = vec![];
    let mut total = 0.0;
    for (img, secs) in frames {
        let img = if (w, h) != img.dimensions() { image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle) } else { img };
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for (x, y, p) in img.enumerate_pixels() {
            let [r, g, b, mut a] = p.0;
            if let Some(kc) = key {
                // soft chroma key
                let d = dist(Col::rgb(r, g, b), kc);
                let t = ((d - 45.0) / 40.0).clamp(0.0, 1.0);
                a = (a as f32 * t) as u8;
            }
            if sticker {
                a = (a as f32 * round_cover(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, radius)) as u8;
            }
            let m = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
            data.extend_from_slice(&[m(r), m(g), m(b), a]);
        }
        let pm = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)?;
        total += secs;
        out.push((pm, secs));
    }
    Some(Sprite { frames: out, total, w, h, sticker })
}

/// Coverage of a pixel by a rounded rectangle (anti-aliased corners).
fn round_cover(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let cx = x.clamp(r, w - r);
    let cy = y.clamp(r, h - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

impl Sprite {
    pub fn frame_at(&self, t: f32) -> &Pixmap {
        if self.frames.len() == 1 || self.total <= 0.0 {
            return &self.frames[0].0;
        }
        let mut u = t.rem_euclid(self.total);
        for (pm, d) in &self.frames {
            if u < *d {
                return pm;
            }
            u -= d;
        }
        &self.frames[0].0
    }

    /// On-screen size for a character this many pixels tall.
    pub fn size_for(&self, height: f32) -> (f32, f32) {
        let k = height / self.h as f32;
        (self.w as f32 * k, height)
    }
}

/// Draw `frame` with its bottom-centre at `foot`, `height` px tall, mirrored
/// if `flip`, rotated `deg` degrees around its centre.
pub fn draw(g: &mut Gfx, sp: &Sprite, frame: &Pixmap, foot: P, height: f32, flip: bool, deg: f32) {
    let (w, h) = sp.size_for(height);
    let k = height / sp.h as f32;
    let c = (foot.0 - g.ox, foot.1 - h / 2.0 - g.oy);
    let base = Transform::from_translate(c.0, c.1).pre_rotate(deg);
    if sp.sticker {
        // white border + soft shadow behind the sticker
        let pad = (height * 0.035).max(2.0);
        let r = w.min(h) * 0.14 + pad;
        g.round_rect_t(base, -w / 2.0 - pad + 2.0, -h / 2.0 - pad + 3.0, w + pad * 2.0, h + pad * 2.0, r, Col::rgba(0, 0, 0, 70));
        g.round_rect_t(base, -w / 2.0 - pad, -h / 2.0 - pad, w + pad * 2.0, h + pad * 2.0, r, Col::rgb(250, 250, 250));
    }
    let tf = base.pre_scale(if flip { -k } else { k }, k).pre_translate(-(sp.w as f32) / 2.0, -(sp.h as f32) / 2.0);
    let paint = PixmapPaint { opacity: g.alpha.clamp(0.0, 1.0), quality: FilterQuality::Bilinear, ..Default::default() };
    g.pm.draw_pixmap(0, 0, frame.as_ref(), &paint, tf, None);
}
