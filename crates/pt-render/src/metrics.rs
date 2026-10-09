//! Image metrics for critique loops: objective numbers an agent (or a VLM judge prompt) can
//! use next to its own eyes. Deterministic; no external dependencies.

use crate::Image;

#[derive(Clone, Debug)]
pub struct Stats {
    pub mean_luma: f64,
    /// Standard deviation of luminance (0-255): global contrast.
    pub contrast: f64,
    /// Fraction of pixels at (near) black / white.
    pub clipped_dark: f64,
    pub clipped_bright: f64,
    /// Hasler-Suesstrunk colorfulness.
    pub colorfulness: f64,
    /// Fraction of pixels whose Sobel gradient exceeds a threshold: a proxy for detail.
    pub edge_density: f64,
    pub mean_saturation: f64,
}

fn luma(p: &[u8]) -> f64 {
    0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
}

pub fn analyze(img: &Image) -> Stats {
    let n = (img.w * img.h).max(1) as f64;
    let l: Vec<f64> = img.rgb.chunks(3).map(luma).collect();
    let mean = l.iter().sum::<f64>() / n;
    let var = l.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    let dark = l.iter().filter(|x| **x < 8.0).count() as f64 / n;
    let bright = l.iter().filter(|x| **x > 247.0).count() as f64 / n;
    let (mut rg_s, mut rg_ss, mut yb_s, mut yb_ss) = (0.0, 0.0, 0.0, 0.0);
    let mut sat = 0.0;
    for p in img.rgb.chunks(3) {
        let (r, g, b) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let (rg, yb) = (r - g, 0.5 * (r + g) - b);
        rg_s += rg;
        rg_ss += rg * rg;
        yb_s += yb;
        yb_ss += yb * yb;
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        sat += if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
    }
    let (rg_sd, yb_sd) = ((rg_ss / n - (rg_s / n).powi(2)).max(0.0).sqrt(), (yb_ss / n - (yb_s / n).powi(2)).max(0.0).sqrt());
    let colorfulness = (rg_sd * rg_sd + yb_sd * yb_sd).sqrt() + 0.3 * ((rg_s / n).powi(2) + (yb_s / n).powi(2)).sqrt();
    let mut edges = 0usize;
    if img.w > 2 && img.h > 2 {
        for y in 1..img.h - 1 {
            for x in 1..img.w - 1 {
                let at = |dx: i32, dy: i32| l[((y as i32 + dy) as usize) * img.w + (x as i32 + dx) as usize];
                let gx = -at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1) + at(1, -1) + 2.0 * at(1, 0) + at(1, 1);
                let gy = -at(-1, -1) - 2.0 * at(0, -1) - at(1, -1) + at(-1, 1) + 2.0 * at(0, 1) + at(1, 1);
                if (gx * gx + gy * gy).sqrt() > 40.0 {
                    edges += 1;
                }
            }
        }
    }
    Stats { mean_luma: mean, contrast: var.sqrt(), clipped_dark: dark, clipped_bright: bright, colorfulness, edge_density: edges as f64 / n, mean_saturation: sat / n }
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub mse: f64,
    pub psnr: f64,
    /// Mean SSIM over 8x8 luminance windows (1 = identical).
    pub ssim: f64,
}

/// Nearest-neighbour resize so a reference of another size can be compared.
pub fn resize(img: &Image, w: usize, h: usize) -> Image {
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = ((x * img.w / w).min(img.w - 1), (y * img.h / h).min(img.h - 1));
            let i = (sy * img.w + sx) * 3;
            rgb.extend_from_slice(&img.rgb[i..i + 3]);
        }
    }
    Image { w, h, rgb }
}

pub fn compare(a: &Image, b: &Image) -> Comparison {
    let b = if a.w != b.w || a.h != b.h { resize(b, a.w, a.h) } else { b.clone() };
    let n = (a.w * a.h * 3).max(1) as f64;
    let mse = a.rgb.iter().zip(&b.rgb).map(|(x, y)| (*x as f64 - *y as f64).powi(2)).sum::<f64>() / n;
    let psnr = if mse <= 1e-12 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10_det() };
    let (la, lb): (Vec<f64>, Vec<f64>) = (a.rgb.chunks(3).map(luma).collect(), b.rgb.chunks(3).map(luma).collect());
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let (mut total, mut count) = (0.0, 0usize);
    let win = 8usize;
    let mut y = 0;
    while y + win <= a.h {
        let mut x = 0;
        while x + win <= a.w {
            let (mut ma, mut mb) = (0.0, 0.0);
            for j in 0..win {
                for i in 0..win {
                    ma += la[(y + j) * a.w + x + i];
                    mb += lb[(y + j) * a.w + x + i];
                }
            }
            let m = (win * win) as f64;
            let (ma, mb) = (ma / m, mb / m);
            let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
            for j in 0..win {
                for i in 0..win {
                    let (da, db) = (la[(y + j) * a.w + x + i] - ma, lb[(y + j) * a.w + x + i] - mb);
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            let (va, vb, cov) = (va / (m - 1.0), vb / (m - 1.0), cov / (m - 1.0));
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            count += 1;
            x += win;
        }
        y += win;
    }
    Comparison { mse, psnr, ssim: if count == 0 { 1.0 } else { total / count as f64 } }
}

trait Log10Det {
    fn log10_det(self) -> f64;
}
impl Log10Det for f64 {
    fn log10_det(self) -> f64 {
        pt_core::math::ln(self as f32) as f64 / std::f64::consts::LN_10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(v: u8) -> Image {
        Image { w: 32, h: 32, rgb: vec![v; 32 * 32 * 3] }
    }

    fn gradient() -> Image {
        let mut rgb = Vec::new();
        for y in 0..32 {
            for x in 0..32 {
                rgb.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, 128]);
            }
        }
        Image { w: 32, h: 32, rgb }
    }

    #[test]
    fn identical_images_compare_perfectly() {
        let g = gradient();
        let c = compare(&g, &g);
        assert!(c.mse == 0.0 && c.ssim > 0.999 && c.psnr >= 99.0);
        let d = compare(&g, &flat(128));
        assert!(d.ssim < 0.9 && d.mse > 100.0 && d.psnr < 30.0, "{d:?}");
    }

    #[test]
    fn stats_detect_contrast_clipping_and_detail() {
        let s = analyze(&flat(255));
        assert!(s.clipped_bright > 0.99 && s.contrast < 1e-6 && s.edge_density == 0.0);
        let mut checker = Image { w: 32, h: 32, rgb: vec![0; 32 * 32 * 3] };
        for y in 0..32 {
            for x in 0..32 {
                let v = if (x / 4 + y / 4) % 2 == 0 { 220 } else { 30 };
                let i = (y * 32 + x) * 3;
                checker.rgb[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
        let c = analyze(&checker);
        assert!(c.contrast > 80.0 && c.edge_density > 0.1, "{c:?}");
        assert!(analyze(&gradient()).colorfulness > 20.0);
    }

    #[test]
    fn resize_lets_references_of_other_sizes_compare() {
        let g = gradient();
        let small = resize(&g, 16, 16);
        let c = compare(&g, &small);
        assert!(c.ssim > 0.5, "{c:?}");
    }
}
