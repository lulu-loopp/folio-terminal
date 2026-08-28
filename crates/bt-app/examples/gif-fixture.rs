//! Temporary: write the animated-GIF fixture with four deliberately unequal
//! frame delays, then read it back through the decoder the product will use.
//! Deleted before the branch is reported.

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
