use anyhow::{anyhow, bail, Context};
use byteorder::{BigEndian, ReadBytesExt};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::surface::Surface;
use std::io::{Cursor, Read};

#[derive(Debug, Clone, Copy)]
struct Pixel(u8, u8, u8, u8);

impl Pixel {
    fn hash(self) -> usize {
        let r = self.0 as usize;
        let g = self.1 as usize;
        let b = self.2 as usize;
        let a = self.3 as usize;
        (r * 3 + g * 5 + b * 7 + a * 11) % 64
    }
}

enum Channels {
    Rgb,
    Rgba,
}

enum Colorspace {
    Srgb,
    Linear,
}

struct QoiImage {
    width: u32,
    height: u32,
    channels: Channels,
    _colorspace: Colorspace,
    pixels: Vec<u8>,
}

impl QoiImage {
    const MAGIC: [u8; 4] = *b"qoif";

    fn decode(data: &[u8]) -> anyhow::Result<Self> {
        let mut cursor = Cursor::new(data);

        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic)?;

        if magic != Self::MAGIC {
            bail!("invalid magic bytes");
        }

        let width = cursor.read_u32::<BigEndian>().context("read width")?;
        let height = cursor.read_u32::<BigEndian>().context("read height")?;
        let channels = cursor.read_u8().context("read num channels")?;
        let colorspace = cursor.read_u8().context("read colorspace")?;

        if channels != 3 && channels != 4 {
            bail!("invalid number of channels");
        }

        if colorspace != 0 && colorspace != 1 {
            bail!("invalid colorspace");
        }

        let num_pixel_bytes = width as usize * height as usize * channels as usize;
        let mut pixels = Vec::with_capacity(num_pixel_bytes);

        let mut prev_pixel = Pixel(0, 0, 0, 255);
        let mut seen_pixels = [Pixel(0, 0, 0, 0); 64];
        seen_pixels[prev_pixel.hash()] = prev_pixel;

        while pixels.len() < num_pixel_bytes {
            let op = cursor.read_u8().context("read op")?;
            let p = if op == 0b1111_1110 {
                // QOI_OP_RGB
                let r = cursor.read_u8().context("QOI_OP_RGB read r")?;
                let g = cursor.read_u8().context("QOI_OP_RGB read g")?;
                let b = cursor.read_u8().context("QOI_OP_RGB read b")?;
                let a = prev_pixel.3;
                Pixel(r, g, b, a)
            } else if op == 0b1111_1111 {
                // QOI_OP_RGBA
                let r = cursor.read_u8().context("QOI_OP_RGBA read r")?;
                let g = cursor.read_u8().context("QOI_OP_RGBA read g")?;
                let b = cursor.read_u8().context("QOI_OP_RGBA read b")?;
                let a = cursor.read_u8().context("QOI_OP_RGBA read a")?;
                Pixel(r, g, b, a)
            } else if op & 0b1100_0000 == 0b0000_0000 {
                // QOI_OP_INDEX
                let idx = op & 0b0011_1111;
                seen_pixels[idx as usize]
            } else if op & 0b1100_0000 == 0b0100_0000 {
                // QOI_OP_DIFF
                let dr = (op >> 4) & 0b11;
                let dg = (op >> 2) & 0b11;
                let db = op & 0b11;
                Pixel(
                    prev_pixel.0.wrapping_add(dr).wrapping_sub(2),
                    prev_pixel.1.wrapping_add(dg).wrapping_sub(2),
                    prev_pixel.2.wrapping_add(db).wrapping_sub(2),
                    prev_pixel.3,
                )
            } else if op & 0b1100_0000 == 0b1000_0000 {
                // QOI_OP_LUMA
                let next_byte = cursor.read_u8().context("QOI_OP_LUMA read next byte")?;

                let dg = (op & 0b0011_1111).wrapping_sub(32);
                let dr = (next_byte >> 4).wrapping_add(dg).wrapping_sub(8);
                let db = (next_byte & 0xF).wrapping_add(dg).wrapping_sub(8);

                Pixel(
                    prev_pixel.0.wrapping_add(dr),
                    prev_pixel.1.wrapping_add(dg),
                    prev_pixel.2.wrapping_add(db),
                    prev_pixel.3,
                )
            } else if op & 0b1100_0000 == 0b1100_0000 {
                // QOI_OP_RUN
                let run = (op & 0b0011_1111) + 1;

                for _ in 0..run {
                    pixels.push(prev_pixel.0);
                    pixels.push(prev_pixel.1);
                    pixels.push(prev_pixel.2);
                    if channels == 4 {
                        pixels.push(prev_pixel.3);
                    }
                }
                continue;
            } else {
                bail!("invalid op")
            };

            prev_pixel = p;
            seen_pixels[p.hash()] = p;

            pixels.push(p.0);
            pixels.push(p.1);
            pixels.push(p.2);
            if channels == 4 {
                pixels.push(p.3);
            }
        }

        let mut end_marker = [0u8; 8];
        cursor
            .read_exact(&mut end_marker)
            .context("read byte stream end marker")?;
        if end_marker != [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01] {
            bail!("invalid byte stream end marker");
        }

        Ok(Self {
            width,
            height,
            channels: if channels == 3 {
                Channels::Rgb
            } else {
                Channels::Rgba
            },
            _colorspace: if colorspace == 0 {
                Colorspace::Srgb
            } else {
                Colorspace::Linear
            },
            pixels,
        })
    }

    fn pitch(&self) -> u32 {
        self.width * self.bytes_per_pixel()
    }

    fn bytes_per_pixel(&self) -> u32 {
        match self.channels {
            Channels::Rgb => 3,
            Channels::Rgba => 4,
        }
    }
}

pub fn main() -> anyhow::Result<()> {
    let Some(filepath) = std::env::args().nth(1) else {
        bail!("usage: qoi_viewer <path>");
    };

    let data = std::fs::read(&filepath).context("reading file")?;

    let image = QoiImage::decode(&data).context("parsing qoi file")?;

    let sdl_context = sdl2::init()
        .map_err(|e| anyhow!(e))
        .context("initializing sdl2")?;

    let video_subsystem = sdl_context
        .video()
        .map_err(|e| anyhow!(e))
        .context("initializing video subsystem")?;

    let window = video_subsystem
        .window(&filepath, image.width, image.height)
        .position_centered()
        .build()
        .map_err(|e| anyhow!(e))
        .context("creating window")?;

    let mut canvas = window
        .into_canvas()
        .build()
        .map_err(|e| anyhow!(e))
        .context("creating canvas")?;

    let mut pixel_data = image.pixels.clone();
    let surface = Surface::from_data(
        &mut pixel_data,
        image.width,
        image.height,
        image.pitch(),
        match image.channels {
            Channels::Rgb => PixelFormatEnum::RGB24,
            Channels::Rgba => PixelFormatEnum::RGBA32,
        },
    )
    .map_err(|e| anyhow!(e))
    .context("creating surface from image")?;

    let texture_creator = canvas.texture_creator();
    let texture = texture_creator
        .create_texture_from_surface(surface)
        .map_err(|e| anyhow!(e))
        .context("creating texture from surface")?;

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas
        .copy(&texture, None, None)
        .map_err(|e| anyhow!(e))
        .context("copying image texture to canvas")?;
    canvas.present();

    let mut event_pump = sdl_context.event_pump().unwrap();
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }
    }

    Ok(())
}
