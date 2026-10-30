//! Export screenshots to a PDF, one image per page.
//!
//! This is a deliberately small PDF 1.4 writer rather than a dependency: an
//! A4 page per capture needs a page tree, an image XObject per page and a
//! content stream that scales it to fit — no fonts, vectors or annotations.
//! Image samples are stored as `FlateDecode` RGB (lossless), and transparency
//! is composited over white because PDF images have no alpha here.
//!
//! Long captures (a stitched page, say) are split across several pages so the
//! text does not shrink to nothing on one tall sheet.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::{Rgba, RgbaImage};

/// A4 in PostScript points (72 per inch).
const A4_W: f32 = 595.276;
const A4_H: f32 = 841.890;
/// White border around the image on a page.
const MARGIN: f32 = 18.0;

/// One drawn page: a source rectangle from `image` and where it lands.
struct PagePlan {
    image: usize,
    /// Crop inside the source image, in pixels: (x, y, w, h).
    src: (u32, u32, u32, u32),
    /// Page size in points.
    pw: f32,
    ph: f32,
    /// Placement rectangle in PDF user space (origin bottom-left).
    x: f32,
    y: f32,
    dw: f32,
    dh: f32,
}

/// Write `images` to `path`, one page per image (long ones spill over).
pub fn export(images: &[RgbaImage], path: &Path) -> Result<()> {
    let plans = plan_pages(images);
    if plans.is_empty() {
        anyhow::bail!("nothing to export");
    }
    let bytes = build_pdf(images, &plans)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Load image files and export them in the given order.
pub fn export_paths(paths: &[PathBuf], out: &Path) -> Result<()> {
    let mut images = Vec::with_capacity(paths.len());
    for path in paths {
        let img = image::open(path)
            .with_context(|| format!("reading {}", path.display()))?
            .to_rgba8();
        images.push(img);
    }
    export(&images, out)
}

/// Decide the page layout for every image. Tall images become several pages,
/// each showing a horizontal band at full width.
fn plan_pages(images: &[RgbaImage]) -> Vec<PagePlan> {
    let mut plans = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let (iw, ih) = (image.width().max(1), image.height().max(1));
        // Landscape sheets for wide captures, portrait otherwise.
        let (pw, ph) = if iw as f32 / ih as f32 >= 1.2 {
            (A4_H, A4_W)
        } else {
            (A4_W, A4_H)
        };
        let inner_w = pw - MARGIN * 2.0;
        let inner_h = ph - MARGIN * 2.0;
        let scale = inner_w / iw as f32;
        let total_h = ih as f32 * scale;

        if total_h <= inner_h {
            // One page, centred.
            let dw = inner_w;
            let dh = total_h;
            plans.push(PagePlan {
                image: index,
                src: (0, 0, iw, ih),
                pw,
                ph,
                x: (pw - dw) / 2.0,
                y: (ph - dh) / 2.0,
                dw,
                dh,
            });
            continue;
        }

        // Split into bands. Each band is `inner_h / scale` source pixels tall.
        let band_px = (inner_h / scale).max(1.0);
        let count = (ih as f32 / band_px).ceil() as u32;
        for b in 0..count {
            let y0 = (b as f32 * band_px).round() as u32;
            let y1 = (((b + 1) as f32 * band_px).round() as u32).min(ih);
            if y1 <= y0 {
                continue;
            }
            let bh = y1 - y0;
            let dw = inner_w;
            let dh = bh as f32 * scale;
            plans.push(PagePlan {
                image: index,
                src: (0, y0, iw, bh),
                pw,
                ph,
                x: (pw - dw) / 2.0,
                // Top-aligned so a partial last band sits at the top.
                y: ph - MARGIN - dh,
                dw,
                dh,
            });
        }
    }
    plans
}

/// Serialise the whole document into bytes.
fn build_pdf(images: &[RgbaImage], plans: &[PagePlan]) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    // High-bit comment marks the file as binary for transfer tools.
    out.extend_from_slice(&[0x25, 0xE2, 0xE3, 0xCF, 0xD3, 0x0A]);

    let n = plans.len();
    // Object ids: 1 catalog, 2 pages, then three per page.
    let page_id = |i: usize| 3 + i * 3;
    let content_id = |i: usize| 4 + i * 3;
    let image_id = |i: usize| 5 + i * 3;
    let total = 2 + n * 3;

    let mut offsets: Vec<usize> = vec![0; total + 1]; // 1-based ids

    // 1: catalog.
    offsets[1] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // 2: page tree.
    offsets[2] = out.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_id(i))).collect();
    let pages = format!(
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {n} >>\nendobj\n",
        kids.join(" ")
    );
    out.extend_from_slice(pages.as_bytes());

    for (i, plan) in plans.iter().enumerate() {
        let source = &images[plan.image];
        let (sx, sy, sw, sh) = plan.src;
        let crop = image::imageops::crop_imm(source, sx, sy, sw, sh).to_image();
        let rgb = to_rgb_over_white(&crop);
        let compressed = deflate(&rgb)?;

        // Page object.
        offsets[page_id(i)] = out.len();
        let page = format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.3} {ph:.3}] \
             /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>\nendobj\n",
            page_id(i),
            image_id(i),
            content_id(i),
            pw = plan.pw,
            ph = plan.ph,
        );
        out.extend_from_slice(page.as_bytes());

        // Content stream: "cm" scales the unit square to the placement rect.
        let content = format!(
            "q {dw:.3} 0 0 {dh:.3} {x:.3} {y:.3} cm /Im0 Do Q",
            dw = plan.dw,
            dh = plan.dh,
            x = plan.x,
            y = plan.y,
        );
        offsets[content_id(i)] = out.len();
        let cobj = format!(
            "{} 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n",
            content_id(i),
            content.len(),
            content
        );
        out.extend_from_slice(cobj.as_bytes());

        // Image XObject (lossless RGB, one byte per sample).
        offsets[image_id(i)] = out.len();
        let iobj = format!(
            "{} 0 obj\n<< /Type /XObject /Subtype /Image /Width {sw} /Height {sh} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
            image_id(i),
            compressed.len()
        );
        out.extend_from_slice(iobj.as_bytes());
        out.extend_from_slice(&compressed);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    // Cross-reference table. Every entry is exactly 20 bytes.
    let xref_at = out.len();
    let size = total + 1;
    let mut xref = format!("xref\n0 {size}\n");
    xref.push_str("0000000000 65535 f \n");
    for id in 1..=total {
        xref.push_str(&format!("{:010} 00000 n \n", offsets[id]));
    }
    out.extend_from_slice(xref.as_bytes());

    let trailer = format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
    );
    out.extend_from_slice(trailer.as_bytes());
    Ok(out)
}

/// Flatten RGBA to RGB, blending transparent pixels over white (PDF has no
/// alpha in a `DeviceRGB` image).
fn to_rgb_over_white(image: &RgbaImage) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(image.pixels().len() * 3);
    for Rgba(p) in image.pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
        rgb.push(blend(p[0]));
        rgb.push(blend(p[1]));
        rgb.push(blend(p[2]));
    }
    rgb
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_valid_pdf_header() {
        let img = RgbaImage::from_pixel(40, 30, Rgba([30, 120, 200, 255]));
        let path = std::env::temp_dir().join(format!("pinsight-pdf-{}.pdf", std::process::id()));
        export(&[img], &path).expect("export");
        let bytes = std::fs::read(&path).expect("read");
        assert!(bytes.starts_with(b"%PDF-1.4"), "missing PDF header");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Type /Page "));
        assert!(text.contains("/Filter /FlateDecode"));
        assert!(text.contains("%%EOF"));
        assert!(text.contains("startxref"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tall_image_splits_into_several_pages() {
        // 200 x 4000 is far taller than A4; it must spill over.
        let img = RgbaImage::from_pixel(200, 4000, Rgba([10, 10, 10, 255]));
        let plans = plan_pages(&[img]);
        assert!(plans.len() > 1, "tall image should paginate");
        // Every band stays inside the source height and is contiguous.
        let mut covered = 0;
        for p in &plans {
            assert!(p.src.1 + p.src.3 <= 4000);
            covered += p.src.3;
        }
        assert!(covered >= 4000, "bands should cover the whole image");
    }

    #[test]
    fn wide_image_uses_a_landscape_page() {
        let img = RgbaImage::from_pixel(400, 100, Rgba([1, 2, 3, 255]));
        let plans = plan_pages(&[img]);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].pw > plans[0].ph, "wide image should be landscape");
    }
}
