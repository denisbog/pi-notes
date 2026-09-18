//! Prints syntect scope names per token; used to align the pi theme selectors.
//!
//! `cargo run -p pi-mdview --no-default-features --features syntax --example dump_scopes`

use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

fn main() {
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let cases: &[(&str, &str)] = &[
        ("rust", "fn main() { let x: u32 = 42; }"),
        ("python", "def greet(name: str) -> None:\n    pass"),
        ("javascript", "const x = 1; function f(a) {}"),
        ("c", "int main(void) { return 0; }"),
    ];

    for (lang, code) in cases {
        let Some(syntax) = syntaxes.find_syntax_by_token(lang) else {
            continue;
        };
        println!("=== {lang} ({})", syntax.name);
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        for line in code.lines() {
            let ops = state.parse_line(line, &syntaxes).unwrap_or_default();
            let mut last = 0usize;
            for (offset, op) in ops {
                if offset > last {
                    let text = &line[last..offset];
                    if !text.trim().is_empty() {
                        let scopes: Vec<String> =
                            stack.as_slice().iter().map(|s| s.to_string()).collect();
                        println!("  {text:?}  {}", scopes.join(" "));
                    }
                }
                stack.apply(&op).ok();
                last = offset;
            }
            if last < line.len() {
                let text = &line[last..];
                if !text.trim().is_empty() {
                    let scopes: Vec<String> =
                        stack.as_slice().iter().map(|s| s.to_string()).collect();
                    println!("  {text:?}  {}", scopes.join(" "));
                }
            }
        }
    }
}
