//! **Write `test-assets/folio-anim-test.gif`, then read it back through the
//! decoder the product uses** (route B slice ②, 2026-08-28; `docs/DESIGN.md`
//! §7.44 ⑤).
//!
//! The fixture's whole point is that its four frames declare **unequal** delays
//! — 100, 200, 300 and 400 ms — because every wrong way to animate a GIF passes
//! a uniform one: one frame per redraw, a constant hundred milliseconds, the
//! first frame's delay applied to all of them.
//!
//! ```text
//! cargo run --example gif-fixture -- test-assets\folio-anim-test.gif
//! ```
//!
//! Kept beside the fixture rather than deleted, on `PROVENANCE.md`'s own rule:
//! a tracked file whose recipe is gone is a file nobody can check. The read-back
//! is part of the recipe — it prints the four delays and the first pixel of each
//! frame, so running this is also how you confirm the round trip still survives
//! an `image` upgrade.

use std::fs::File;
use std::io::BufWriter;
use std::time::Duration;

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::{AnimationDecoder, Delay, Frame, Rgba, RgbaImage};

fn main() {
    let out = std::env::args().nth(1).expect("an output path");
    let colours = [
        [0xE0, 0x4B, 0x2F, 0xFF],
        [0x2F, 0xE0, 0x4B, 0xFF],
        [0x2F, 0x4B, 0xE0, 0xFF],
        [0xE0, 0xD2, 0x2F, 0xFF],
    ];
    let delays_ms = [100_u64, 200, 300, 400];
    {
        let file = BufWriter::new(File::create(&out).expect("create"));
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        for (colour, delay) in colours.iter().zip(delays_ms) {
            let mut image = RgbaImage::new(64, 64);
            for pixel in image.pixels_mut() {
                *pixel = Rgba(*colour);
            }
            encoder
                .encode_frame(Frame::from_parts(
                    image,
                    0,
                    0,
                    Delay::from_saturating_duration(Duration::from_millis(delay)),
                ))
                .expect("encode");
        }
    }
    let bytes = std::fs::read(&out).expect("read back");
    println!("{} bytes {}", out, bytes.len());
    let decoder = GifDecoder::new(std::io::Cursor::new(&bytes)).expect("decode");
    for (index, frame) in decoder.into_frames().enumerate() {
        let frame = frame.expect("a frame");
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let buffer = frame.buffer();
        println!(
            "  frame {index}: {}x{} delay {}/{} ms first-pixel {:?}",
            buffer.width(),
            buffer.height(),
            numerator,
            denominator,
            buffer.get_pixel(0, 0).0
        );
    }
}
