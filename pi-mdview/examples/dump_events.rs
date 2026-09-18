//! Prints pulldown-cmark events with byte ranges; handy when porting pi's
//! `marked` token behaviour.
//!
//! `cargo run -p pi-mdview --no-default-features --example dump_events -- file.md`

use pulldown_cmark::{Options, Parser};

fn main() {
    let path = std::env::args().nth(1);
    let source = match path {
        Some(path) => std::fs::read_to_string(path).expect("read file"),
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .expect("read stdin");
            buffer
        }
    };

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);

    for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
        let raw = source
            .get(range.clone())
            .unwrap_or("")
            .escape_debug()
            .to_string();
        println!(
            "{:>3}..{:<3} {:?}  raw={:?}",
            range.start, range.end, event, raw
        );
    }
}
