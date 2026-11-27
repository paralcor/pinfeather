use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::{Crc, Decompress, FlushDecompress, Status};
use png::{ColorType, DecodeOptions, Decoder, Limits, OutputInfo, Transformations};
use serde::Serialize;

use crate::contracts::{ContractError, ContractResult, MAX_PNG_BYTES};

const MAX_SIDE: u32 = 8192;
const MAX_PIXELS: u64 = 16_777_216;
const MAX_DECODE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FRAMES: u64 = 128;
const PREVIEW_SIDE: u32 = 320;

#[derive(Debug, Serialize)]
pub(crate) struct Thumbnail {
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    rgba_base64: String,
}

fn invalid_png() -> ContractError {
    ContractError::invalid_input("PNG is invalid or exceeds the image limits")
}

fn filtered_bytes(width: u32, height: u32, bits: usize, interlaced: bool) -> usize {
    let pass_bytes = |width: u32, height: u32| {
        if width == 0 { 0 } else { ((width as usize * bits).div_ceil(8) + 1) * height as usize }
    };
    if !interlaced { return pass_bytes(width, height); }
    // Adam7 passes are (column offset, row offset, column step, row step).
    [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
        (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)].into_iter().map(|(x, y, dx, dy)| {
        pass_bytes(width.saturating_sub(x).div_ceil(dx), height.saturating_sub(y).div_ceil(dy))
    }).sum()
}

fn validate_image_streams(mut chunks: &[u8], mut lengths: &[usize]) -> ContractResult<()> {
    // png can stop inflating after the last row and only check the remaining chunk CRCs.
    // Read every frame through its zlib trailer with a fixed-size scratch buffer.
    let mut inflater = Decompress::new(true);
    let mut output = [0; 16 * 1024];
    let mut has_data = false;
    let mut stream_end = false;
    loop {
        let (header, rest) = chunks.split_at_checked(8).ok_or_else(invalid_png)?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let (payload, rest) = rest.split_at_checked(length).ok_or_else(invalid_png)?;
        let (expected_crc, rest) = rest.split_at_checked(4).ok_or_else(invalid_png)?;
        let kind = &header[4..];
        let mut crc = Crc::new();
        crc.update(kind);
        crc.update(payload);
        if crc.sum().to_be_bytes() != expected_crc { return Err(invalid_png()); }
        chunks = rest;
        let mut input = match kind {
            b"IDAT" => payload,
            b"fdAT" => payload.get(4..).ok_or_else(invalid_png)?, // Skip the APNG sequence number.
            b"fcTL" if has_data => {
                if !stream_end { return Err(invalid_png()); }
                lengths = &lengths[1..];
                inflater.reset(true);
                has_data = false;
                stream_end = false;
                continue;
            }
            b"IEND" => return if stream_end && lengths.len() == 1 { Ok(()) } else { Err(invalid_png()) },
            _ => continue,
        };
        has_data = true;
        // A verified stream may be followed by PNG's allowed IDAT padding.
        if stream_end { continue; }
        let expected = *lengths.first().ok_or_else(invalid_png)? as u64;
        let mut drain = false;
        while !input.is_empty() || drain {
            let (previous_in, previous_out) = (inflater.total_in(), inflater.total_out());
            let status = inflater.decompress(input, &mut output, FlushDecompress::None).map_err(|_| invalid_png())?;
            let consumed = (inflater.total_in() - previous_in) as usize;
            let produced = inflater.total_out() - previous_out;
            if inflater.total_out() > expected { return Err(invalid_png()); }
            if status == Status::StreamEnd {
                if inflater.total_out() != expected { return Err(invalid_png()); }
                stream_end = true;
                break;
            }
            if consumed == 0 && produced == 0 {
                if input.is_empty() { break; }
                return Err(invalid_png());
            }
            input = &input[consumed..];
            drain = produced == output.len() as u64;
        }
    }
}

fn decode(
    data: &[u8],
    mut first_frame: impl FnMut(&OutputInfo, &[u8]) -> ContractResult<()>,
) -> ContractResult<()> {
    if data.len() > MAX_PNG_BYTES {
        return Err(invalid_png());
    }
    let mut options = DecodeOptions::default();
    options.set_ignore_adler32(false);
    let mut decoder = Decoder::new_with_options(Cursor::new(data), options);
    decoder.set_limits(Limits { bytes: MAX_DECODE_BYTES });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(Transformations::normalize_to_color8());
    let header = decoder.read_header_info().map_err(|_| invalid_png())?;
    let pixels = u64::from(header.width) * u64::from(header.height);
    if header.width > MAX_SIDE || header.height > MAX_SIDE || pixels > MAX_PIXELS {
        return Err(invalid_png());
    }

    let mut reader = decoder.read_info().map_err(|_| invalid_png())?;
    let frames = reader.info().animation_control.map_or(1, |animation| {
        u64::from(animation.num_frames) + u64::from(reader.info().frame_control.is_none())
    });
    if frames > MAX_FRAMES || pixels * frames > MAX_PIXELS {
        return Err(invalid_png());
    }
    let length = reader.output_buffer_size().filter(|size| *size <= MAX_DECODE_BYTES)
        .ok_or_else(invalid_png)?;
    let mut decoded = vec![0; length];
    let bits = reader.info().bits_per_pixel();
    let interlaced = reader.info().interlaced;
    let mut lengths = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let info = reader.next_frame(&mut decoded).map_err(|_| invalid_png())?;
        lengths.push(filtered_bytes(info.width, info.height, bits, interlaced));
        if index == 0 {
            first_frame(&info, &decoded[..info.buffer_size()])?;
        }
    }
    drop(decoded);
    reader.finish().map_err(|_| invalid_png())?;
    drop(reader);
    validate_image_streams(&data[8..], &lengths)
}

pub(crate) fn validate(data: &[u8]) -> ContractResult<()> {
    decode(data, |_, _| Ok(()))
}

pub(crate) fn thumbnail(data: &[u8]) -> ContractResult<Thumbnail> {
    let mut thumbnail = None;
    decode(data, |frame, pixels| {
        let scale = frame.width.max(frame.height).max(PREVIEW_SIDE);
        let width = (frame.width * PREVIEW_SIDE / scale).max(1);
        let height = (frame.height * PREVIEW_SIDE / scale).max(1);
        let channels = frame.color_type.samples();
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let row = (y * frame.height / height) as usize * frame.line_size;
            for x in 0..width {
                let index = row + (x * frame.width / width) as usize * channels;
                let pixel = &pixels[index..index + channels];
                let color = match frame.color_type {
                    ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
                    ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
                    ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
                    ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
                    ColorType::Indexed => return Err(invalid_png()),
                };
                rgba.extend_from_slice(&color);
            }
        }
        thumbnail = Some(Thumbnail {
            source_width: frame.width, source_height: frame.height, width, height,
            rgba_base64: STANDARD.encode(rgba),
        });
        Ok(())
    })?;
    thumbnail.ok_or_else(invalid_png)
}

#[cfg(test)]
pub(crate) fn sample_png(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    let mut encoder = png::Encoder::new(&mut data, width, height);
    encoder.set_color(ColorType::Rgb);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[22, 44, 66].repeat((width * height) as usize)).unwrap();
    writer.finish().unwrap();
    data
}

#[cfg(test)]
pub(crate) fn adler_test_png(corrupt: bool) -> Vec<u8> {
    let mut data = vec![
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a,
        0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1,
        8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde,
        0, 0, 0, 15, b'I', b'D', b'A', b'T', 0x78, 0x01, 0x01, 4, 0, 0xfb, 0xff,
        0, 22, 44, 66, 0, 0xe0, 0, 0x85, 0x77, 0x24, 0x9b, 0xb5,
        0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    if corrupt {
        // Change the zlib Adler-32 trailer and use the matching CRC-32 for the modified IDAT.
        data[55..60].copy_from_slice(&[0x84, 0, 0x23, 0xab, 0x23]);
    }
    data
}

#[cfg(test)]
pub(crate) fn split_adler_test_png(corrupt: bool, animated: bool) -> Vec<u8> {
    use png::chunk::{fcTL, fdAT, IDAT};
    let mut data = Vec::new();
    let mut encoder = png::Encoder::new(&mut data, 1, 1);
    encoder.set_color(ColorType::Rgb);
    if animated { encoder.set_animated(1, 0).unwrap(); }
    let mut writer = encoder.write_header().unwrap();
    let pixels = [0x78, 0x01, 0x01, 4, 0, 0xfb, 0xff, 0, 22, 44, 66];
    let trailer = [0, 0xe0, 0, if corrupt { 0x84 } else { 0x85 }];
    if animated {
        // The fallback image is separate from the animation's one frame.
        writer.write_chunk(IDAT, &[pixels.as_slice(), &[0, 0xe0, 0, 0x85]].concat()).unwrap();
        let mut frame = [0; 26];
        frame[7] = 1;
        frame[11] = 1;
        writer.write_chunk(fcTL, &frame).unwrap();
        writer.write_chunk(fdAT, &[&1u32.to_be_bytes()[..], &pixels].concat()).unwrap();
        writer.write_chunk(fdAT, &[&2u32.to_be_bytes()[..], &trailer].concat()).unwrap();
    } else {
        writer.write_chunk(IDAT, &pixels).unwrap();
        writer.write_chunk(IDAT, &trailer).unwrap();
    }
    writer.finish().unwrap();
    data
}

#[cfg(test)]
pub(crate) fn corrupt_fdat_crc_test_png() -> Vec<u8> {
    let mut data = split_adler_test_png(false, true);
    // A 12-byte IEND follows the last fdAT, which holds a sequence number and Adler-32.
    let crc_start = data.len() - 12 - 4;
    assert_eq!(&data[crc_start - 12..crc_start - 8], b"fdAT");
    data[crc_start] ^= 1;
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ErrorCode;

    #[test]
    fn scales_png_and_returns_only_rgba_thumbnail() {
        let data = sample_png(640, 320);
        let image = thumbnail(&data).unwrap();
        assert_eq!((image.source_width, image.source_height), (640, 320));
        assert_eq!((image.width, image.height), (320, 160));
        let rgba = STANDARD.decode(image.rgba_base64).unwrap();
        assert_eq!(rgba.len(), 320 * 160 * 4);
        assert_eq!(&rgba[..4], &[22, 44, 66, 255]);
    }

    #[test]
    fn rejects_bad_png_before_returning_a_thumbnail_or_allowing_copy() {
        let mut data = sample_png(2, 1);
        data.truncate(data.len() - 4);
        for invalid in [data, sample_png(MAX_SIDE + 1, 1), vec![0; MAX_PNG_BYTES + 1]] {
            assert_eq!(thumbnail(&invalid).unwrap_err().code, ErrorCode::InvalidInput);
            assert_eq!(validate(&invalid).unwrap_err().code, ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn rejects_bad_adler32_even_with_valid_idat_crc() {
        let good = adler_test_png(false);
        assert!(thumbnail(&good).is_ok());
        assert!(validate(&good).is_ok());
        let bad = adler_test_png(true);
        assert_eq!(thumbnail(&bad).map_err(|error| error.code).err(), Some(ErrorCode::InvalidInput));
        assert_eq!(validate(&bad).map_err(|error| error.code).err(), Some(ErrorCode::InvalidInput));
    }

    #[test]
    fn rejects_split_adler32_in_idat() {
        assert!(thumbnail(&split_adler_test_png(false, false)).is_ok());
        assert_eq!(thumbnail(&split_adler_test_png(true, false)).unwrap_err().code, ErrorCode::InvalidInput);
    }

    #[test]
    fn rejects_bad_fdat_crc_with_valid_adler32() {
        assert!(thumbnail(&split_adler_test_png(false, true)).is_ok());
        assert_eq!(thumbnail(&corrupt_fdat_crc_test_png()).unwrap_err().code, ErrorCode::InvalidInput);
    }

    #[test]
    fn rejects_split_adler32_in_fdat() {
        assert!(thumbnail(&split_adler_test_png(false, true)).is_ok());
        assert_eq!(thumbnail(&split_adler_test_png(true, true)).unwrap_err().code, ErrorCode::InvalidInput);
    }
}
