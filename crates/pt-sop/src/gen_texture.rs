//! Textures made by the program itself, so a world does not have to lean on scanned ones. First: a conifer needle SPRAY, an RGBA
//! picture of a twig with its needles on a transparent background, for the cards of a procedural tree (`tree_conifer`).
//!
//! The picture is drawn, not photographed: a curved stem from the bottom edge, side shoots, and needles as tapered lines with
//! colour that runs from dark at the stem to light at the tip. Deterministic for a seed; the transparent pixels carry the average
//! needle colour so texture filtering does not draw dark fringes.

use pt_core::math::{self, sq};
use pt_core::rng::rand4;
use std::path::Path;

/// An RGBA picture, row 0 at the top, 4 bytes per pixel.
pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<[f32; 3]>,
    a: Vec<f32>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, rgb: vec![[0.0; 3]; w * h], a: vec![0.0; w * h] }
    }

    /// A tapered line from `p0` (half width `w0`) to `p1` (half width `w1`), pixel units, painted over what is there.
    fn line(&mut self, p0: [f32; 2], p1: [f32; 2], w0: f32, w1: f32, c0: [f32; 3], c1: [f32; 3]) {
        let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let pad = w0.max(w1) + 1.5;
        let (x0, x1) = ((p0[0].min(p1[0]) - pad).floor().max(0.0) as usize, ((p0[0].max(p1[0]) + pad).ceil() as usize).min(self.w - 1));
        let (y0, y1) = ((p0[1].min(p1[1]) - pad).floor().max(0.0) as usize, ((p0[1].max(p1[1]) + pad).ceil() as usize).min(self.h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - p0[0]) * dx + (py - p0[1]) * dy) / len2).clamp(0.0, 1.0);
                let (cx, cy) = (p0[0] + dx * t, p0[1] + dy * t);
                let d = (sq(px - cx) + sq(py - cy)).sqrt();
                let hw = w0 + (w1 - w0) * t;
                let cover = (hw + 0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let i = y * self.w + x;
                let col = [c0[0] + (c1[0] - c0[0]) * t, c0[1] + (c1[1] - c0[1]) * t, c0[2] + (c1[2] - c0[2]) * t];
                // over: the new stroke on top of what is there
                let a_new = cover;
                let a_out = a_new + self.a[i] * (1.0 - a_new);
                for k in 0..3 {
                    self.rgb[i][k] = (col[k] * a_new + self.rgb[i][k] * self.a[i] * (1.0 - a_new)) / a_out.max(1e-6);
                }
                self.a[i] = a_out;
            }
        }
    }

    fn finish(self) -> Rgba {
        // the colour under transparent pixels is the average of the painted ones
        let (mut sum, mut n) = ([0.0f32; 3], 0.0f32);
        for (c, a) in self.rgb.iter().zip(&self.a) {
            if *a > 0.5 {
                for k in 0..3 {
                    sum[k] += c[k];
                }
                n += 1.0;
            }
        }
        let avg = if n > 0.0 { [sum[0] / n, sum[1] / n, sum[2] / n] } else { [0.1, 0.2, 0.08] };
        let mut px = Vec::with_capacity(self.w * self.h * 4);
        for (c, a) in self.rgb.iter().zip(&self.a) {
            let col = if *a > 0.02 { *c } else { avg };
            // linear to sRGB-ish encoding for the file (the renderer reads it as sRGB colour)
            for k in 0..3 {
                px.push((math::pow(col[k].clamp(0.0, 1.0), 1.0 / 2.2) * 255.0 + 0.5) as u8);
            }
            px.push((a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
        Rgba { w: self.w, h: self.h, px }
    }
}

/// Needles: tapered lines of linear-light green. `scale` is the size of the picture in pixels per metre of a 1 m spray.
fn r(seed: i64, stream: u32, i: u64) -> f32 {
    rand4(seed, stream, i)[0]
}

/// A spray of conifer needles: one curved stem rising from the bottom centre, side shoots to both sides, needles on all of them.
pub fn needle_spray(size: usize, seed: i64) -> Rgba {
    let mut cv = Canvas::new(size, size);
    let s = size as f32;
    let dark = [0.012, 0.045, 0.010];
    let mid = [0.030, 0.105, 0.022];
    let light = [0.085, 0.190, 0.045];
    let stem_a = [0.050, 0.030, 0.015];
    let stem_b = [0.040, 0.070, 0.020];
    // the main stem: a gentle curve from the bottom edge to near the top
    let sway = (r(seed, 1, 0) - 0.5) * 0.08 * s;
    let stem = |t: f32| -> [f32; 2] { [0.5 * s + sway * math::sin(t * 3.0) * t, s * (0.985 - 0.95 * t)] };
    let mut idx = 10u64;
    let mut needle = |cv: &mut Canvas, base: [f32; 2], ang: f32, len: f32, shade: f32| {
        let k = idx;
        idx += 1;
        let a = ang + (r(seed, 2, k) - 0.5) * 0.25;
        let l = len * (0.8 + 0.4 * r(seed, 3, k));
        let tip = [base[0] + math::cos(a) * l, base[1] - math::sin(a) * l];
        let tone = (shade + 0.35 * (r(seed, 4, k) - 0.5)).clamp(0.0, 1.0);
        let c0 = if tone < 0.5 { [dark[0] + (mid[0] - dark[0]) * tone * 2.0, dark[1] + (mid[1] - dark[1]) * tone * 2.0, dark[2] + (mid[2] - dark[2]) * tone * 2.0] } else { let u = (tone - 0.5) * 2.0; [mid[0] + (light[0] - mid[0]) * u, mid[1] + (light[1] - mid[1]) * u, mid[2] + (light[2] - mid[2]) * u] };
        let c1 = [c0[0] * 1.25, c0[1] * 1.3, c0[2] * 1.15];
        cv.line(base, tip, 0.012 * s, 0.002 * s, c0, c1);
    };
    // side shoots first (they lie behind), then the stem's own needles, then the stem on top
    let shoots = 11;
    for j in 0..shoots {
        let t = 0.10 + 0.84 * j as f32 / shoots as f32;
        for side in [-1.0f32, 1.0] {
            let base = stem(t);
            let reach = (0.46 * (1.0 - t * 0.92)).max(0.06) * s * (0.8 + 0.4 * r(seed, 5, (j * 2) as u64 + (side > 0.0) as u64));
            let up = 0.75 + 0.35 * r(seed, 6, j as u64);
            let tip = [base[0] + side * reach, base[1] - reach * up * 0.62];
            cv.line(base, tip, 0.006 * s, 0.002 * s, stem_a, stem_b);
            // needles along the shoot, fanning forward
            let n = ((reach / s) * 120.0).max(8.0) as usize;
            for q in 0..n {
                let u = 0.12 + 0.9 * q as f32 / n as f32;
                let b = [base[0] + (tip[0] - base[0]) * u.min(1.0), base[1] + (tip[1] - base[1]) * u.min(1.0)];
                let shoot_ang = math::atan2(-(tip[1] - base[1]), tip[0] - base[0]);
                for turn in [-1.0f32, 1.0] {
                    needle(&mut cv, b, shoot_ang + turn * (0.9 + 0.35 * r(seed, 7, q as u64)), 0.052 * s * (1.0 - 0.35 * u), 0.25 + 0.7 * u);
                }
            }
        }
    }
    // needles on the main stem
    let n = 100;
    for q in 0..n {
        let t = 0.04 + 0.95 * q as f32 / n as f32;
        let b = stem(t);
        for turn in [-1.0f32, 1.0] {
            needle(&mut cv, b, std::f32::consts::FRAC_PI_2 + turn * (0.85 + 0.4 * r(seed, 8, q as u64)), 0.05 * s * (1.0 - 0.5 * t), 0.2 + 0.75 * t);
        }
    }
    let mut prev = stem(0.0);
    for q in 1..=24 {
        let t = q as f32 / 24.0;
        let p = stem(t);
        cv.line(prev, p, 0.008 * s * (1.0 - 0.7 * t) + 0.002 * s, 0.006 * s * (1.0 - 0.7 * t) + 0.002 * s, stem_a, stem_b);
        prev = p;
    }
    cv.finish()
}

pub fn write_png(path: &Path, img: &Rgba) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.w as u32, img.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().map_err(|e| e.to_string())?.write_image_data(&img.px).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spray_is_deterministic_and_mostly_empty_with_a_solid_core() {
        let a = needle_spray(256, 3);
        let b = needle_spray(256, 3);
        assert_eq!(a.px, b.px);
        assert_ne!(a.px, needle_spray(256, 4).px, "another seed, another spray");
        let opaque = a.px.chunks(4).filter(|p| p[3] > 200).count() as f32 / (256.0 * 256.0);
        let clear = a.px.chunks(4).filter(|p| p[3] < 8).count() as f32 / (256.0 * 256.0);
        assert!((0.08..0.6).contains(&opaque), "opaque share {opaque}");
        assert!(clear > 0.3, "most of a twig's picture is empty, got {clear}");
        // the bottom centre holds the stem
        let mid_bottom = &a.px[((256 - 3) * 256 + 128) * 4..((256 - 3) * 256 + 128) * 4 + 4];
        assert!(mid_bottom[3] > 100, "the stem leaves the bottom edge: {mid_bottom:?}");
    }
}
