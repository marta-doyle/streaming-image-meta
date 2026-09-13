//! Decodes the tags in IFD0 of a TIFF-structured Exif payload - the bytes
//! that follow the "Exif\0\0" marker in a JPEG APP1 segment. The Exif and
//! GPS sub-IFDs (reached via the pointer tags 0x8769 and 0x8825) are left
//! alone for now; they use their own tag numbering and mixing them into a
//! single flat list would make tag IDs ambiguous.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            ByteOrder::Little => u16::from_le_bytes(b),
            ByteOrder::Big => u16::from_be_bytes(b),
        }
    }

    fn u32(self, b: [u8; 4]) -> u32 {
        match self {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        }
    }

    fn i16(self, b: [u8; 2]) -> i16 {
        match self {
            ByteOrder::Little => i16::from_le_bytes(b),
            ByteOrder::Big => i16::from_be_bytes(b),
        }
    }

    fn i32(self, b: [u8; 4]) -> i32 {
        match self {
            ByteOrder::Little => i32::from_le_bytes(b),
            ByteOrder::Big => i32::from_be_bytes(b),
        }
    }

    fn f32(self, b: [u8; 4]) -> f32 {
        match self {
            ByteOrder::Little => f32::from_le_bytes(b),
            ByteOrder::Big => f32::from_be_bytes(b),
        }
    }

    fn f64(self, b: [u8; 8]) -> f64 {
        match self {
            ByteOrder::Little => f64::from_le_bytes(b),
            ByteOrder::Big => f64::from_be_bytes(b),
        }
    }
}

/// A decoded Exif field. Rational values are kept as (numerator,
/// denominator) pairs rather than reduced to a float, since the reduction
/// is lossy and different tags want different precision.
#[derive(Debug, Clone, PartialEq)]
pub enum ExifValue {
    Byte(Vec<u8>),
    Ascii(String),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Rational(Vec<(u32, u32)>),
    SByte(Vec<i8>),
    Undefined(Vec<u8>),
    SShort(Vec<i16>),
    SLong(Vec<i32>),
    SRational(Vec<(i32, i32)>),
    Float(Vec<f32>),
    Double(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExifTag {
    pub id: u16,
    pub value: ExifValue,
}

/// Parses IFD0 out of a raw TIFF-structured Exif payload. Returns an empty
/// list if the header doesn't look like valid TIFF or any offset in it runs
/// past the end of `data` - a photo with garbled Exif is still a valid
/// photo, and the caller cares more about width/height than a broken
/// metadata block.
pub(crate) fn parse_ifd0(data: &[u8]) -> Vec<ExifTag> {
    parse(data).unwrap_or_default()
}

fn parse(data: &[u8]) -> Option<Vec<ExifTag>> {
    if data.len() < 8 {
        return None;
    }
    let order = match &data[0..2] {
        b"II" => ByteOrder::Little,
        b"MM" => ByteOrder::Big,
        _ => return None,
    };
    if order.u16(read2(data, 2)?) != 42 {
        return None;
    }
    let ifd0_offset = order.u32(read4(data, 4)?) as usize;
    parse_ifd(data, order, ifd0_offset)
}

fn parse_ifd(data: &[u8], order: ByteOrder, offset: usize) -> Option<Vec<ExifTag>> {
    let count = order.u16(read2(data, offset)?) as usize;
    let entries_start = offset.checked_add(2)?;
    let mut tags = Vec::with_capacity(count);

    for i in 0..count {
        let entry = entries_start.checked_add(i.checked_mul(12)?)?;
        let id = order.u16(read2(data, entry)?);
        let type_code = order.u16(read2(data, entry.checked_add(2)?)?);
        let field_count = order.u32(read4(data, entry.checked_add(4)?)?) as usize;
        let inline = read4(data, entry.checked_add(8)?)?;

        if let Some(value) = read_value(data, order, type_code, field_count, inline) {
            tags.push(ExifTag { id, value });
        }
    }

    Some(tags)
}

fn elem_size(type_code: u16) -> Option<usize> {
    Some(match type_code {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// `inline` is the raw 4-byte value/offset field from the IFD entry, taken
/// verbatim (not byte-order-converted) since a value that fits in it is
/// stored left-justified as plain bytes, not as a swapped integer.
fn read_value(
    data: &[u8],
    order: ByteOrder,
    type_code: u16,
    count: usize,
    inline: [u8; 4],
) -> Option<ExifValue> {
    let size = elem_size(type_code)?;
    let total = size.checked_mul(count)?;

    let bytes: &[u8] = if total <= 4 {
        &inline[..total]
    } else {
        let offset = order.u32(inline) as usize;
        data.get(offset..offset.checked_add(total)?)?
    };

    Some(match type_code {
        1 => ExifValue::Byte(bytes.to_vec()),
        2 => {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            ExifValue::Ascii(String::from_utf8_lossy(&bytes[..end]).into_owned())
        }
        3 => ExifValue::Short(
            bytes
                .chunks_exact(2)
                .map(|c| order.u16([c[0], c[1]]))
                .collect(),
        ),
        4 => ExifValue::Long(
            bytes
                .chunks_exact(4)
                .map(|c| order.u32([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        5 => ExifValue::Rational(
            bytes
                .chunks_exact(8)
                .map(|c| {
                    (
                        order.u32([c[0], c[1], c[2], c[3]]),
                        order.u32([c[4], c[5], c[6], c[7]]),
                    )
                })
                .collect(),
        ),
        6 => ExifValue::SByte(bytes.iter().map(|&b| b as i8).collect()),
        7 => ExifValue::Undefined(bytes.to_vec()),
        8 => ExifValue::SShort(
            bytes
                .chunks_exact(2)
                .map(|c| order.i16([c[0], c[1]]))
                .collect(),
        ),
        9 => ExifValue::SLong(
            bytes
                .chunks_exact(4)
                .map(|c| order.i32([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        10 => ExifValue::SRational(
            bytes
                .chunks_exact(8)
                .map(|c| {
                    (
                        order.i32([c[0], c[1], c[2], c[3]]),
                        order.i32([c[4], c[5], c[6], c[7]]),
                    )
                })
                .collect(),
        ),
        11 => ExifValue::Float(
            bytes
                .chunks_exact(4)
                .map(|c| order.f32([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        12 => ExifValue::Double(
            bytes
                .chunks_exact(8)
                .map(|c| order.f64([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        _ => return None,
    })
}

fn read2(data: &[u8], offset: usize) -> Option<[u8; 2]> {
    data.get(offset..offset.checked_add(2)?)?.try_into().ok()
}

fn read4(data: &[u8], offset: usize) -> Option<[u8; 4]> {
    data.get(offset..offset.checked_add(4)?)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Little-endian TIFF header with IFD0 at offset 8 holding two tags:
    // Orientation (0x0112, SHORT, value 1) and Make (0x010F, ASCII, "ABC").
    fn sample_le() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"II"); // byte order
        buf.extend_from_slice(&42u16.to_le_bytes()); // magic
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
        buf.extend_from_slice(&2u16.to_le_bytes()); // entry count

        // Orientation: tag, type=SHORT, count=1, inline value=1
        buf.extend_from_slice(&0x0112u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[1, 0, 0, 0]);

        // Make: tag, type=ASCII, count=4, inline value="ABC\0"
        buf.extend_from_slice(&0x010Fu16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(b"ABC\0");

        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset
        buf
    }

    #[test]
    fn parses_inline_short_and_ascii() {
        let tags = parse_ifd0(&sample_le());
        assert_eq!(
            tags,
            vec![
                ExifTag {
                    id: 0x0112,
                    value: ExifValue::Short(vec![1]),
                },
                ExifTag {
                    id: 0x010F,
                    value: ExifValue::Ascii("ABC".to_string()),
                },
            ]
        );
    }

    #[test]
    fn parses_out_of_line_rational() {
        // A single RATIONAL (8 bytes) doesn't fit inline, so it's stored at
        // an offset and the entry's value field holds that offset instead.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"MM");
        buf.extend_from_slice(&42u16.to_be_bytes());
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes()); // one entry

        buf.extend_from_slice(&0x829Du16.to_be_bytes()); // FNumber
        buf.extend_from_slice(&5u16.to_be_bytes()); // RATIONAL
        buf.extend_from_slice(&1u32.to_be_bytes()); // count
        let value_offset = buf.len() as u32 + 4 /* next-IFD offset field */ + 4;
        buf.extend_from_slice(&value_offset.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // next IFD offset

        buf.extend_from_slice(&28u32.to_be_bytes());
        buf.extend_from_slice(&10u32.to_be_bytes());

        let tags = parse_ifd0(&buf);
        assert_eq!(
            tags,
            vec![ExifTag {
                id: 0x829D,
                value: ExifValue::Rational(vec![(28, 10)]),
            }]
        );
    }

    #[test]
    fn rejects_bad_header() {
        assert!(parse_ifd0(b"not exif").is_empty());
        assert!(parse_ifd0(&[]).is_empty());
    }
}
