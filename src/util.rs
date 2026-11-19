//! Small shared helpers: RNG, colors, math, strings, paths.

use std::path::PathBuf;

pub type P = (f32, f32);

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
    pub fn idx(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.idx(v.len())]
    }
    pub fn sign(&mut self) -> f32 {
        if self.chance(0.5) {
            1.0
        } else {
            -1.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Col {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Col {
        Col { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Col {
        Col { r, g, b, a }
    }
    pub fn hex(s: &str) -> Option<Col> {
        let s = s.trim().trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Col::rgb(v(0)?, v(2)?, v(4)?)),
            8 => Some(Col::rgba(v(0)?, v(2)?, v(4)?, v(6)?)),
            3 => {
                let c = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|x| x * 17);
                Some(Col::rgb(c(0)?, c(1)?, c(2)?))
            }
            _ => None,
        }
    }
    pub fn alpha(self, a: f32) -> Col {
        Col { a: (self.a as f32 * a.clamp(0.0, 1.0)) as u8, ..self }
    }
    pub fn mix(self, o: Col, t: f32) -> Col {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        Col { r: m(self.r, o.r), g: m(self.g, o.g), b: m(self.b, o.b), a: m(self.a, o.a) }
    }
    pub fn luma(self) -> f32 {
        (0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32) / 255.0
    }
}

pub const WHITE: Col = Col::rgb(255, 255, 255);
pub const BLACK: Col = Col::rgb(14, 14, 18);
pub const RED: Col = Col::rgb(220, 30, 30);

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn lerp_p(a: P, b: P, t: f32) -> P {
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}
pub fn ease_io(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn dist(a: P, b: P) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}
pub fn rad(d: f32) -> f32 {
    d.to_radians()
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("RamRonin");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn packs_dir() -> PathBuf {
    let d = data_dir().join("packs");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// "chrome.exe" -> "Chrome" etc. Friendly names for the clouds.
pub fn pretty_name(exe: &str) -> String {
    let stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
    let known = [
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("ms-teams", "Teams"),
        ("teams", "Teams"),
        ("code", "VS Code"),
        ("explorer", "Explorer"),
        ("slack", "Slack"),
        ("discord", "Discord"),
        ("spotify", "Spotify"),
        ("outlook", "Outlook"),
        ("olk", "Outlook"),
        ("devenv", "Visual Studio"),
        ("idea64", "IntelliJ"),
        ("pycharm64", "PyCharm"),
        ("rider64", "Rider"),
        ("msedgewebview2", "WebView2"),
        ("vmmem", "WSL/VM"),
        ("vmmemwsl", "WSL"),
        ("winword", "Word"),
        ("excel", "Excel"),
        ("powerpnt", "PowerPoint"),
        ("onenote", "OneNote"),
        ("zoom", "Zoom"),
        ("steam", "Steam"),
        ("node", "Node"),
        ("python", "Python"),
        ("java", "Java"),
        ("javaw", "Java"),
        ("docker desktop", "Docker"),
        ("claude", "Claude"),
    ];
    let low = stem.to_lowercase();
    for (k, v) in known {
        if low == k {
            return v.to_string();
        }
    }
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => exe.to_string(),
    }
}

pub fn fmt_bytes(b: u64) -> String {
    let gb = b as f64 / (1024.0 * 1024.0 * 1024.0);
    if gb >= 1.0 {
        format!("{:.1} GB", gb)
    } else {
        format!("{} MB", b / (1024 * 1024))
    }
}
