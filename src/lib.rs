//! Read dimensions and embedded metadata out of JPEG and PNG files without
//! ever buffering the pixel data. Every parser here walks the container
//! format segment by segment and only allocates for the pieces that are
//! inherently small and bounded by the format itself (a PNG IHDR chunk, a
//! JPEG APP1 segment, a text chunk). Anything else - most importantly the
//! actual compressed image data - is discarded in fixed-size chunks as it's
//! read past, so a caller can hand this a multi-gigabyte file or a live pipe
//! and memory use stays flat.

use std::io::{self, Read};

mod exif;
mod inflate;
mod jpeg;
mod png;

pub use exif::{ExifTag, ExifValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
}

#[derive(Debug, Clone)]
pub struct ImageMetadata {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// Tags decoded from IFD0 of a JPEG APP1 Exif segment. Empty if the
    /// image has no Exif segment or its TIFF header doesn't parse.
    pub exif: Vec<ExifTag>,
    /// PNG tEXt, zTXt, and iTXt chunks as (keyword, text) pairs, with
    /// zTXt/iTXt decompressed. A chunk that fails to decompress or doesn't
    /// parse is skipped rather than surfaced as an error.
    pub text: Vec<(String, String)>,
}

/// Reads metadata from any `Read` source, detecting JPEG or PNG from the
/// leading bytes. Consumes only as much of `reader` as needed to reach the
/// end of the header data (JPEG stops at the start-of-scan marker; PNG reads
/// through to the IEND chunk without buffering IDAT).
pub fn read_metadata<R: Read>(mut reader: R) -> io::Result<ImageMetadata> {
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;

    if magic[0] == 0xFF && magic[1] == 0xD8 {
        // The first two bytes are the JPEG SOI marker; the rest of what we
        // just read is already part of the next marker, so splice it back
        // in front of the reader instead of re-reading it.
        let prefix = io::Cursor::new(magic[2..8].to_vec());
        jpeg::read(prefix.chain(reader))
    } else if magic == png::SIGNATURE {
        png::read(reader)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unrecognized image format (expected JPEG or PNG)",
        ))
    }
}

/// Reads and throws away exactly `remaining` bytes using a small fixed
/// buffer, regardless of how large `remaining` is.
pub(crate) fn discard<R: Read>(reader: &mut R, mut remaining: u64) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    while remaining > 0 {
        let chunk = remaining.min(buf.len() as u64) as usize;
        reader.read_exact(&mut buf[..chunk])?;
        remaining -= chunk as u64;
    }
    Ok(())
}
