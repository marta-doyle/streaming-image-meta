use std::io::{self, Read};

use crate::{discard, inflate, Format, ImageMetadata};

pub(crate) const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// tEXt/zTXt/iTXt chunks larger than this are skipped rather than buffered.
/// A well-formed image has no reason to carry megabytes of text metadata,
/// and the chunk length field is attacker-controlled input.
const MAX_TEXT_CHUNK: u64 = 1_000_000;

/// Cap on the decompressed size of a zTXt/iTXt chunk. DEFLATE can expand a
/// small input by three orders of magnitude, so the compressed-size cap
/// above isn't enough on its own to bound memory use.
const MAX_DECOMPRESSED_TEXT: usize = 20_000_000;

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
            b"zTXt" if len <= MAX_TEXT_CHUNK => {
                let mut data = vec![0u8; len as usize];
                r.read_exact(&mut data)?;
                if let Some(entry) = parse_ztxt(&data) {
                    text.push(entry);
                }
            }
            b"iTXt" if len <= MAX_TEXT_CHUNK => {
                let mut data = vec![0u8; len as usize];
                r.read_exact(&mut data)?;
                if let Some(entry) = parse_itxt(&data) {
                    text.push(entry);
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
        exif: Vec::new(),
        text,
    })
}

/// A zTXt chunk is keyword \0 compression-method compressed-text. The
/// compression method byte is always 0 (zlib/DEFLATE) per the spec; any
/// other value means a future method we don't understand.
fn parse_ztxt(data: &[u8]) -> Option<(String, String)> {
    let kw_end = data.iter().position(|&b| b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..kw_end]).into_owned();

    let rest = data.get(kw_end + 1..)?;
    let method = *rest.first()?;
    if method != 0 {
        return None;
    }
    let compressed = rest.get(1..)?;
    let bytes = inflate::zlib_decompress(compressed, MAX_DECOMPRESSED_TEXT)?;
    Some((keyword, String::from_utf8_lossy(&bytes).into_owned()))
}

/// An iTXt chunk is keyword \0 compression-flag compression-method
/// language-tag \0 translated-keyword \0 text. Text is compressed only if
/// the flag is 1, and only method 0 (zlib/DEFLATE) is defined.
fn parse_itxt(data: &[u8]) -> Option<(String, String)> {
    let kw_end = data.iter().position(|&b| b == 0)?;
    let keyword = String::from_utf8_lossy(&data[..kw_end]).into_owned();

    let rest = data.get(kw_end + 1..)?;
    let compressed_flag = *rest.first()?;
    let compression_method = *rest.get(1)?;
    let after_flags = rest.get(2..)?;

    let lang_end = after_flags.iter().position(|&b| b == 0)?;
    let after_lang = after_flags.get(lang_end + 1..)?;

    let tr_end = after_lang.iter().position(|&b| b == 0)?;
    let text_bytes = after_lang.get(tr_end + 1..)?;

    let value = match compressed_flag {
        0 => String::from_utf8_lossy(text_bytes).into_owned(),
        1 if compression_method == 0 => {
            let bytes = inflate::zlib_decompress(text_bytes, MAX_DECOMPRESSED_TEXT)?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => return None,
    };

    Some((keyword, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid zlib stream (a single uncompressed "stored"
    /// DEFLATE block) wrapping `text`, without needing a real compressor.
    fn zlib_stored(text: &[u8]) -> Vec<u8> {
        let mut stream = vec![0x78, 0x01];
        stream.push(0x01); // bfinal=1, btype=00 (stored)
        stream.extend_from_slice(&(text.len() as u16).to_le_bytes());
        stream.extend_from_slice(&(!(text.len() as u16)).to_le_bytes());
        stream.extend_from_slice(text);
        stream.extend_from_slice(&[0, 0, 0, 0]); // adler32, unchecked
        stream
    }

    #[test]
    fn parses_ztxt() {
        let mut data = b"Comment\0\0".to_vec();
        data.extend_from_slice(&zlib_stored(b"hello world"));

        let (keyword, value) = parse_ztxt(&data).unwrap();
        assert_eq!(keyword, "Comment");
        assert_eq!(value, "hello world");
    }

    #[test]
    fn rejects_ztxt_unknown_method() {
        let mut data = b"Comment\0\x01".to_vec();
        data.extend_from_slice(&zlib_stored(b"hello"));
        assert!(parse_ztxt(&data).is_none());
    }

    #[test]
    fn parses_itxt_uncompressed() {
        let mut data = b"Title\0".to_vec();
        data.push(0); // compression flag: not compressed
        data.push(0); // compression method
        data.extend_from_slice(b"en\0"); // language tag
        data.extend_from_slice("Titre\0".as_bytes()); // translated keyword
        data.extend_from_slice("caf\u{e9}".as_bytes());

        let (keyword, value) = parse_itxt(&data).unwrap();
        assert_eq!(keyword, "Title");
        assert_eq!(value, "caf\u{e9}");
    }

    #[test]
    fn parses_itxt_compressed() {
        let mut data = b"Title\0".to_vec();
        data.push(1); // compression flag: compressed
        data.push(0); // compression method: zlib
        data.extend_from_slice(b"\0"); // empty language tag
        data.extend_from_slice(b"\0"); // empty translated keyword
        data.extend_from_slice(&zlib_stored(b"compressed text"));

        let (keyword, value) = parse_itxt(&data).unwrap();
        assert_eq!(keyword, "Title");
        assert_eq!(value, "compressed text");
    }
}
