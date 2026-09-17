use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::process::ExitCode;

use imgmeta::{ExifValue, Format, ImageMetadata};

fn main() -> ExitCode {
    let mut path = None;
    let mut json = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--json" => json = true,
            _ if path.is_none() => path = Some(arg),
            _ => {
                eprintln!("usage: imgmeta [--json] <path|->");
                return ExitCode::from(2);
            }
        }
    }

    let Some(path) = path else {
        eprintln!("usage: imgmeta [--json] <path|->");
        return ExitCode::from(2);
    };

    let result = if path == "-" {
        run(BufReader::new(io::stdin().lock()), json)
    } else {
        match File::open(&path) {
            Ok(f) => run(BufReader::new(f), json),
            Err(e) => {
                eprintln!("imgmeta: {}: {}", path, e);
                return ExitCode::FAILURE;
            }
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("imgmeta: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn run<R: Read>(reader: R, json: bool) -> io::Result<()> {
    let meta = imgmeta::read_metadata(reader)?;

    if json {
        println!("{}", to_json(&meta));
    } else {
        print_text(&meta);
    }

    Ok(())
}

fn print_text(meta: &ImageMetadata) {
    println!("format: {:?}", meta.format);
    println!("dimensions: {}x{}", meta.width, meta.height);

    if meta.exif.is_empty() {
        println!("exif: none");
    } else {
        println!("exif tags:");
        for tag in &meta.exif {
            println!("  0x{:04x}: {:?}", tag.id, tag.value);
        }
    }

    if meta.text.is_empty() {
        println!("text chunks: none");
    } else {
        println!("text chunks:");
        for (key, value) in &meta.text {
            println!("  {}: {}", key, value);
        }
    }
}

fn to_json(meta: &ImageMetadata) -> String {
    let format = match meta.format {
        Format::Jpeg => "jpeg",
        Format::Png => "png",
    };

    let exif = meta
        .exif
        .iter()
        .map(|tag| {
            format!(
                r#"{{"id":"0x{:04x}",{}}}"#,
                tag.id,
                exif_value_json(&tag.value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let text = meta
        .text
        .iter()
        .map(|(key, value)| {
            format!(
                r#"{{"keyword":{},"value":{}}}"#,
                json_string(key),
                json_string(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"{{"format":"{}","width":{},"height":{},"exif":[{}],"text":[{}]}}"#,
        format, meta.width, meta.height, exif, text
    )
}

/// Renders an Exif field as a `"type"` tag plus its `"value"`, since the
/// numeric variants (Short vs Long vs SRational, ...) aren't distinguishable
/// once they're all just JSON numbers.
fn exif_value_json(value: &ExifValue) -> String {
    fn nums<T: std::fmt::Display>(v: &[T]) -> String {
        v.iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
    fn ratios<T: std::fmt::Display>(v: &[(T, T)]) -> String {
        v.iter()
            .map(|(n, d)| format!("[{},{}]", n, d))
            .collect::<Vec<_>>()
            .join(",")
    }

    let (ty, values) = match value {
        ExifValue::Byte(v) => ("Byte", nums(v)),
        ExifValue::Ascii(s) => return format!(r#""type":"Ascii","value":{}"#, json_string(s)),
        ExifValue::Short(v) => ("Short", nums(v)),
        ExifValue::Long(v) => ("Long", nums(v)),
        ExifValue::Rational(v) => ("Rational", ratios(v)),
        ExifValue::SByte(v) => ("SByte", nums(v)),
        ExifValue::Undefined(v) => ("Undefined", nums(v)),
        ExifValue::SShort(v) => ("SShort", nums(v)),
        ExifValue::SLong(v) => ("SLong", nums(v)),
        ExifValue::SRational(v) => ("SRational", ratios(v)),
        ExifValue::Float(v) => ("Float", nums(v)),
        ExifValue::Double(v) => ("Double", nums(v)),
    };

    format!(r#""type":"{}","value":[{}]"#, ty, values)
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use imgmeta::ExifTag;

    #[test]
    fn escapes_control_and_special_characters() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("line\nbreak"), "\"line\\nbreak\"");
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn renders_full_metadata_as_json() {
        let meta = ImageMetadata {
            format: Format::Png,
            width: 10,
            height: 20,
            exif: vec![ExifTag {
                id: 0x0112,
                value: ExifValue::Short(vec![1]),
            }],
            text: vec![("Comment".to_string(), "hi \"there\"".to_string())],
        };

        assert_eq!(
            to_json(&meta),
            r#"{"format":"png","width":10,"height":20,"exif":[{"id":"0x0112","type":"Short","value":[1]}],"text":[{"keyword":"Comment","value":"hi \"there\""}]}"#
        );
    }
}
