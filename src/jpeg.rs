use std::io::{self, Read};

use crate::{discard, Format, ImageMetadata};

fn read_u8<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u16<R: Read>(r: &mut R) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

/// Reads up to and including the marker byte, skipping the 0xFF fill bytes
/// that are allowed to pad the front of any marker.
fn read_marker<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut b = read_u8(r)?;
    if b != 0xFF {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected JPEG marker",
        ));
    }
    loop {
        b = read_u8(r)?;
        if b != 0xFF {
            return Ok(b);
        }
    }
}

/// Start-of-frame markers carry the dimensions. 0xC4, 0xC8 and 0xCC fall in
/// the same numeric range but are DHT / JPG / DAC, not SOF.
fn is_sof(marker: u8) -> bool {
    (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC
}

pub(crate) fn read<R: Read>(mut r: R) -> io::Result<ImageMetadata> {
    let mut width = 0u32;
    let mut height = 0u32;
    let mut exif = Vec::new();

    loop {
        let marker = read_marker(&mut r)?;

        // Standalone markers (TEM, RSTn, stray SOI) carry no length or
        // payload. SOS marks the start of entropy-coded scan data, which we
        // never want to read, and EOI marks the end of the file; both are
        // our cue to stop, since every tag we care about appears earlier.
        match marker {
            0x01 => continue,
            0xD0..=0xD8 => continue,
            0xD9 | 0xDA => break,
            _ => {}
        }

        let len = read_u16(&mut r)? as u64;
        if len < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad JPEG segment length",
            ));
        }
        let data_len = len - 2;

        if is_sof(marker) {
            if data_len < 5 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated SOF segment",
                ));
            }
            let mut hdr = [0u8; 5];
            r.read_exact(&mut hdr)?;
            height = u16::from_be_bytes([hdr[1], hdr[2]]) as u32;
            width = u16::from_be_bytes([hdr[3], hdr[4]]) as u32;
            discard(&mut r, data_len - 5)?;
        } else if marker == 0xE1 {
            // APP1: may be Exif, may be XMP or something else entirely.
            // Bounded by the 16-bit length field, so buffering the whole
            // segment is fine - this is metadata, not pixel data.
            let mut data = vec![0u8; data_len as usize];
            r.read_exact(&mut data)?;
            if data.len() >= 6 && &data[..6] == b"Exif\0\0" {
                exif = crate::exif::parse_ifd0(&data[6..]);
            }
        } else {
            discard(&mut r, data_len)?;
        }
    }

    Ok(ImageMetadata {
        format: Format::Jpeg,
        width,
        height,
        exif,
        text: Vec::new(),
    })
}
