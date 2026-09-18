//! Renders a markdown file to stdout using `pi-mdview`.
//!
//! `cargo run -p pi-mdview --no-default-features --example render_file -- <width> <file> [--light] [--ansi]`
//!
//! With `--ansi` the styled lines are emitted as truecolor SGR sequences
//! (useful for eyeballing the result in a terminal).

use std::io::Read;

use pi_mdview::{RenderOptions, Renderer, Theme};

fn main() {
    let mut args = std::env::args().skip(1);
    let width: usize = args
        .next()
        .unwrap_or_else(|| "80".to_string())
        .parse()
        .expect("width");
    let mut path = None;
    let mut theme = Theme::pi_dark();
    let mut ansi = false;
    let mut padding = 0usize;

    for arg in args {
        match arg.as_str() {
            "--light" => theme = Theme::pi_light(),
            "--dark" => theme = Theme::pi_dark(),
            "--ansi" => ansi = true,
            "--pad" => padding = 2,
            other => path = Some(other.to_string()),
        }
    }

    let source = match path {
        Some(path) => std::fs::read_to_string(path).expect("read file"),
        None => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .expect("read stdin");
            buffer
        }
    };

    let renderer = Renderer::new(theme).options(RenderOptions {
        padding_x: padding,
        padding_y: 0,
        pad_to_width: true,
        render_latex: true,
    });

    for line in renderer.render(&source, width) {
        if ansi {
            println!("{}", line.to_ansi());
        } else {
            println!("{}", line.plain());
        }
    }
}
