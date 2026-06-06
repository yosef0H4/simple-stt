use anyhow::Result;
use image::{ImageBuffer, Rgba};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum UiSurface {
    Settings,
    Overlay,
}

pub fn save(surface: UiSurface, output: &Path, section: Option<&str>) -> Result<()> {
    match surface {
        UiSurface::Settings => save_settings(output, section),
        UiSurface::Overlay => save_overlay(output),
    }
}

fn save_settings(output: &Path, section: Option<&str>) -> Result<()> {
    let mut img = ImageBuffer::from_pixel(1120, 760, Rgba([245, 247, 250, 255]));
    rect(&mut img, 0, 0, 240, 760, [28, 33, 39, 255]);
    text_block(&mut img, 34, 34, 170, 26, [86, 214, 185, 255]);
    let sections = ["General", "Audio", "Model", "Typing", "Logging", "Advanced"];
    for (idx, name) in sections.iter().enumerate() {
        let y = 100 + idx as u32 * 58;
        let active = section.unwrap_or("general").eq_ignore_ascii_case(name);
        rect(
            &mut img,
            22,
            y,
            196,
            40,
            if active {
                [54, 72, 82, 255]
            } else {
                [28, 33, 39, 255]
            },
        );
        label_bar(&mut img, 46, y + 12, name.len() as u32 * 8, active);
    }
    rect(&mut img, 278, 42, 780, 72, [255, 255, 255, 255]);
    rect(&mut img, 278, 144, 360, 152, [255, 255, 255, 255]);
    rect(&mut img, 670, 144, 388, 152, [255, 255, 255, 255]);
    rect(&mut img, 278, 330, 780, 178, [255, 255, 255, 255]);
    rect(&mut img, 278, 542, 780, 128, [255, 255, 255, 255]);
    accent_line(&mut img, 310, 190, 270);
    accent_line(&mut img, 704, 190, 260);
    for idx in 0..20 {
        let height = 10 + ((idx * 13) % 46);
        rect(
            &mut img,
            322 + idx * 12,
            250 - height,
            7,
            height,
            [86, 214, 185, 255],
        );
    }
    rect(&mut img, 704, 402, 310, 52, [238, 246, 244, 255]);
    rect(&mut img, 704, 472, 150, 42, [86, 214, 185, 255]);
    img.save(output)?;
    Ok(())
}

fn save_overlay(output: &Path) -> Result<()> {
    let mut img = ImageBuffer::from_pixel(360, 140, Rgba([0, 0, 0, 0]));
    rect(&mut img, 70, 44, 220, 54, [24, 26, 30, 225]);
    for idx in 0..12 {
        let height = 12 + ((idx * 17) % 32);
        rect(
            &mut img,
            94 + idx * 14,
            71 - height / 2,
            7,
            height,
            [86, 214, 185, 245],
        );
    }
    img.save(output)?;
    Ok(())
}

fn rect(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    let max_x = (x + w).min(img.width());
    let max_y = (y + h).min(img.height());
    for py in y..max_y {
        for px in x..max_x {
            img.put_pixel(px, py, Rgba(color));
        }
    }
}

fn text_block(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    rect(img, x, y, w, h, color);
}

fn label_bar(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, x: u32, y: u32, w: u32, active: bool) {
    rect(
        img,
        x,
        y,
        w,
        10,
        if active {
            [240, 247, 250, 255]
        } else {
            [142, 154, 165, 255]
        },
    );
}

fn accent_line(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, x: u32, y: u32, w: u32) {
    rect(img, x, y, w, 8, [86, 214, 185, 255]);
}
