//! Build script: generates the app icon (no binary assets in the repo), the
//! Windows manifest and the VERSIONINFO resource, then compiles them into the exe.
//! Proper version info + manifest + icon make the exe look like a normal,
//! well-behaved desktop app to Windows and to antivirus heuristics.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u32> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let ver_commas = format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3]);
    let ver_dots = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);

    let ico_path = out.join("app.ico");
    fs::write(&ico_path, make_ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();

    let manifest_path = out.join("app.manifest");
    fs::write(&manifest_path, MANIFEST).unwrap();

    let esc = |p: &PathBuf| p.display().to_string().replace('\\', "\\\\");
    let rc = format!(
        r#"1 ICON "{ico}"
1 24 "{man}"
1 VERSIONINFO
FILEVERSION {vc}
PRODUCTVERSION {vc}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "RamRonin Project"
      VALUE "FileDescription", "RamRonin - stickman ninja memory monitor"
      VALUE "FileVersion", "{vd}"
      VALUE "InternalName", "RamRonin"
      VALUE "LegalCopyright", "Open source. Free to use."
      VALUE "OriginalFilename", "RamRonin.exe"
      VALUE "ProductName", "RamRonin"
      VALUE "ProductVersion", "{vd}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = esc(&ico_path),
        man = esc(&manifest_path),
        vc = ver_commas,
        vd = ver_dots
    );
    let rc_path = out.join("app.rc");
    fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity type="win32" name="RamRonin.App" version="1.0.0.0" processorArchitecture="*"/>
  <description>RamRonin stickman ninja memory monitor</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </asmv3:windowsSettings>
  </asmv3:application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

// ---------------------------------------------------------------------------
// Tiny signed-distance-field painter for the icon: a black ninja head with a
// red headband + flowing tails and a blue spiral orb.
// ---------------------------------------------------------------------------

fn seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (vx, vy) = (bx - ax, by - ay);
    let (wx, wy) = (px - ax, py - ay);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (dx, dy) = (wx - vx * t, wy - vy * t);
    (dx * dx + dy * dy).sqrt()
}

fn blend(dst: &mut [f32; 4], c: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let out_a = a + dst[3] * (1.0 - a);
    for i in 0..3 {
        dst[i] = (c[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a.max(1e-6);
    }
    dst[3] = out_a;
}

fn pixel(x: f32, y: f32, px: f32) -> [f32; 4] {
    // x,y in 0..1 ; px = size of one pixel in these units (for anti-aliasing)
    let cov = |d: f32| (0.5 - d / px).clamp(0.0, 1.0);
    let mut c = [0.0f32; 4];
    // soft round backdrop
    let bd = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() - 0.47;
    blend(&mut c, [0.97, 0.95, 0.90], cov(bd));
    // orb glow
    let od = ((x - 0.70).powi(2) + (y - 0.68).powi(2)).sqrt();
    blend(&mut c, [0.35, 0.75, 1.0], cov(od - 0.20) * 0.35);
    blend(&mut c, [0.55, 0.85, 1.0], cov(od - 0.14));
    blend(&mut c, [0.92, 0.98, 1.0], cov(od - 0.06));
    // spiral on orb
    let ang = (y - 0.68).atan2(x - 0.70);
    let spiral = ((od * 60.0 - ang * 2.0).sin()).abs();
    if od < 0.13 {
        blend(&mut c, [1.0, 1.0, 1.0], (1.0 - spiral) * 0.5);
    }
    // headband tails
    let t1 = seg_dist(x, y, 0.22, 0.36, 0.06, 0.22).min(seg_dist(x, y, 0.06, 0.22, 0.02, 0.10));
    let t2 = seg_dist(x, y, 0.22, 0.38, 0.08, 0.36).min(seg_dist(x, y, 0.08, 0.36, 0.03, 0.30));
    blend(&mut c, [0.86, 0.10, 0.10], cov(t1 - 0.035));
    blend(&mut c, [0.86, 0.10, 0.10], cov(t2 - 0.03));
    // head
    let hd = ((x - 0.42).powi(2) + (y - 0.40).powi(2)).sqrt() - 0.24;
    blend(&mut c, [0.07, 0.07, 0.09], cov(hd));
    // headband across head
    if hd < 0.0 {
        let bd = (y - (0.34 + (x - 0.42) * 0.15)).abs() - 0.045;
        blend(&mut c, [0.86, 0.10, 0.10], cov(bd));
        // glowing red eye with pupil
        let ed = ((x - 0.53).powi(2) + (y - 0.45).powi(2)).sqrt();
        blend(&mut c, [0.95, 0.12, 0.12], cov(ed - 0.065));
        blend(&mut c, [0.05, 0.0, 0.0], cov(ed - 0.025));
    }
    c
}

fn render(size: u32) -> Vec<u8> {
    // BGRA, bottom-up rows, 4x4 supersampling on top of the analytic AA.
    let mut out = vec![0u8; (size * size * 4) as usize];
    let px = 1.0 / size as f32;
    for row in 0..size {
        for col in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..2 {
                for sx in 0..2 {
                    let x = (col as f32 + 0.25 + sx as f32 * 0.5) * px;
                    let y = (row as f32 + 0.25 + sy as f32 * 0.5) * px;
                    let p = pixel(x, y, px);
                    for i in 0..3 {
                        acc[i] += p[i] * p[3];
                    }
                    acc[3] += p[3];
                }
            }
            let a = acc[3] / 4.0;
            let (r, g, b) = if acc[3] > 0.0 {
                (acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3])
            } else {
                (0.0, 0.0, 0.0)
            };
            let dst_row = size - 1 - row;
            let i = ((dst_row * size + col) * 4) as usize;
            out[i] = (b * 255.0) as u8;
            out[i + 1] = (g * 255.0) as u8;
            out[i + 2] = (r * 255.0) as u8;
            out[i + 3] = (a * 255.0) as u8;
        }
    }
    out
}

fn make_ico(sizes: &[u32]) -> Vec<u8> {
    let mut images = Vec::new();
    for &s in sizes {
        let mut img = Vec::new();
        let mask_row = ((s + 31) / 32) * 4;
        let pixels = render(s);
        // BITMAPINFOHEADER
        img.extend_from_slice(&40u32.to_le_bytes());
        img.extend_from_slice(&(s as i32).to_le_bytes());
        img.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        img.extend_from_slice(&1u16.to_le_bytes());
        img.extend_from_slice(&32u16.to_le_bytes());
        img.extend_from_slice(&0u32.to_le_bytes());
        img.extend_from_slice(&((pixels.len() as u32) + mask_row * s).to_le_bytes());
        img.extend_from_slice(&[0u8; 16]);
        img.extend_from_slice(&pixels);
        img.extend(std::iter::repeat(0u8).take((mask_row * s) as usize));
        images.push((s, img));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, img) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for (_, img) in &images {
        out.extend_from_slice(img);
    }
    out
}
