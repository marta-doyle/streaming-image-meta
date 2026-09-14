//! A minimal DEFLATE (RFC 1951) and zlib (RFC 1950) decompressor, written
//! from scratch because the project takes on no third-party dependencies and
//! the standard library has no inflate. Only what PNG's zTXt/iTXt chunks
//! need: decompress a self-contained byte slice into a bounded output
//! buffer. No streaming, no dictionary support, no compression side.

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bitbuf: u32,
    bitcount: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            bitbuf: 0,
            bitcount: 0,
        }
    }

    /// Reads `n` bits (n <= 16) packed least-significant-bit first, which is
    /// how every DEFLATE field except Huffman codes themselves is packed.
    fn bits(&mut self, n: u32) -> Option<u32> {
        while self.bitcount < n {
            let byte = *self.data.get(self.pos)?;
            self.pos += 1;
            self.bitbuf |= (byte as u32) << self.bitcount;
            self.bitcount += 8;
        }
        let value = self.bitbuf & ((1u32 << n) - 1);
        self.bitbuf >>= n;
        self.bitcount -= n;
        Some(value)
    }

    /// Discards any bits buffered but not yet consumed from the current
    /// byte, moving the reader to the next byte boundary.
    fn align_to_byte(&mut self) {
        self.bitbuf = 0;
        self.bitcount = 0;
    }

    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn read_u16_le(&mut self) -> Option<u16> {
        let b = self.read_bytes(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
}

/// A canonical Huffman decoder built from a list of per-symbol code
/// lengths, following the counting/offset construction from RFC 1951
/// section 3.2.2 rather than building an explicit tree.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;

        let mut offsets = [0u16; 16];
        for i in 1..16 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let l = l as usize;
                symbols[offsets[l] as usize] = sym as u16;
                offsets[l] += 1;
            }
        }

        Huffman { counts, symbols }
    }

    /// Decodes one symbol. Huffman codes are packed most-significant-bit
    /// first (the one exception to DEFLATE's usual bit order), so the code
    /// value is built up by reading a bit at a time and comparing against
    /// the first code of each length, per the canonical decode algorithm.
    fn decode(&self, br: &mut BitReader) -> Option<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;

        for len in 1..16usize {
            code |= br.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return Some(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn fixed_lit_tree() -> Huffman {
    let mut lengths = [0u8; 288];
    lengths[0..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..288].fill(8);
    Huffman::build(&lengths)
}

fn fixed_dist_tree() -> Huffman {
    Huffman::build(&[5u8; 30])
}

fn read_code_lengths(br: &mut BitReader, cl_tree: &Huffman, total: usize) -> Option<Vec<u8>> {
    let mut lengths = Vec::with_capacity(total);
    let mut prev = 0u8;

    while lengths.len() < total {
        match cl_tree.decode(br)? {
            sym @ 0..=15 => {
                lengths.push(sym as u8);
                prev = sym as u8;
            }
            16 => {
                if lengths.is_empty() {
                    return None;
                }
                let repeat = 3 + br.bits(2)?;
                for _ in 0..repeat {
                    if lengths.len() >= total {
                        return None;
                    }
                    lengths.push(prev);
                }
            }
            17 => {
                let repeat = 3 + br.bits(3)?;
                for _ in 0..repeat {
                    if lengths.len() >= total {
                        return None;
                    }
                    lengths.push(0);
                }
                prev = 0;
            }
            18 => {
                let repeat = 11 + br.bits(7)?;
                for _ in 0..repeat {
                    if lengths.len() >= total {
                        return None;
                    }
                    lengths.push(0);
                }
                prev = 0;
            }
            _ => return None,
        }
    }

    Some(lengths)
}

fn read_dynamic_tables(br: &mut BitReader) -> Option<(Huffman, Huffman)> {
    let hlit = 257 + br.bits(5)? as usize;
    let hdist = 1 + br.bits(5)? as usize;
    let hclen = 4 + br.bits(4)? as usize;

    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        cl_lengths[CODE_LENGTH_ORDER[i]] = br.bits(3)? as u8;
    }
    let cl_tree = Huffman::build(&cl_lengths);

    let lengths = read_code_lengths(br, &cl_tree, hlit + hdist)?;
    let lit_tree = Huffman::build(&lengths[..hlit]);
    let dist_tree = Huffman::build(&lengths[hlit..]);
    Some((lit_tree, dist_tree))
}

fn inflate_block(
    br: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
    max_output: usize,
) -> Option<()> {
    loop {
        let sym = lit.decode(br)?;
        if sym < 256 {
            if out.len() >= max_output {
                return None;
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Some(());
        } else {
            let idx = (sym - 257) as usize;
            let base = *LENGTH_BASE.get(idx)? as usize;
            let extra = *LENGTH_EXTRA.get(idx)? as u32;
            let length = base + br.bits(extra)? as usize;

            let dsym = dist.decode(br)? as usize;
            let dbase = *DIST_BASE.get(dsym)? as usize;
            let dextra = *DIST_EXTRA.get(dsym)? as u32;
            let distance = dbase + br.bits(dextra)? as usize;

            if distance == 0 || distance > out.len() || out.len() + length > max_output {
                return None;
            }
            for _ in 0..length {
                let b = out[out.len() - distance];
                out.push(b);
            }
        }
    }
}

/// Inflates a raw (headerless) DEFLATE stream, stopping at the first final
/// block and refusing to grow the output past `max_output` bytes - a small
/// compressed chunk can expand enormously, and this is decompressing
/// attacker-controlled input.
fn inflate(data: &[u8], max_output: usize) -> Option<Vec<u8>> {
    let mut br = BitReader::new(data);
    let mut out = Vec::new();
    let fixed_lit = fixed_lit_tree();
    let fixed_dist = fixed_dist_tree();

    loop {
        let final_block = br.bits(1)? != 0;
        let block_type = br.bits(2)?;

        match block_type {
            0 => {
                br.align_to_byte();
                let len = br.read_u16_le()?;
                let nlen = br.read_u16_le()?;
                if len != !nlen {
                    return None;
                }
                let bytes = br.read_bytes(len as usize)?;
                if out.len() + bytes.len() > max_output {
                    return None;
                }
                out.extend_from_slice(bytes);
            }
            1 => inflate_block(&mut br, &fixed_lit, &fixed_dist, &mut out, max_output)?,
            2 => {
                let (lit_tree, dist_tree) = read_dynamic_tables(&mut br)?;
                inflate_block(&mut br, &lit_tree, &dist_tree, &mut out, max_output)?;
            }
            _ => return None,
        }

        if final_block {
            return Some(out);
        }
    }
}

/// Decompresses a zlib-wrapped (RFC 1950) DEFLATE stream, as used by PNG's
/// zTXt and compressed iTXt chunks. The trailing Adler-32 checksum is not
/// verified: this is metadata extraction, not integrity checking, and a
/// corrupt checksum on otherwise well-formed text is not worth rejecting.
pub(crate) fn zlib_decompress(data: &[u8], max_output: usize) -> Option<Vec<u8>> {
    if data.len() < 2 {
        return None;
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0F != 8 {
        return None; // not the DEFLATE compression method
    }
    if u16::from_be_bytes([cmf, flg]) % 31 != 0 {
        return None; // header check bits don't match
    }
    if flg & 0x20 != 0 {
        return None; // preset dictionary not supported
    }
    inflate(&data[2..], max_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zlib_stored(text: &[u8]) -> Vec<u8> {
        let mut stream = vec![0x78, 0x01]; // valid zlib header, no dictionary
        stream.push(0x01); // bfinal=1, btype=00 (stored)
        stream.extend_from_slice(&(text.len() as u16).to_le_bytes());
        stream.extend_from_slice(&(!(text.len() as u16)).to_le_bytes());
        stream.extend_from_slice(text);
        stream.extend_from_slice(&[0, 0, 0, 0]); // adler32, unchecked
        stream
    }

    #[test]
    fn decompresses_stored_block() {
        let stream = zlib_stored(b"hello");
        assert_eq!(zlib_decompress(&stream, 1024).unwrap(), b"hello");
    }

    #[test]
    fn enforces_output_cap() {
        let stream = zlib_stored(b"hello");
        assert!(zlib_decompress(&stream, 3).is_none());
    }

    #[test]
    fn rejects_non_deflate_method() {
        let stream = [0x79, 0x00, 0, 0];
        assert!(zlib_decompress(&stream, 1024).is_none());
    }

    #[test]
    fn rejects_preset_dictionary() {
        // FDICT bit set, with FLG otherwise chosen so the header checksum
        // still passes - isolates the failure to the FDICT check.
        let stream = [0x78, 0x20, 0, 0];
        assert!(zlib_decompress(&stream, 1024).is_none());
    }

    #[test]
    fn rejects_truncated_stream() {
        assert!(zlib_decompress(&[0x78, 0x01, 0x01], 1024).is_none());
    }

    #[test]
    fn rejects_empty_input() {
        assert!(zlib_decompress(&[], 1024).is_none());
        assert!(zlib_decompress(&[0x78], 1024).is_none());
    }
}
