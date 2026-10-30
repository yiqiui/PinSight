//! Generic image upload: send a captured image to a destination and return a
//! shareable locator (a URL for the HTTP backend, an absolute path for the
//! local backend).
//!
//! PinSight intentionally hard-codes no single host. The HTTP backend POSTs or
//! PUTs to a URL the user configures, which covers image hosts, OSS/S3
//! presigned URLs and WebDAV. The local backend simply copies the file into a
//! folder — handy with no network or for keeping a local mirror.
//!
//! The call is synchronous and must NOT run on the GUI thread; callers should
//! spawn it on a worker thread and relay the `Result` back. The function only
//! ever returns `Result` (it never panics), which matters because the release
//! profile sets `panic = "abort"`.

use crate::config::{
    HttpMethod, HttpUpload, LocalUpload, UploadBackend, UploadConfig, UploadResponse,
};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;

/// Upload an in-memory image and return a shareable locator.
pub fn upload_image(image: &image::RgbaImage, cfg: &UploadConfig) -> Result<String> {
    if !cfg.enabled {
        anyhow::bail!("upload is disabled in the configuration");
    }
    match cfg.backend {
        UploadBackend::Local => upload_local(image, &cfg.local),
        UploadBackend::Http => upload_http(image, &cfg.http),
    }
}

fn upload_local(image: &image::RgbaImage, cfg: &LocalUpload) -> Result<String> {
    if cfg.folder.as_os_str().is_empty() {
        anyhow::bail!("upload.local.folder is not set");
    }
    std::fs::create_dir_all(&cfg.folder)
        .with_context(|| format!("creating upload folder {}", cfg.folder.display()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path: PathBuf = cfg.folder.join(format!("pinsight-{stamp}.png"));
    crate::util::save_png_to(image, &path)?;
    Ok(path.to_string_lossy().into_owned())
}

fn upload_http(image: &image::RgbaImage, cfg: &HttpUpload) -> Result<String> {
    if cfg.url.trim().is_empty() {
        anyhow::bail!("upload.http.url is not set");
    }
    let bytes = png_bytes(image)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_secs.max(1)))
        .build()
        .context("building the HTTP client")?;

    let method = match cfg.method {
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
    };
    let mut builder = client.request(method, &cfg.url);

    for raw in &cfg.headers {
        if let Some((k, v)) = raw.split_once(':') {
            builder = builder.header(k.trim(), v.trim());
        }
    }

    let field = if cfg.field.trim().is_empty() {
        "file".to_string()
    } else {
        cfg.field.trim().to_string()
    };

    // PUT (and any non-multipart intent) sends the raw bytes as the body; POST
    // sends multipart/form-data with the image under `field`.
    let resp = if matches!(cfg.method, HttpMethod::Put) {
        builder
            .header(reqwest::header::CONTENT_TYPE, "image/png")
            .body(bytes)
            .send()
            .context("upload request failed")?
    } else {
        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name("pinsight.png")
            .mime_str("image/png")?;
        let form = reqwest::blocking::multipart::Form::new().part(field, part);
        builder
            .multipart(form)
            .send()
            .context("upload request failed")?
    };

    let status = resp.status();
    let body = resp.text().context("reading the upload response body")?;
    if !status.is_success() {
        anyhow::bail!("upload failed: HTTP {status}\n{body}");
    }

    let locator = match cfg.response {
        UploadResponse::Raw => body.trim().to_string(),
        UploadResponse::Json => extract_json_field(&body, &cfg.json_field)
            .context("reading the URL from the JSON response")?,
    };
    if locator.trim().is_empty() {
        anyhow::bail!("upload succeeded but the response contained no URL");
    }
    Ok(locator.trim().to_string())
}

/// Encode the image as PNG bytes for upload.
fn png_bytes(image: &image::RgbaImage) -> Result<Vec<u8>> {
    let dyn_img = image::DynamicImage::ImageRgba8(image.clone());
    let mut buf: Vec<u8> = Vec::new();
    dyn_img
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .context("encoding the image as PNG")?;
    Ok(buf)
}

/// Walk a (possibly nested, "."-separated) key path through a JSON value and
/// return the string at the end. A missing key is an error; a non-string leaf
/// is stringified so a bare URL or number still works.
fn extract_json_field(body: &str, path: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("the response was not valid JSON")?;
    let mut current = &value;
    for key in path.split('.').filter(|k| !k.is_empty()) {
        current = current
            .get(key)
            .with_context(|| format!("the JSON response has no field `{key}`"))?;
    }
    match current {
        serde_json::Value::String(s) => Ok(s.clone()),
        other => Ok(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_json_field() {
        let body = r#"{"data":{"url":"https://x/y.png"}}"#;
        assert_eq!(
            extract_json_field(body, "data.url").unwrap(),
            "https://x/y.png"
        );
    }

    #[test]
    fn missing_json_field_errors() {
        let body = r#"{"ok":true}"#;
        assert!(extract_json_field(body, "url").is_err());
    }

    #[test]
    fn local_upload_writes_a_file() {
        let dir = std::env::temp_dir().join(format!("pinsight-upload-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = LocalUpload {
            folder: dir.clone(),
        };
        let img = image::RgbaImage::new(2, 2);
        let out = upload_local(&img, &cfg).unwrap();
        assert!(std::path::Path::new(&out).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
