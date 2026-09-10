use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: imgmeta <path|->");
            return ExitCode::from(2);
        }
    };

    let result = if path == "-" {
        run(BufReader::new(io::stdin().lock()))
    } else {
        match File::open(&path) {
            Ok(f) => run(BufReader::new(f)),
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

fn run<R: Read>(reader: R) -> io::Result<()> {
    let meta = imgmeta::read_metadata(reader)?;

    println!("format: {:?}", meta.format);
    println!("dimensions: {}x{}", meta.width, meta.height);

    match &meta.exif {
        Some(bytes) => println!("exif: {} bytes (raw, not yet decoded)", bytes.len()),
        None => println!("exif: none"),
    }

    if meta.text.is_empty() {
        println!("text chunks: none");
    } else {
        println!("text chunks:");
        for (key, value) in &meta.text {
            println!("  {}: {}", key, value);
        }
    }

    Ok(())
}
