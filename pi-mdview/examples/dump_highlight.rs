//! Prints syntax spans for a snippet; useful for checking the generated theme.
//!
//! `cargo run -p pi-mdview --no-default-features --features syntax --example dump_highlight`

use pi_mdview::{highlight::Highlighter, Theme};

fn main() {
    let theme = Theme::pi_dark();
    let highlighter = Highlighter::new(&theme).expect("highlighter");

    let snippets: &[(&str, &str)] = &[
        (
            "rust",
            "fn main() {\n    let x: u32 = 42; // comment\n    println!(\"{x}\");\n}\n",
        ),
        (
            "python",
            "def greet(name: str) -> None:\n    print(f\"hi {name}\")\n",
        ),
        ("javascript", "const x = 1; // hi\n"),
    ];

    for (lang, code) in snippets {
        println!("--- {lang} (supported: {})", highlighter.supports(lang));
        for spans in highlighter.highlight(code, Some(lang)) {
            for span in spans {
                println!("  {:?} {:?}", span.style.fg, span.text);
            }
        }
    }
}
