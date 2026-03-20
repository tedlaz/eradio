use std::io::Cursor;

fn main() {
    // Only embed icon on Windows targets
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        return;
    }

    let ico_path = format!("{}/eradio.ico", std::env::var("OUT_DIR").unwrap());

    // Load the PNG icon and convert to multi-size ICO
    let png_bytes = std::fs::read("eradio.png").expect("Failed to read eradio.png");
    generate_ico_from_png(&png_bytes, &ico_path);

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&ico_path);
    res.set("FileDescription", "eRadio - Internet Radio Player");
    res.set("ProductName", "eRadio");
    res.set("LegalCopyright", "Ted Lazaros");
    res.compile().expect("Failed to compile Windows resources");
}

fn decode_png(data: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(Cursor::new(data));
    let mut reader = decoder.read_info().expect("Failed to read PNG info");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("Failed to decode PNG");
    buf.truncate(info.buffer_size());

    let (w, h) = (info.width, info.height);

    // Convert to RGBA if needed
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for chunk in buf.chunks(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for chunk in buf.chunks(2) {
                rgba.push(chunk[0]);
                rgba.push(chunk[0]);
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for &g in &buf {
                rgba.push(g);
                rgba.push(g);
                rgba.push(g);
                rgba.push(255);
            }
            rgba
        }
        _ => panic!("Unsupported PNG color type: {:?}", info.color_type),
    };

    (w, h, rgba)
}

fn resize_rgba(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            // Bilinear interpolation
            let sx = dx as f32 * x_ratio;
            let sy = dy as f32 * y_ratio;
            let x0 = (sx as u32).min(src_w - 1);
            let y0 = (sy as u32).min(src_h - 1);
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            // Read the four neighboring pixels
            let i00 = ((y0 * src_w + x0) * 4) as usize;
            let i10 = ((y0 * src_w + x1) * 4) as usize;
            let i01 = ((y1 * src_w + x0) * 4) as usize;
            let i11 = ((y1 * src_w + x1) * 4) as usize;

            // Get alpha values
            let a00 = src[i00 + 3] as f32;
            let a10 = src[i10 + 3] as f32;
            let a01 = src[i01 + 3] as f32;
            let a11 = src[i11 + 3] as f32;

            // Interpolate alpha
            let a = a00 * (1.0 - fx) * (1.0 - fy)
                + a10 * fx * (1.0 - fy)
                + a01 * (1.0 - fx) * fy
                + a11 * fx * fy;
            let di = ((dy * dst_w + dx) * 4) as usize;
            dst[di + 3] = a.round() as u8;

            // Interpolate RGB in premultiplied alpha space to avoid dark fringes
            if a > 0.0 {
                for c in 0..3 {
                    let p00 = src[i00 + c] as f32 * a00;
                    let p10 = src[i10 + c] as f32 * a10;
                    let p01 = src[i01 + c] as f32 * a01;
                    let p11 = src[i11 + c] as f32 * a11;
                    let val = p00 * (1.0 - fx) * (1.0 - fy)
                        + p10 * fx * (1.0 - fy)
                        + p01 * (1.0 - fx) * fy
                        + p11 * fx * fy;
                    dst[di + c] = (val / a).clamp(0.0, 255.0).round() as u8;
                }
            }
        }
    }
    dst
}

fn flood_fill_bg_transparent(rgba: &mut [u8], w: u32, h: u32) {
    let (w, h) = (w as usize, h as usize);
    let mut visited = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = Vec::new();

    // Seed from all edge pixels
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }

    while let Some((x, y)) = stack.pop() {
        let idx = y * w + x;
        if visited[idx] {
            continue;
        }
        let off = idx * 4;
        if rgba[off] <= 240 || rgba[off + 1] <= 240 || rgba[off + 2] <= 240 {
            continue;
        }
        visited[idx] = true;
        rgba[off] = 0;
        rgba[off + 1] = 0;
        rgba[off + 2] = 0;
        rgba[off + 3] = 0;

        if x > 0 { stack.push((x - 1, y)); }
        if x + 1 < w { stack.push((x + 1, y)); }
        if y > 0 { stack.push((x, y - 1)); }
        if y + 1 < h { stack.push((x, y + 1)); }
    }
}

fn generate_ico_from_png(png_bytes: &[u8], ico_path: &str) {
    let (w, h, mut rgba) = decode_png(png_bytes);
    flood_fill_bg_transparent(&mut rgba, w, h);
    let rgba = rgba;
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);

    for size in [16u32, 32, 48, 64, 256] {
        let resized = if w == size && h == size {
            rgba.clone()
        } else {
            resize_rgba(&rgba, w, h, size, size)
        };
        let image = ico::IconImage::from_rgba_data(size, size, resized);
        icon_dir.add_entry(ico::IconDirEntry::encode_as_png(&image).unwrap());
    }

    let mut buf = Vec::new();
    icon_dir.write(&mut Cursor::new(&mut buf)).unwrap();
    std::fs::write(ico_path, buf).unwrap();
}
