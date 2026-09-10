use std::io::{self, Read};

use crate::{discard, Format, ImageMetadata};

pub(crate) const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// tEXt chunks larger than this are skipped rather than buffered. A
/// well-formed image has no reason to carry megabytes of plain text
/// metadata, and the chunk length field is attacker-controlled input.
const MAX_TEXT_CHUNK: u64 = 1_000_000;

pub(crate) fn read<R: Read>(mut r: R) -> io::Result<ImageMetadata> {
    let mut width = 0u32;
    let mut height = 0u32;
    let mut text = Vec::new();

    loop {
        let mut len_buf = [0u8; 4];
        match r.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len_buf) as u64;

        let mut kind = [0u8; 4];
        r.read_exact(&mut kind)?;

        match &kind {
            b"IHDR" => {
                if len != 13 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid IHDR length",
                    ));
                }
                let mut data = [0u8; 13];
                r.read_exact(&mut data)?;
                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            }
            b"tEXt" if len <= MAX_TEXT_CHUNK => {
                let mut data = vec![0u8; len as usize];
                r.read_exact(&mut data)?;
                if let Some(pos) = data.iter().position(|&b| b == 0) {
                    let keyword = String::from_utf8_lossy(&data[..pos]).into_owned();
                    let value = String::from_utf8_lossy(&data[pos + 1..]).into_owned();
                    text.push((keyword, value));
                }
            }
            _ => discard(&mut r, len)?,
        }

        let mut crc = [0u8; 4];
        r.read_exact(&mut crc)?;

        if &kind == b"IEND" {
            break;
        }
    }

    Ok(ImageMetadata {
        format: Format::Png,
        width,
        height,
        exif: None,
        text,
    })
}
