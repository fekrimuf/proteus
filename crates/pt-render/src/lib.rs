//! Software preview renderer (no GPU): z-buffered triangle rasterizer with shaded, wireframe
//! and attribute-heatmap modes, multi-angle contact sheets and PNG output.
//! Large instance tables are subsampled by a fixed stride (deterministic) for speed.

pub mod metrics;
pub mod pathtrace;

use pt_core::math::{self, Mat34};
use pt_core::{Column, Geometry};
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub enum Mode {
    Shaded,
    Wire,
    /// Color by a point attribute: `Cd` (v3 = direct color), any f32 attr (ramp), or "height" (Y).
    Attr(String),
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub width: usize,
    pub height: usize,
    pub mode: Mode,
    pub azimuth_deg: f32,
    pub elevation_deg: f32,
    pub max_instances: usize,
    /// Orthographic top/front/side views for contact sheets when Some.
    pub fov_deg: f32,
    /// Frame the camera on these bounds instead of the geometry (to compare shapes).
    pub fit_bounds: Option<([f32; 3], [f32; 3])>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { width: 640, height: 480, mode: Mode::Shaded, azimuth_deg: 35.0, elevation_deg: 28.0, max_instances: 200_000, fov_deg: 40.0, fit_bounds: None }
    }
}

#[derive(Clone, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

pub struct RenderInfo {
    pub triangles_drawn: usize,
    pub instances_total: usize,
    pub instances_drawn: usize,
}

struct Tri {
    p: [[f32; 3]; 3],
    c: [[f32; 3]; 3],
    n: Option<[[f32; 3]; 3]>,
}

fn ramp(t: f32) -> [f32; 3] {
    // Blue -> cyan -> green -> yellow -> red.
    let t = t.clamp(0.0, 1.0) * 4.0;
    let (i, f) = (t.floor() as usize, t - t.floor());
    let stops = [[0.1, 0.15, 0.8], [0.1, 0.75, 0.9], [0.2, 0.8, 0.25], [0.95, 0.9, 0.2], [0.9, 0.15, 0.1]];
    let (a, b) = (stops[i.min(4)], stops[(i + 1).min(4)]);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

/// Per-vertex colors for `g`'s points according to `mode`.
fn point_colors(g: &Geometry, mode: &Mode, range: Option<(f32, f32)>) -> Vec<[f32; 3]> {
    let base = [0.72, 0.75, 0.80];
    let n = g.n_points();
    match mode {
        Mode::Attr(name) if name == "height" => {
            let (lo, hi) = range.unwrap_or((0.0, 1.0));
            let span = if hi > lo { hi - lo } else { 1.0 };
            g.p().iter().map(|p| ramp((p[1] - lo) / span)).collect()
        }
        Mode::Attr(name) => match g.point.get(name) {
            Some(Column::V3(v)) => v.to_vec(),
            Some(Column::F32(v)) => {
                let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |a, &x| (a.0.min(x), a.1.max(x)));
                let span = if hi > lo { hi - lo } else { 1.0 };
                v.iter().map(|&x| ramp((x - lo) / span)).collect()
            }
            _ => vec![base; n],
        },
        _ => vec![base; n],
    }
}

/// Triangles of a geometry's own (non-instanced) surface in its local space, with colors
/// resolved for `mode` and smooth normals when `N` exists.
fn bake(geo: &Geometry, mode: &Mode) -> Vec<Tri> {
    let range = geo.base_bounds().map(|b| (b.0[1], b.1[1]));
    let colors = point_colors(geo, mode, range);
    let p = geo.p();
    let normals = geo.point.get("N").and_then(Column::as_v3);
    geo.triangles()
        .into_iter()
        .map(|t| Tri {
            p: [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]],
            c: [colors[t[0] as usize], colors[t[1] as usize], colors[t[2] as usize]],
            n: normals.map(|nn| [nn[t[0] as usize], nn[t[1] as usize], nn[t[2] as usize]]),
        })
        .collect()
}

fn place(t: &Tri, m: &Mat34, tint: Option<[f32; 3]>) -> Tri {
    let rot = |n: [f32; 3]| [m[0] * n[0] + m[1] * n[1] + m[2] * n[2], m[4] * n[0] + m[5] * n[1] + m[6] * n[2], m[8] * n[0] + m[9] * n[1] + m[10] * n[2]];
    Tri {
        p: [math::xform_point(m, t.p[0]), math::xform_point(m, t.p[1]), math::xform_point(m, t.p[2])],
        c: tint.map_or(t.c, |c| [c; 3]),
        n: t.n.map(|n| [rot(n[0]), rot(n[1]), rot(n[2])]),
    }
}

const MAX_TRIS: usize = 12_000_000;

fn collect_tris(g: &Geometry, mode: &Mode, o: &Opts, tris: &mut Vec<Tri>, info: &mut RenderInfo) {
    collect_rec(g, None, None, mode, o, tris, info, 0);
}

/// Recursively expand (possibly nested) instances. Only the top level is subsampled by stride.
#[allow(clippy::too_many_arguments)]
fn collect_rec(g: &Geometry, xf: Option<Mat34>, tint: Option<[f32; 3]>, mode: &Mode, o: &Opts, tris: &mut Vec<Tri>, info: &mut RenderInfo, depth: usize) {
    if tris.len() >= MAX_TRIS || depth > 8 {
        return;
    }
    let ident = math::IDENTITY;
    let base = bake(g, mode);
    let m0 = xf.unwrap_or(ident);
    tris.extend(base.iter().map(|t| place(t, &m0, tint)));
    let Some(inst) = &g.instances else { return };
    if depth == 0 {
        info.instances_total += inst.len();
    }
    let stride = if depth == 0 { (inst.len() / o.max_instances.max(1)).max(1) } else { 1 };
    // Leaf prototypes (no instances of their own) are baked once and reused for every instance.
    let baked: Vec<Option<Vec<Tri>>> = inst.protos.iter().map(|p| if p.instances.is_none() { Some(bake(p, mode)) } else { None }).collect();
    let icol = match mode {
        Mode::Attr(n) => inst.attrs.get(n),
        _ => inst.attrs.get("Cd"),
    };
    for i in (0..inst.len()).step_by(stride) {
        if tris.len() >= MAX_TRIS {
            break;
        }
        let pid = inst.proto_id[i] as usize;
        let m = match xf {
            Some(x) => math::mul34(&x, &inst.xf[i]),
            None => inst.xf[i],
        };
        let t = match icol {
            Some(Column::V3(v)) => Some(v[i]),
            Some(Column::F32(v)) => {
                let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |a, &x| (a.0.min(x), a.1.max(x)));
                Some(ramp((v[i] - lo) / if hi > lo { hi - lo } else { 1.0 }))
            }
            _ => None,
        }
        .or(tint);
        match &baked[pid] {
            Some(b) => tris.extend(b.iter().map(|tr| place(tr, &m, t))),
            None => collect_rec(&inst.protos[pid], Some(m), t, mode, o, tris, info, depth + 1),
        }
        if depth == 0 {
            info.instances_drawn += 1;
        }
    }
}

struct Cam {
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    fwd: [f32; 3],
    f: f32, // focal length in NDC units
    aspect: f32,
}

fn make_cam(lo: [f32; 3], hi: [f32; 3], o: &Opts) -> Cam {
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let d = math::sub(hi, lo);
    let radius = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 0.5).max(1e-4);
    let (sa, ca) = math::sin_cos(math::to_radians(o.azimuth_deg));
    let (se, ce) = math::sin_cos(math::to_radians(o.elevation_deg));
    let dir = [ce * sa, se, ce * ca]; // from center toward eye
    let half = math::to_radians(o.fov_deg) * 0.5;
    let (sh, ch) = math::sin_cos(half);
    let dist = radius / sh * 1.05;
    let eye = [c[0] + dir[0] * dist, c[1] + dir[1] * dist, c[2] + dir[2] * dist];
    let fwd = [-dir[0], -dir[1], -dir[2]];
    let world_up = if dir[1].abs() > 0.999 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
    let right = math::normalize(math::cross(fwd, world_up));
    let up = math::cross(right, fwd);
    Cam { eye, right, up, fwd, f: ch / sh, aspect: o.width as f32 / o.height as f32 }
}

impl Cam {
    /// Returns (ndc_x, ndc_y, depth) or None if behind the camera.
    fn project(&self, p: [f32; 3]) -> Option<[f32; 3]> {
        let v = math::sub(p, self.eye);
        let z = v[0] * self.fwd[0] + v[1] * self.fwd[1] + v[2] * self.fwd[2];
        if z <= 1e-5 {
            return None;
        }
        let x = v[0] * self.right[0] + v[1] * self.right[1] + v[2] * self.right[2];
        let y = v[0] * self.up[0] + v[1] * self.up[1] + v[2] * self.up[2];
        Some([self.f * x / z / self.aspect, self.f * y / z, z])
    }
}

/// Screen-space triangle ready for rasterization.
struct Sc {
    a: [f32; 2],
    b: [f32; 2],
    c: [f32; 2],
    z: [f32; 3],
    col: [[f32; 3]; 3],
    nrm: Option<[[f32; 3]; 3]>,
    shade: f32,
    inv_area: f32,
    sqrt_area: f32,
    min_y: i32,
    max_y: i32,
    min_x: i32,
    max_x: i32,
}

pub fn render(g: &Geometry, o: &Opts) -> (Image, RenderInfo) {
    let (img, info, _, _) = render_full(g, o);
    (img, info)
}

/// Binary coverage mask (row-major, `o.width * o.height`). Pass `fit_bounds` so two shapes are
/// framed identically and their masks can be compared.
pub fn silhouette(g: &Geometry, o: &Opts) -> Vec<bool> {
    let (_, _, depth, ss) = render_full(g, o);
    let (w, h) = (o.width * ss, o.height * ss);
    let mut out = vec![false; o.width * o.height];
    for y in 0..o.height {
        for x in 0..o.width {
            let mut hit = 0;
            for sy in 0..ss {
                for sx in 0..ss {
                    if depth[(y * ss + sy) * w + x * ss + sx].is_finite() {
                        hit += 1;
                    }
                }
            }
            out[y * o.width + x] = hit * 2 >= ss * ss;
        }
    }
    let _ = h;
    out
}

/// Decode a PNG into RGB8.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return Err("unsupported PNG color type (use RGB, RGBA or grayscale 8-bit)".into()),
    };
    let mut rgb = Vec::with_capacity(w * h * 3);
    for px in buf[..info.buffer_size()].chunks(ch) {
        match ch {
            1 | 2 => rgb.extend_from_slice(&[px[0], px[0], px[0]]),
            _ => rgb.extend_from_slice(&px[..3]),
        }
    }
    Ok(Image { w, h, rgb })
}

pub fn render_full(g: &Geometry, o: &Opts) -> (Image, RenderInfo, Vec<f32>, usize) {
    let mut info = RenderInfo { triangles_drawn: 0, instances_total: 0, instances_drawn: 0 };
    let mut tris = Vec::new();
    collect_tris(g, &o.mode, o, &mut tris, &mut info);
    // Heavy scenes drop supersampling to stay interactive.
    let ss = if tris.len() > 1_500_000 { 1usize } else { 2 };
    let (w, h) = (o.width * ss, o.height * ss);
    let (lo, hi) = o.fit_bounds.or_else(|| bounds_of(&tris)).unwrap_or(([-1.0; 3], [1.0; 3]));
    let mut cam = make_cam(lo, hi, o);

    // Tighten framing: scale the camera distance so the projected extent fills ~92% of view.
    let step = (tris.len() / 20_000).max(1);
    let mut m = 0.0f32;
    for t in tris.iter().step_by(step) {
        for p in &t.p {
            if let Some(n) = cam.project(*p) {
                m = m.max(n[0].abs()).max(n[1].abs());
            }
        }
    }
    if m > 0.0 && o.fit_bounds.is_none() {
        let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let k = (m / 0.92).clamp(0.4, 1.5);
        for a in 0..3 {
            cam.eye[a] = c[a] + (cam.eye[a] - c[a]) * k;
        }
    }

    let light = math::normalize([-0.4, 0.8, 0.45]);
    let fwd = cam.fwd;
    let (wf, hf) = (w as f32, h as f32);
    let sc: Vec<Sc> = tris
        .par_iter()
        .filter_map(|t| {
            let (a, b, c) = (cam.project(t.p[0])?, cam.project(t.p[1])?, cam.project(t.p[2])?);
            let px = |n: [f32; 3]| [(n[0] * 0.5 + 0.5) * wf, (0.5 - n[1] * 0.5) * hf];
            let (pa, pb, pc) = (px(a), px(b), px(c));
            let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
            if area.abs() < 1e-9 {
                return None;
            }
            let (min_x, max_x) = (pa[0].min(pb[0]).min(pc[0]), pa[0].max(pb[0]).max(pc[0]));
            let (min_y, max_y) = (pa[1].min(pb[1]).min(pc[1]), pa[1].max(pb[1]).max(pc[1]));
            if max_x < 0.0 || max_y < 0.0 || min_x > wf || min_y > hf {
                return None;
            }
            let fnorm = math::normalize(math::cross(math::sub(t.p[1], t.p[0]), math::sub(t.p[2], t.p[0])));
            let ndl = (fnorm[0] * light[0] + fnorm[1] * light[1] + fnorm[2] * light[2]).abs();
            let nde = (fnorm[0] * -cam.fwd[0] + fnorm[1] * -cam.fwd[1] + fnorm[2] * -cam.fwd[2]).abs();
            Some(Sc {
                a: pa, b: pb, c: pc,
                z: [a[2], b[2], c[2]],
                col: t.c,
                nrm: t.n,
                shade: 0.22 + 0.58 * ndl + 0.2 * nde,
                inv_area: 1.0 / area,
                sqrt_area: area.abs().sqrt().max(1.0),
                min_x: min_x.floor().max(0.0) as i32,
                max_x: max_x.ceil().min(wf - 1.0) as i32,
                min_y: min_y.floor().max(0.0) as i32,
                max_y: max_y.ceil().min(hf - 1.0) as i32,
            })
        })
        .collect();
    info.triangles_drawn = sc.len();
    let wire = matches!(o.mode, Mode::Wire);

    // Rasterize in horizontal bands in parallel; each band owns its rows (no data races).
    let band = 16usize;
    let mut col = vec![[0.0f32; 3]; w * h];
    let mut depth = vec![f32::INFINITY; w * h];
    col.par_chunks_mut(w * band).zip(depth.par_chunks_mut(w * band)).enumerate().for_each(|(bi, (cb, db))| {
        let y0 = bi * band;
        let rows = cb.len() / w;
        for r in 0..rows {
            let t = (y0 + r) as f32 / hf;
            cb[r * w..(r + 1) * w].fill([0.16 - 0.08 * t, 0.18 - 0.08 * t, 0.24 - 0.08 * t]);
        }
        let (by0, by1) = (y0 as i32, (y0 + rows) as i32 - 1);
        for t in &sc {
            if t.max_y < by0 || t.min_y > by1 {
                continue;
            }
            for y in t.min_y.max(by0)..=t.max_y.min(by1) {
                let py = y as f32 + 0.5;
                let row = (y as usize - y0) * w;
                for x in t.min_x..=t.max_x {
                    let px = x as f32 + 0.5;
                    let w0 = ((t.b[0] - px) * (t.c[1] - py) - (t.b[1] - py) * (t.c[0] - px)) * t.inv_area;
                    let w1 = ((t.c[0] - px) * (t.a[1] - py) - (t.c[1] - py) * (t.a[0] - px)) * t.inv_area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * t.z[0] + w1 * t.z[1] + w2 * t.z[2];
                    let idx = row + x as usize;
                    if z < db[idx] {
                        db[idx] = z;
                        let shade = match &t.nrm {
                            Some(n) => {
                                let mut v = [0.0f32; 3];
                                for k in 0..3 {
                                    v[k] = w0 * n[0][k] + w1 * n[1][k] + w2 * n[2][k];
                                }
                                let v = math::normalize(v);
                                let ndl = (v[0] * light[0] + v[1] * light[1] + v[2] * light[2]).abs();
                                let nde = (v[0] * -fwd[0] + v[1] * -fwd[1] + v[2] * -fwd[2]).abs();
                                0.22 + 0.58 * ndl + 0.2 * nde
                            }
                            None => t.shade,
                        };
                        let mut rgb = [0.0; 3];
                        for k in 0..3 {
                            rgb[k] = (w0 * t.col[0][k] + w1 * t.col[1][k] + w2 * t.col[2][k]) * shade;
                        }
                        if wire {
                            let e = w0.min(w1).min(w2);
                            rgb = if e < 1.6 / t.sqrt_area { [0.95, 0.95, 0.98] } else { [rgb[0] * 0.25, rgb[1] * 0.25, rgb[2] * 0.25] };
                        }
                        cb[idx] = rgb;
                    }
                }
            }
        }
    });

    // Downsample (box filter) + gamma.
    let mut out = vec![0u8; o.width * o.height * 3];
    for y in 0..o.height {
        for x in 0..o.width {
            let mut acc = [0.0f32; 3];
            for sy in 0..ss {
                for sx in 0..ss {
                    let c = col[(y * ss + sy) * w + x * ss + sx];
                    for k in 0..3 {
                        acc[k] += c[k];
                    }
                }
            }
            for k in 0..3 {
                let v = (acc[k] / (ss * ss) as f32).clamp(0.0, 1.0);
                out[(y * o.width + x) * 3 + k] = (gamma(v) * 255.0 + 0.5) as u8;
            }
        }
    }
    (Image { w: o.width, h: o.height, rgb: out }, info, depth, ss)
}

/// Approximate sRGB encode without powf (determinism lint): sqrt blend.
fn gamma(v: f32) -> f32 {
    let s = v.sqrt();
    v * 0.35 + s * 0.65
}

fn bounds_of(tris: &[Tri]) -> Option<([f32; 3], [f32; 3])> {
    let first = tris.first()?.p[0];
    let (mut lo, mut hi) = (first, first);
    for t in tris {
        for p in &t.p {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    Some((lo, hi))
}

/// 2x2 sheet: 3/4 view, front, side, top.
pub fn contact_sheet(g: &Geometry, o: &Opts) -> (Image, RenderInfo) {
    let views = [(35.0, 28.0), (0.0, 0.0), (90.0, 0.0), (0.0, 89.0)];
    let (hw, hh) = (o.width / 2, o.height / 2);
    let mut sheet = Image { w: hw * 2, h: hh * 2, rgb: vec![0; hw * 2 * hh * 2 * 3] };
    let mut info = RenderInfo { triangles_drawn: 0, instances_total: 0, instances_drawn: 0 };
    for (i, (az, el)) in views.iter().enumerate() {
        let mut vo = o.clone();
        vo.width = hw;
        vo.height = hh;
        vo.azimuth_deg = *az;
        vo.elevation_deg = *el;
        let (img, inf) = render(g, &vo);
        info = inf;
        let (ox, oy) = ((i % 2) * hw, (i / 2) * hh);
        for y in 0..hh {
            let src = &img.rgb[y * hw * 3..(y + 1) * hw * 3];
            let dst = ((oy + y) * sheet.w + ox) * 3;
            sheet.rgb[dst..dst + hw * 3].copy_from_slice(src);
        }
    }
    (sheet, info)
}

pub fn encode_png(img: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w as u32, img.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&img.rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
