//! Objective image critique shared by the `critique` MCP tool and `pt critique-bench`.

use pt_core::Geometry;
use pt_render::{Image, Opts};
use serde_json::{json, Value};

/// Brief-agnostic critique of a render: framing, centering, contrast, clipping, detail, plus (optionally)
/// structural similarity to a reference image. `score` is the fraction of rubric checks that pass.
pub fn critique_json(geo: &Geometry, o: &Opts, reference: Option<&Image>) -> Value {
    let (img, _) = pt_render::render(geo, o);
    let mask = pt_render::silhouette(geo, o);
    let coverage = mask.iter().filter(|b| **b).count() as f64 / mask.len().max(1) as f64;
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
    for (i, m) in mask.iter().enumerate() {
        if *m {
            sx += (i % o.width) as f64;
            sy += (i / o.width) as f64;
            n += 1.0;
        }
    }
    let off = if n > 0.0 { ((sx / n) / o.width as f64 - 0.5).abs().max(((sy / n) / o.height as f64 - 0.5).abs()) } else { 1.0 };
    let st = pt_render::metrics::analyze(&img);
    let check = |ok: bool, name: &str, value: f64, good: &str, advice: &str| json!({"check": name, "pass": ok, "value": value, "target": good, "advice": if ok { "" } else { advice }});
    let mut rubric = vec![
        check((0.12..=0.85).contains(&coverage), "framing", coverage, "subject covers 12-85% of the frame", "the subject is tiny or overflowing; check scale (meters) and stray far-away geometry"),
        check(off < 0.3, "centering", off, "subject centroid within 30% of frame center", "geometry is lopsided or has far outliers; check instances and bounds"),
        check(st.contrast > 18.0, "contrast", st.contrast, "luminance std > 18", "render looks flat: add height/shape variation or use render_final for lighting"),
        check(st.clipped_bright < 0.08 && st.clipped_dark < 0.5, "clipping", st.clipped_bright, "<8% blown highlights", "highlights are clipping: reduce exposure or lighting"),
        check(st.edge_density > 0.01, "detail", st.edge_density, "edge density > 1% of pixels", "very little visible detail: add secondary detail (noise, instances, bevels)"),
    ];
    let mut result = json!({"coverage": coverage, "center_offset": off, "mean_luma": st.mean_luma, "contrast": st.contrast, "clipped_bright": st.clipped_bright, "clipped_dark": st.clipped_dark, "colorfulness": st.colorfulness, "edge_density": st.edge_density, "saturation": st.mean_saturation});
    if let Some(reference) = reference {
        let c = pt_render::metrics::compare(&img, reference);
        result["vs_reference"] = json!({"ssim": c.ssim, "psnr": c.psnr, "mse": c.mse});
        rubric.push(check(c.ssim > 0.6, "reference_similarity", c.ssim, "SSIM > 0.6", "structure differs from the reference: compare silhouettes and proportions"));
    }
    let passed = rubric.iter().filter(|r| r["pass"] == true).count();
    result["rubric"] = json!(rubric);
    result["score"] = json!(passed as f64 / rubric.len() as f64);
    result
}
