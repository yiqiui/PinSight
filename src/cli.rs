//! Headless command line, modelled on `scrot` / `maim`.
//!
//! `pinsight shot` captures without creating a window, which is what a Linux
//! desktop without a tray (or any shell script / CI job) wants:
//!
//! ```text
//! pinsight shot                          # whole monitor → save dir, print path
//! pinsight shot --out shot.png           # whole monitor → shot.png
//! pinsight shot --region 10,20,640,480   # physical rectangle
//! pinsight shot --clipboard              # whole monitor → clipboard
//! pinsight shot --delay 3 --out s.png    # wait 3 s first
//! pinsight shot --monitor 1              # pick a monitor by id
//! pinsight pdf a.png b.png --out book.pdf  # images → one PDF (long ones paginate)
//! ```
//!
//! Everything here avoids `eframe`: no window, no GL context, so it works over
//! SSH/X11 forwarding and in headless-ish sessions where a GUI cannot start.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use crate::capture;
use crate::pdf;
use crate::util::{self, PhysRect};

const USAGE: &str = "\
pinsight shot — capture without the GUI (scrot/maim style)

USAGE:
    pinsight shot [OPTIONS]

OPTIONS:
    --out <FILE>        write the PNG here (default: the configured save dir)
    --region X,Y,W,H    capture a physical-desktop rectangle
    --monitor <ID>      capture one monitor (see --list-monitors)
    --window <ID>       capture one top-level window (see --list-windows)
    --clipboard         copy the image to the clipboard
    --ocr               recognise text with Windows.Media.Ocr and print it
    --delay <SECONDS>   wait before capturing
    --list-monitors     print monitor ids and exit
    --list-windows      print window ids/titles and exit
    -h, --help          show this help

NOTE:
    --ocr uses the offline engine built into Windows 10/11; the recognised
    text is printed to stdout and the saved image path (if any) to stderr.
";

/// Returns true when the arguments asked for the CLI and it ran.
pub fn maybe_run(args: &[String]) -> Option<Result<()>> {
    let (cmd, rest) = args.split_first()?;
    match cmd.as_str() {
        "shot" | "capture" | "screenshot" => Some(run(rest)),
        "pdf" | "export-pdf" => Some(run_pdf(rest)),
        "gif" => Some(run_gif(rest)),
        "voice" => Some(run_voice(rest)),
        "help" if args.len() == 1 => {
            print!("{USAGE}");
            Some(Ok(()))
        }
        _ => None,
    }
}

fn run(args: &[String]) -> Result<()> {
    let mut out: Option<PathBuf> = None;
    let mut region: Option<PhysRect> = None;
    let mut monitor_id: Option<u32> = None;
    let mut window_id: Option<u32> = None;
    let mut clipboard = false;
    let mut delay = 0.0_f32;
    let mut ocr = false;
    let mut upload = false;
    let mut list = false;
    let mut list_windows = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--out needs a file path"))?;
                out = Some(PathBuf::from(value));
            }
            "--region" | "-r" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--region needs X,Y,W,H"))?;
                region = Some(parse_region(value)?);
            }
            "--monitor" | "-m" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--monitor needs an id"))?;
                monitor_id = Some(value.parse().context("--monitor expects a number")?);
            }
            "--clipboard" | "-c" => clipboard = true,
            "--ocr" => ocr = true,
            "--upload" => upload = true,
            "--delay" | "-d" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--delay needs seconds"))?;
                delay = value.parse().context("--delay expects a number")?;
            }
            "--list-monitors" => list = true,
            "--window" | "-w" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| anyhow!("--window needs an id"))?;
                window_id = Some(value.parse().context("--window expects a number")?);
            }
            "--list-windows" => list_windows = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => {
                return Err(anyhow!(
                    "unknown argument `{other}` (try `pinsight shot --help`)"
                ));
            }
        }
        i += 1;
    }

    if list {
        for monitor in capture::monitors()? {
            println!(
                "{}\t{}x{}+{}+{}\t{}",
                monitor.id,
                monitor.rect.w,
                monitor.rect.h,
                monitor.rect.x,
                monitor.rect.y,
                monitor.name
            );
        }
        return Ok(());
    }

    if list_windows {
        for w in capture::list_windows() {
            println!(
                "{}\t{}x{}+{}+{}\t{}\t{}",
                w.id, w.rect.w, w.rect.h, w.rect.x, w.rect.y, w.app, w.title
            );
        }
        return Ok(());
    }

    if delay > 0.0 {
        std::thread::sleep(Duration::from_secs_f32(delay));
    }

    let image = match region {
        Some(rect) => capture::grab_physical_rect(rect)?,
        None => {
            if let Some(id) = window_id {
                capture::grab_window(id)?
            } else {
                let info = match monitor_id {
                    Some(id) => capture::find_monitor(id)?,
                    None => capture::monitor_at_cursor()?,
                };
                capture::grab_monitor(&info)?
            }
        }
    };

    if ocr {
        let words = crate::ocr::recognize(image.clone())
            .context("OCR failed (Windows.Media.Ocr is Windows 10/11 only)")?;
        // stdout carries only the recognised text so it can be piped or
        // redirected cleanly; any saved path is reported on stderr below.
        println!("{}", crate::ocr::words_to_text(&words));
    }

    if upload {
        let config = crate::config::Config::load();
        let mut upload_cfg = config.upload.clone();
        if upload_cfg.backend == crate::config::UploadBackend::Local
            && upload_cfg.local.folder.as_os_str().is_empty()
        {
            upload_cfg.local.folder = config.save_dir.join("uploaded");
        }
        let url = crate::upload::upload_image(&image, &upload_cfg).context("upload failed")?;
        // stdout stays machine-readable: the shareable link, nothing else.
        println!("{url}");
        return Ok(());
    }

    if clipboard {
        let mut board = arboard::Clipboard::new().context("opening the clipboard")?;
        board
            .set_image(util::to_arboard(&image))
            .context("writing the image to the clipboard")?;
    }

    // Like maim: with neither --out nor --clipboard, still produce a file so a
    // shell script can capture the printed path.
    if out.is_some() || !clipboard {
        let path = match out {
            Some(path) => {
                util::save_png_to(&image, &path)?;
                path
            }
            None => {
                let config = crate::config::Config::load();
                util::save_image(
                    &image,
                    &config.save_dir,
                    &util::SaveOptions {
                        template: config.name_template.clone(),
                        quality: config.image_quality,
                        format: util::ImageFormat::Png,
                        title: String::new(),
                        decorations: config.decorations.clone(),
                    },
                )?
            }
        };
        if ocr {
            // Keep stdout (the text) machine-readable; report the file on stderr.
            eprintln!("{}", path.display());
        } else {
            // stdout stays machine-readable: one absolute path, nothing else.
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn run_pdf(args: &[String]) -> Result<()> {
    const PDF_USAGE: &str = "\
pinsight pdf — bundle screenshots into a PDF (scrot/maim style)

USAGE:
    pinsight pdf <IMAGE...> [--out FILE]

    Images become one PDF page each. Tall captures (long screenshots) are split
    across several portrait pages so the text stays readable. Transparency is
    flattened over white.

OPTIONS:
    <IMAGE...>        one or more image files (png/jpg/webp/bmp/tif/gif)
    --out <FILE>      output PDF (default: the first image with a .pdf extension)
    -h, --help        show this help
";
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| anyhow!("--out needs a path"))?;
                out = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                print!("{PDF_USAGE}");
                return Ok(());
            }
            other => {
                if other.starts_with('-') {
                    return Err(anyhow!(
                        "unknown argument `{other}` (try `pinsight pdf --help`)"
                    ));
                }
                inputs.push(PathBuf::from(other));
            }
        }
        i += 1;
    }
    if inputs.is_empty() {
        return Err(anyhow!("no input images — try `pinsight pdf --help`"));
    }
    let out = match out {
        Some(path) => path,
        None => {
            let mut fallback = inputs[0].clone();
            fallback.set_extension("pdf");
            fallback
        }
    };
    pdf::export_paths(&inputs, &out)?;
    // Machine-readable: the produced file path on stdout.
    println!("{}", out.display());
    Ok(())
}

fn run_gif(args: &[String]) -> Result<()> {
    const GIF_USAGE: &str = "\
pinsight gif — edit an animated GIF (scrot/maim style)

USAGE:
    pinsight gif <trim|crop|text|mp4> <INPUT> [OPTIONS]

OPERATIONS:
    trim        keep frames [--start N]..[--end N] (end 0 = last)
    crop        cut a rectangle --x --y --w --h
    text        burn --text onto every frame (--font N, --pos 0..4)
    mp4         convert to H.264 MP4 (needs ffmpeg)

OPTIONS:
    --out <FILE>      output path (default: <input>-out.gif|mp4)
    --speed <1..30>   GIF quality (1 fast, 30 best; default 10)
    --pos <0..4>      0 top-left 1 top-right 2 bottom-left 3 bottom-right 4 center
    -h, --help        show this help

EXAMPLES:
    pinsight gif trim clip.gif --start 2 --end 20
    pinsight gif crop clip.gif --x 10 --y 10 --w 200 --h 120
    pinsight gif text clip.gif --text \"Hello\" --font 48 --pos 3
    pinsight gif mp4 clip.gif
";
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{GIF_USAGE}");
        return Ok(());
    }
    let op = args[0].as_str();
    let rest = &args[1..];
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut start = 0u32;
    let mut end = 0u32;
    let mut x = 0u32;
    let mut y = 0u32;
    let mut w = 0u32;
    let mut h = 0u32;
    let mut text = String::new();
    let mut font = 32u32;
    let mut pos = 3u8;
    let mut speed = 10i32;
    // Consume the value that follows a flag. A free fn (not a closure) so it
    // does not capture `i`, which would block the `i += 1` at the loop's end.
    fn take<'a>(rest: &'a [String], i: &mut usize) -> Result<&'a String> {
        *i += 1;
        rest.get(*i)
            .ok_or_else(|| anyhow!("missing value after `{}`", rest[*i - 1]))
    }
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--out" | "-o" => {
                out = Some(PathBuf::from(take(rest, &mut i)?));
            }
            "--start" => {
                start = take(rest, &mut i)?
                    .parse()
                    .context("--start expects a number")?;
            }
            "--end" => {
                end = take(rest, &mut i)?
                    .parse()
                    .context("--end expects a number")?;
            }
            "--x" => {
                x = take(rest, &mut i)?
                    .parse()
                    .context("--x expects a number")?;
            }
            "--y" => {
                y = take(rest, &mut i)?
                    .parse()
                    .context("--y expects a number")?;
            }
            "--w" => {
                w = take(rest, &mut i)?
                    .parse()
                    .context("--w expects a number")?;
            }
            "--h" => {
                h = take(rest, &mut i)?
                    .parse()
                    .context("--h expects a number")?;
            }
            "--text" => {
                text = take(rest, &mut i)?.clone();
            }
            "--font" => {
                font = take(rest, &mut i)?
                    .parse()
                    .context("--font expects a number")?;
            }
            "--pos" => {
                pos = take(rest, &mut i)?.parse().context("--pos expects 0..4")?;
            }
            "--speed" => {
                speed = take(rest, &mut i)?
                    .parse()
                    .context("--speed expects a number")?;
            }
            other => {
                if other.starts_with('-') {
                    return Err(anyhow!(
                        "unknown argument `{other}` (try `pinsight gif --help`)"
                    ));
                }
                if input.is_none() {
                    input = Some(PathBuf::from(other));
                } else {
                    return Err(anyhow!(
                        "unexpected argument `{other}` (try `pinsight gif --help`)"
                    ));
                }
            }
        }
        i += 1;
    }
    let input = input.ok_or_else(|| anyhow!("missing input GIF (try `pinsight gif --help`)"))?;
    if !input.exists() {
        return Err(anyhow!("input not found: {}", input.display()));
    }
    let out = match out {
        Some(p) => p,
        None => {
            let mut o = input.clone();
            let stem = o
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            o.set_file_name(format!("{stem}-out"));
            o.set_extension(if op == "mp4" { "mp4" } else { "gif" });
            o
        }
    };
    match op {
        "trim" => crate::gifedit::trim(
            &input,
            &out,
            start as usize,
            if end == 0 { None } else { Some(end as usize) },
            speed,
        )?,
        "crop" => crate::gifedit::crop(&input, &out, x, y, w, h, speed)?,
        "text" => {
            let position = match pos {
                0 => crate::gifedit::TextPos::TopLeft,
                1 => crate::gifedit::TextPos::TopRight,
                2 => crate::gifedit::TextPos::BottomLeft,
                3 => crate::gifedit::TextPos::BottomRight,
                _ => crate::gifedit::TextPos::Center,
            };
            crate::gifedit::add_text(
                &input,
                &out,
                &crate::gifedit::TextOptions {
                    text,
                    font_size: font,
                    color: [255, 255, 255],
                    position,
                },
                speed,
            )?;
        }
        "mp4" => crate::gifedit::to_mp4(&input, &out)?,
        other => {
            return Err(anyhow!(
                "unknown operation `{other}` (try `pinsight gif --help`)"
            ));
        }
    }
    // Machine-readable: the produced file path on stdout.
    println!("{}", out.display());
    Ok(())
}

fn run_voice(args: &[String]) -> Result<()> {
    const VOICE_USAGE: &str = "\
pinsight voice — record a microphone voice note to WAV (needs ffmpeg)

USAGE:
    pinsight voice [--out FILE] [--seconds N]

OPTIONS:
    --out <FILE>      output WAV (default: voice-note.wav)
    --seconds <N>     stop after N seconds (default: record until interrupted)
    -h, --help        show this help
";
    let mut out: Option<PathBuf> = None;
    let mut seconds: Option<u32> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| anyhow!("--out needs a path"))?;
                out = Some(PathBuf::from(v));
            }
            "--seconds" | "-s" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--seconds needs a number"))?;
                seconds = Some(v.parse().context("--seconds expects a number")?);
            }
            "-h" | "--help" => {
                print!("{VOICE_USAGE}");
                return Ok(());
            }
            other => {
                return Err(anyhow!(
                    "unknown argument `{other}` (try `pinsight voice --help`)"
                ));
            }
        }
        i += 1;
    }
    let out = out.unwrap_or_else(|| PathBuf::from("voice-note.wav"));
    crate::voice::record(&out, seconds).context("voice recording failed")?;
    // Machine-readable: the produced file path on stdout.
    println!("{}", out.display());
    Ok(())
}

fn parse_region(value: &str) -> Result<PhysRect> {
    let parts: Vec<&str> = value.split(',').map(|p| p.trim()).collect();
    if parts.len() != 4 {
        return Err(anyhow!("--region expects X,Y,W,H (got `{value}`)"));
    }
    let nums: Vec<i64> = parts
        .iter()
        .map(|p| p.parse::<i64>().context("--region expects four integers"))
        .collect::<Result<_>>()?;
    Ok(PhysRect {
        x: nums[0] as i32,
        y: nums[1] as i32,
        w: nums[2].max(1) as u32,
        h: nums[3].max(1) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_region() {
        let r = parse_region("10,20,640,480").unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (10, 20, 640, 480));
    }

    #[test]
    fn rejects_malformed_regions() {
        assert!(parse_region("10,20,640").is_err());
        assert!(parse_region("a,b,c,d").is_err());
        assert!(parse_region("10,20,0,0").is_ok(), "zero size clamps to 1");
    }

    #[test]
    fn only_claims_its_own_subcommands() {
        assert!(maybe_run(&["shot".into()]).is_some());
        assert!(maybe_run(&["pdf".into(), "a.png".into(), "b.png".into()]).is_some());
        assert!(maybe_run(&["--mcp".into()]).is_none());
        assert!(maybe_run(&[]).is_none());
    }
}
