//! Developer CLI. Foundation commands only; extraction/decoding comes later.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use soe_res::ResFile;
use soe_video::decode_planar;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [res, info, path] if res == "res" && info == "info" => res_info(path),
        [img, dump, path, out] if img == "img" && dump == "dump" => img_dump(path, out, "auto"),
        [img, dump, path, out, flag, pal]
            if img == "img" && dump == "dump" && flag == "--palette" =>
        {
            img_dump(path, out, pal)
        }
        [room, compose, scenes, id, resdir, out] if room == "room" && compose == "compose" => {
            room_compose(scenes, id, resdir, out, 0)
        }
        [room, compose, scenes, id, resdir, out, flag, frame]
            if room == "room" && compose == "compose" && flag == "--frame" =>
        {
            room_compose(scenes, id, resdir, out, frame.parse().context("frame")?)
        }
        [font, dump, res, exe, text, out] if font == "font" && dump == "dump" => {
            font_dump(res, exe, text, out)
        }
        [] => {
            print_help();
            Ok(())
        }
        [help] if help == "-h" || help == "--help" => {
            print_help();
            Ok(())
        }
        _ => {
            print_help();
            bail!("unknown command")
        }
    }
}

fn print_help() {
    eprintln!("soe res info <file.res>   show .RES header and record table");
    eprintln!("soe img dump <file.res> <outdir> [--palette auto|room|map|ui]");
    eprintln!("soe room compose <scenes.res> <scene-id> <resdir> <out.png> [--frame N]");
    eprintln!("soe font dump <title.res> <excal.exe> <text> <out.png>");
}

fn font_dump(res_path: &str, exe_path: &str, text: &str, out: &str) -> Result<()> {
    let file = ResFile::parse(std::fs::read(res_path)?)?;
    let record = file
        .records()
        .into_iter()
        .find(|record| record.id == 200)
        .context("font atlas record 200 not found")?;
    let atlas = decode_planar(&file.decode(record)?)?;
    let exe = std::fs::read(exe_path).with_context(|| format!("reading {exe_path}"))?;
    let (height, spacing, metrics) =
        soe_engine::font::BitmapFont::metrics_from_excal(&exe).context("EXCAL font metrics")?;
    if u16::from(height) != atlas.height {
        eprintln!(
            "warning: metric height {height} differs from atlas height {}",
            atlas.height
        );
    }
    let font =
        soe_engine::font::BitmapFont::new(atlas, spacing, &metrics).map_err(anyhow::Error::msg)?;

    let (_, colors) = palette_table("ui");
    let mut frame = soe_video::IndexedFrame::new(
        320,
        100,
        vec![0; 320 * 100],
        soe_assets::Palette::new(colors.clone()),
    )?;
    font.draw_text(&mut frame, 2, 2, text, None);
    let mut pal = vec![0u8; 768];
    for (i, color) in colors.iter().enumerate() {
        pal[i * 3..i * 3 + 3].copy_from_slice(color);
    }
    let f = std::fs::File::create(out).with_context(|| format!("creating {out}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), 320, 100);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(pal);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&frame.pixels)?;
    drop(writer);
    println!("wrote {text:?} to {out}");
    Ok(())
}

fn room_compose(scenes: &str, id: &str, resdir: &str, out: &str, frame: usize) -> Result<()> {
    use std::collections::HashMap;

    let scene_id: u16 = id.parse().context("scene id")?;
    let data = std::fs::read(scenes).with_context(|| format!("reading {scenes}"))?;
    let file = ResFile::parse(data)?;
    let rec = file
        .records()
        .into_iter()
        .find(|r| r.id == scene_id)
        .with_context(|| format!("scene {scene_id} not found"))?;
    let scene = soe_engine::scene::Scene::parse(&file.decode(rec)?).map_err(anyhow::Error::msg)?;

    let mut resources = soe_engine::resources::ResourceManager::new();
    for entry in std::fs::read_dir(resdir)
        .with_context(|| format!("reading {resdir}"))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_uppercase().ends_with(".RES") {
            resources.load_from_dir(std::path::Path::new(resdir), &name)?;
        }
    }

    let mut images = HashMap::new();
    for object in &scene.objects {
        if images.contains_key(&object.resource_id) {
            continue;
        }
        let Some(file) = resources.find_file(object.resource_id).map(str::to_owned) else {
            eprintln!("missing object {}", object.resource_id);
            continue;
        };
        let data = resources.record(&file, object.resource_id)?;
        if file.eq_ignore_ascii_case("ANI.RES") {
            if let Ok(bank) = soe_engine::ani::FrameBank::parse(data) {
                let index = if bank.frames.is_empty() {
                    0
                } else {
                    frame % bank.frames.len()
                };
                if let Some(image) = bank.frame_image(index) {
                    images.insert(object.resource_id, image);
                }
            }
            continue;
        }
        match decode_planar(data) {
            Ok(image) => {
                images.insert(object.resource_id, image);
            }
            Err(err) => eprintln!("object {} in {file}: {err}", object.resource_id),
        }
    }

    let (palette_id, colors) = palette_table("room");
    let palette = soe_assets::Palette::new(colors);
    let frame = soe_engine::room::compose_scene(&scene, &palette, |rid| images.get(&rid).cloned());

    let mut pal = vec![0u8; 768];
    for (i, c) in palette.colors.iter().enumerate().take(256) {
        pal[i * 3..i * 3 + 3].copy_from_slice(c);
    }
    let f = std::fs::File::create(out).with_context(|| format!("creating {out}"))?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(f),
        u32::from(frame.width),
        u32::from(frame.height),
    );
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(pal);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&frame.pixels)?;
    drop(writer);
    println!(
        "scene {scene_id}: {} objects, {} images, palette {palette_id} -> {out}",
        scene.objects.len(),
        images.len()
    );
    Ok(())
}

fn res_info(path: &str) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    let file = ResFile::parse(bytes).with_context(|| format!("parsing {path}"))?;
    let h = file.header();
    println!("file: {path}");
    println!("size: {}", file.bytes().len());
    println!("data_start: 0x{:x}", h.data_start);
    println!("index_offset: 0x{:x}", h.index_offset);
    println!("data_length: 0x{:x} ({})", h.data_length, h.data_length);
    println!("index_length: 0x{:x} ({})", h.index_length, h.index_length);
    println!("directory_entries (inferred): {}", file.directory_entries());
    println!();
    println!("records (best-effort sequential scan):");
    println!(
        "  {:>8}  {:>10}  {:>10}  {:>7}",
        "offset", "stored", "expanded", "compressed"
    );
    let records = file.records();
    if records.is_empty() {
        println!("  (none parsed)");
    }
    for (i, rec) in records.iter().enumerate() {
        let expanded = rec
            .expanded_len
            .map_or_else(|| "-".to_string(), |v| v.to_string());
        println!(
            "  {:>3}: 0x{:06x}  {:>10}  {:>10}  {:>7}",
            i, rec.offset, rec.stored_len, expanded, rec.compressed
        );
    }
    Ok(())
}

fn palette_table(kind: &str) -> (String, Vec<[u8; 3]>) {
    let ds = match kind {
        "map" | "world" => 0x20f9,
        "ui" => 0x2159,
        _ => 0x2099,
    };
    let exe = std::env::var_os("SOE_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("ghidra/EXCAL_UNPACKED.EXE"));
    if let Ok(bytes) = std::fs::read(&exe) {
        let off = 0x2a00 + 0x23640 + ds;
        if let Some(raw) = bytes.get(off..off + 96) {
            if raw.iter().all(|b| *b <= 15) {
                let colors = raw
                    .chunks_exact(3)
                    .map(|c| [c[0] * 17, c[1] * 17, c[2] * 17])
                    .collect();
                return (format!("exe:DS:{ds:04x}"), colors);
            }
        }
    }
    (
        format!("default-gray:{kind}"),
        (0..32)
            .map(|i| {
                let v = (i * 8) as u8;
                [v, v, v]
            })
            .collect(),
    )
}

fn img_dump(path: &str, outdir: &str, palette_kind: &str) -> Result<()> {
    let file = ResFile::parse(std::fs::read(path).with_context(|| format!("reading {path}"))?)?;
    let out = std::path::Path::new(outdir);
    std::fs::create_dir_all(out)?;
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let types = file.type_ids();
    let type_label = if types.is_empty() {
        "UNKNOWN".to_string()
    } else {
        types.join("+")
    };
    let mut manifest = Vec::new();
    let mut images = 0usize;
    for rec in file.records() {
        let data = file.decode(rec)?;
        if data.len() < 6 {
            manifest.push(format!(
                "{{\"record\":{},\"kind\":\"unknown\",\"type\":\"{}\"}}",
                rec.id, type_label
            ));
            continue;
        }
        let h = u16::from_be_bytes([data[0], data[1]]);
        let w = u16::from_be_bytes([data[2], data[3]]);
        let planes = u16::from_be_bytes([data[4], data[5]]);
        let mut stride = (usize::from(w) + 7) / 8;
        stride += stride & 1;
        let expected = 6usize + usize::from(planes) * usize::from(h) * stride;
        // Some full-screen records carry a few trailing bytes (INTF#501).
        if planes == 0 || planes > 8 || expected > data.len() {
            let kind = if data
                .iter()
                .take(64)
                .filter(|b| (0x20..=0x7e).contains(*b))
                .count()
                > 48
            {
                "text"
            } else if data.starts_with(b"FORM") || data.windows(4).any(|w| w == b"SMUS") {
                "smus"
            } else {
                "unknown"
            };
            manifest.push(format!(
                "{{\"record\":{},\"kind\":\"{kind}\",\"type\":\"{}\"}}",
                rec.id, type_label
            ));
            continue;
        }
        let img = decode_planar(&data)?;
        let (kind, is_default) = if palette_kind != "auto" {
            (palette_kind.to_string(), false)
        } else if stem == "ENGLAND" && rec.id == 510 {
            ("map".to_string(), false)
        } else if stem == "INTF" {
            ("ui".to_string(), false)
        } else if stem == "TITLE" || stem == "LOGO" {
            ("room".to_string(), true)
        } else {
            ("room".to_string(), false)
        };
        let (base_id, colors) = palette_table(&kind);
        let palette_id = if is_default {
            format!("default-{base_id}")
        } else {
            base_id
        };
        let mut pal = vec![0u8; 768];
        for (i, c) in colors.iter().enumerate() {
            pal[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let name = format!(
            "{}_{}_{}x{}.png",
            std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("res"),
            rec.id,
            w,
            h
        );
        let p = out.join(&name);
        let f = std::fs::File::create(&p)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), u32::from(w), u32::from(h));
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(pal.clone());
        let mut writer = enc.write_header()?;
        writer.write_image_data(&img.indices)?;
        drop(writer);
        let source_path = path.replace('\\', "/");
        let side = format!(
            "{{\"width\":{},\"height\":{},\"planes\":{},\"source\":\"{}#{} #0x{:x}\",\"flag\":{},\"palette\":{{\"id\":\"{}\",\"source\":\"{}\",\"default\":{}}}}}",
            w,
            h,
            planes,
            source_path,
            rec.id,
            rec.offset,
            rec.flags,
            palette_id,
            palette_id,
            is_default
        );
        std::fs::write(out.join(format!("{name}.json")), side)?;
        manifest.push(format!(
            "{{\"record\":{},\"kind\":\"image\",\"file\":\"{name}\",\"type\":\"{}\"}}",
            rec.id, type_label
        ));
        images += 1;
    }
    std::fs::write(
        out.join("manifest.json"),
        format!("[{}]", manifest.join(",")),
    )?;
    let room = palette_table("room").1;
    let map = palette_table("map").1;
    let ui = palette_table("ui").1;
    let palettes = format!(
        "{{\"room\":{{\"id\":\"exe:DS:2099\",\"colors\":{:?}}},\"map\":{{\"id\":\"exe:DS:20f9\",\"colors\":{:?}}},\"ui\":{{\"id\":\"exe:DS:2159\",\"colors\":{:?}}}}}",
        room, map, ui
    );
    std::fs::write(out.join("palettes.json"), palettes)?;
    println!("dumped {images} images to {}", out.display());
    Ok(())
}
