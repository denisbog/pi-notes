//! `pi-mdview` binary: view a markdown file with pi's rendering.
//!
//! ```text
//! pi-mdview [FILE] [--light|--dark] [--font FILE] [--font-family NAME]
//! ```
//!
//! The bundled default font is JetBrains Mono — the font Ghostty (and therefore
//! pi) uses out of the box. `--font-family` selects a different installed
//! family, `--font` additionally loads a font file at startup.

use std::path::PathBuf;

use iced::{Font, Size};
use pi_mdview::app::Viewer;
use pi_mdview::{fonts, Rgb, Theme};

fn main() -> iced::Result {
    let mut path: Option<PathBuf> = None;
    let mut light = false;
    let mut font_family: Option<String> = None;
    let mut font_files: Vec<PathBuf> = Vec::new();
    let mut background: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => light = true,
            "--dark" => light = false,
            "--font" => match args.next() {
                Some(file) => font_files.push(PathBuf::from(file)),
                None => {
                    eprintln!("--font requires a path");
                    std::process::exit(2);
                }
            },
            "--background" => match args.next() {
                Some(color) => background = Some(color),
                None => {
                    eprintln!("--background requires a #rrggbb color");
                    std::process::exit(2);
                }
            },
            "--font-family" => match args.next() {
                Some(family) => font_family = Some(family),
                None => {
                    eprintln!("--font-family requires a name");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("pi-mdview — markdown viewer that renders like pi\n");
                println!(
                    "usage: pi-mdview [FILE] [--light|--dark] [--background '#rrggbb'] \
                     [--font FILE] [--font-family NAME]"
                );
                println!("\nWith no FILE, a demo document is shown.");
                println!("The default font is JetBrains Mono, the font Ghostty/pi render with.");
                return Ok(());
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option: {other}");
                std::process::exit(2);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }

    let theme = if light {
        Theme::pi_light()
    } else {
        Theme::pi_dark()
    };
    // The dark default is the terminal background pi sits on (Ghostty's
    // built-in `#282c34`); override it for a different terminal theme.
    let theme = match background {
        Some(color) => theme.with_background(Rgb::from_hex(&color)),
        None => theme,
    };

    let family = font_family.unwrap_or_else(|| fonts::FAMILY.to_string());
    let font = Font::with_name(Box::leak(family.into_boxed_str()));

    let mut application = iced::application(
        move || Viewer::new(path.clone(), theme, font),
        Viewer::update,
        Viewer::view,
    )
    .title(Viewer::title)
    .theme(Viewer::iced_theme)
    .subscription(Viewer::subscription)
    .default_font(font)
    .window_size(Size::new(1200.0, 860.0));

    // Always keep the bundled JetBrains Mono loaded: it is the default family
    // and it also provides pi's box-drawing/table glyphs as a fallback.
    for bytes in fonts::all() {
        application = application.font(bytes);
    }
    for file in &font_files {
        match std::fs::read(file) {
            Ok(bytes) => application = application.font(bytes),
            Err(error) => eprintln!("cannot read font {}: {error}", file.display()),
        }
    }

    application.run()
}
