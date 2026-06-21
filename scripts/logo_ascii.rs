//! Throwaway helper: convert the Aurelia logo PNG into ASCII art for the splash.
//! Usage: cargo run --example logo_ascii -- <png> [cols]

fn main() {
    let path = std::env::args().nth(1).expect("usage: logo_ascii <png> [cols]");
    let cols: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(72);

    let img = image::open(&path).expect("open image").to_rgba8();
    let (w, h) = img.dimensions();

    // Terminal cells are roughly twice as tall as wide, so squash the row count.
    let rows = ((h as f32 / w as f32) * cols as f32 * 0.5).round().max(1.0) as u32;
    let ramp: Vec<char> = " .:-=+*#%@".chars().collect();
    let cw = w as f32 / cols as f32;
    let ch = h as f32 / rows as f32;

    let mut out = String::new();
    for ry in 0..rows {
        for rx in 0..cols {
            let x0 = (rx as f32 * cw) as u32;
            let x1 = (((rx + 1) as f32 * cw) as u32).min(w);
            let y0 = (ry as f32 * ch) as u32;
            let y1 = (((ry + 1) as f32 * ch) as u32).min(h);

            let mut density = 0f32;
            let mut n = 0f32;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = img.get_pixel(xx, yy).0;
                    let a = p[3] as f32 / 255.0;
                    let lum = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
                        / 255.0;
                    // "Ink" = opaque, non-white pixels (the gold artwork). Darker
                    // gold -> denser. Transparent or near-white -> background.
                    density += a * (1.0 - lum);
                    n += 1.0;
                }
            }
            let density = if n > 0.0 { density / n } else { 0.0 };
            let idx = if density < 0.10 {
                0
            } else {
                // Stretch the gold band (~0.1..0.5) across the ramp.
                let scaled = ((density - 0.10) / 0.40).clamp(0.0, 1.0);
                ((scaled * (ramp.len() - 1) as f32).round() as usize).clamp(1, ramp.len() - 1)
            };
            out.push(ramp[idx]);
        }
        out.push('\n');
    }
    print!("{}", out);
}
