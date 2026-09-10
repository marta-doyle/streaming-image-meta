# imgmeta

Pull dimensions and embedded tags out of JPEG and PNG files without reading
the whole file into memory first.

Most tools that want "just the width and the Exif orientation" end up
decoding the entire image, or shelling out to something that does, because
that's the easy way to get at a container format's header. That's fine for a
one-off script, but it falls apart the moment the input is a 200 MB scan
from a phone, a stream coming off a slow upload, or a pipe with no seek
support at all. This is a small Rust library (plus a thin CLI) that instead
walks the container structure segment by segment - JPEG markers, PNG chunks
- and only ever buffers the pieces that are inherently small and bounded by
the format itself. The actual pixel data is skipped in fixed 8 KB reads and
never touched.

## What it reads today

- JPEG: width, height (from the SOF segment), and the raw Exif payload out
  of the APP1 segment, if present. The Exif TIFF structure itself isn't
  decoded into tags yet - see the roadmap below.
- PNG: width, height (from IHDR), and any `tEXt` chunks as key/value pairs.

Anything else in the file - IDAT, JPEG scan data, unrelated chunks - is read
past and discarded, never allocated.

## Library usage

```rust
use std::fs::File;
use std::io::BufReader;

fn main() -> std::io::Result<()> {
    let file = File::open("photo.jpg")?;
    let meta = imgmeta::read_metadata(BufReader::new(file))?;

    println!("{}x{}", meta.width, meta.height);
    if let Some(exif) = &meta.exif {
        println!("exif payload: {} bytes", exif.len());
    }

    Ok(())
}
```

`read_metadata` takes anything that implements `std::io::Read`, so it works
the same way against a file, a `TcpStream`, or stdin - there's no
requirement to seek.

## CLI usage

```
$ cargo run -- photo.jpg
format: Jpeg
dimensions: 4032x3024
exif: 2114 bytes (raw, not yet decoded)
text chunks: none

$ cat screenshot.png | cargo run -- -
format: Png
dimensions: 1920x1080
exif: none
text chunks:
  Software: some editor
```

Pass `-` as the path to read from stdin instead of a file.

## Status

Early skeleton. Dimensions and raw Exif/text extraction work; the Exif IFD
itself is handed back as opaque bytes for now. See the roadmap in the repo
for what's planned next.

## License

MIT, see LICENSE.
