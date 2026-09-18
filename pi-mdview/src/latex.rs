//! LaTeX -> Unicode, the Rust counterpart of pi's `renderLatex()`.
//!
//! pi pre-processes `$...$`, `$$...$$`, `\(...\)` and `\[...\]` math into
//! Unicode before the markdown tokens are laid out, so a terminal without
//! formula support still shows symbols. This is the same best-effort table,
//! originally written for the `pi-notes` crate in this workspace.
//!
//! Markdown structural syntax is left untouched so tables, code blocks and
//! emphasis still render.

use std::collections::HashMap;

/// Map of LaTeX commands (without the leading backslash) to Unicode.
fn greek() -> HashMap<&'static str, &'static str> {
    [
        ("alpha", "α"),
        ("beta", "β"),
        ("gamma", "γ"),
        ("delta", "δ"),
        ("epsilon", "ε"),
        ("varepsilon", "ε"),
        ("zeta", "ζ"),
        ("eta", "η"),
        ("theta", "θ"),
        ("vartheta", "ϑ"),
        ("iota", "ι"),
        ("kappa", "κ"),
        ("lambda", "λ"),
        ("mu", "μ"),
        ("nu", "ν"),
        ("xi", "ξ"),
        ("omicron", "ο"),
        ("pi", "π"),
        ("varpi", "ϖ"),
        ("rho", "ρ"),
        ("varrho", "ϱ"),
        ("sigma", "σ"),
        ("varsigma", "ς"),
        ("tau", "τ"),
        ("upsilon", "υ"),
        ("phi", "φ"),
        ("varphi", "ϕ"),
        ("chi", "χ"),
        ("psi", "ψ"),
        ("omega", "ω"),
        ("Gamma", "Γ"),
        ("Delta", "Δ"),
        ("Theta", "Θ"),
        ("Lambda", "Λ"),
        ("Xi", "Ξ"),
        ("Pi", "Π"),
        ("Sigma", "Σ"),
        ("Upsilon", "Υ"),
        ("Phi", "Φ"),
        ("Psi", "Ψ"),
        ("Omega", "Ω"),
    ]
    .into_iter()
    .collect()
}

fn symbols() -> HashMap<&'static str, &'static str> {
    [
        ("to", "→"),
        ("rightarrow", "→"),
        ("leftarrow", "←"),
        ("leftrightarrow", "↔"),
        ("Rightarrow", "⇒"),
        ("Leftarrow", "⇐"),
        ("Leftrightarrow", "⇔"),
        ("implies", "⟹"),
        ("iff", "⟺"),
        ("mapsto", "↦"),
        ("longrightarrow", "⟶"),
        ("longleftarrow", "⟵"),
        ("sum", "∑"),
        ("prod", "∏"),
        ("int", "∫"),
        ("oint", "∮"),
        ("infty", "∞"),
        ("partial", "∂"),
        ("nabla", "∇"),
        ("emptyset", "∅"),
        ("varnothing", "∅"),
        ("forall", "∀"),
        ("exists", "∃"),
        ("nexists", "∄"),
        ("leq", "≤"),
        ("geq", "≥"),
        ("le", "≤"),
        ("ge", "≥"),
        ("neq", "≠"),
        ("ne", "≠"),
        ("approx", "≈"),
        ("equiv", "≡"),
        ("sim", "∼"),
        ("propto", "∝"),
        ("ll", "≪"),
        ("gg", "≫"),
        ("pm", "±"),
        ("mp", "∓"),
        ("times", "×"),
        ("cdot", "·"),
        ("div", "÷"),
        ("ast", "∗"),
        ("star", "⋆"),
        ("circ", "∘"),
        ("bullet", "•"),
        ("oplus", "⊕"),
        ("otimes", "⊗"),
        ("in", "∈"),
        ("notin", "∉"),
        ("subseteq", "⊆"),
        ("subset", "⊂"),
        ("supseteq", "⊇"),
        ("supset", "⊃"),
        ("cup", "∪"),
        ("cap", "∩"),
        ("land", "∧"),
        ("lor", "∨"),
        ("neg", "¬"),
        ("lnot", "¬"),
        ("wedge", "∧"),
        ("vee", "∨"),
        ("perp", "⊥"),
        ("parallel", "∥"),
        ("therefore", "∴"),
        ("because", "∵"),
        ("angle", "∠"),
        ("triangle", "△"),
        ("triangleq", "≜"),
        ("cong", "≅"),
        ("simeq", "≃"),
        ("degree", "°"),
        ("prime", "′"),
        ("ldots", "…"),
        ("cdots", "⋯"),
        ("dots", "…"),
        ("ldots", "…"),
        ("hbar", "ℏ"),
        ("ell", "ℓ"),
        ("Re", "ℜ"),
        ("Im", "ℑ"),
        ("aleph", "ℵ"),
        ("eth", "ð"),
        ("mho", "℧"),
        ("wp", "℘"),
        ("pounds", "£"),
        ("euro", "€"),
        ("text", ""),
        ("mathrm", ""),
        ("mathbf", ""),
        ("mathit", ""),
        ("textrm", ""),
        ("operatorname", ""),
        ("displaystyle", ""),
        ("left", ""),
        ("right", ""),
        ("big", ""),
        ("Big", ""),
        ("bigg", ""),
        ("Bigg", ""),
        ("limits", ""),
        ("nolimits", ""),
        ("quad", "  "),
        ("qquad", "    "),
        (",", " "),
        (";", " "),
        ("!", ""),
        (":", " "),
        (" ", " "),
    ]
    .into_iter()
    .collect()
}

/// Unicode superscript for a single character (digits and a few letters).
fn superscript(ch: char) -> Option<&'static str> {
    Some(match ch {
        '0' => "⁰",
        '1' => "¹",
        '2' => "²",
        '3' => "³",
        '4' => "⁴",
        '5' => "⁵",
        '6' => "⁶",
        '7' => "⁷",
        '8' => "⁸",
        '9' => "⁹",
        'i' => "ⁱ",
        'n' => "ⁿ",
        '+' => "⁺",
        '-' => "⁻",
        '(' => "⁽",
        ')' => "⁾",
        '=' => "⁼",
        'x' => "ˣ",
        _ => return None,
    })
}

/// Unicode subscript for a single character.
fn subscript(ch: char) -> Option<&'static str> {
    Some(match ch {
        '0' => "₀",
        '1' => "₁",
        '2' => "₂",
        '3' => "₃",
        '4' => "₄",
        '5' => "₅",
        '6' => "₆",
        '7' => "₇",
        '8' => "₈",
        '9' => "₉",
        '+' => "₊",
        '-' => "₋",
        '=' => "₌",
        '(' => "₍",
        ')' => "₎",
        'a' => "ₐ",
        'e' => "ₑ",
        'i' => "ᵢ",
        'o' => "ₒ",
        'u' => "ᵤ",
        'x' => "ₓ",
        'n' => "ₙ",
        'm' => "ₘ",
        'k' => "ₖ",
        _ => return None,
    })
}

fn replace_superscripts(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '^' {
            // Handle ^{...} and ^x
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                let mut j = i + 2;
                let mut buf = String::new();
                while j < chars.len() && chars[j] != '}' {
                    buf.push(chars[j]);
                    j += 1;
                }
                let mut mapped = String::new();
                let mut ok = true;
                for c in buf.chars() {
                    match superscript(c) {
                        Some(u) => mapped.push_str(u),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && j < chars.len() {
                    out.push_str(&mapped);
                    i = j + 1;
                    continue;
                }
            } else if i + 1 < chars.len() {
                if let Some(u) = superscript(chars[i + 1]) {
                    out.push_str(u);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn replace_subscripts(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                let mut j = i + 2;
                let mut buf = String::new();
                while j < chars.len() && chars[j] != '}' {
                    buf.push(chars[j]);
                    j += 1;
                }
                let mut mapped = String::new();
                let mut ok = true;
                for c in buf.chars() {
                    match subscript(c) {
                        Some(u) => mapped.push_str(u),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && j < chars.len() {
                    out.push_str(&mapped);
                    i = j + 1;
                    continue;
                }
            } else if i + 1 < chars.len() && chars[i + 1] != ' ' {
                if let Some(u) = subscript(chars[i + 1]) {
                    out.push_str(u);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Reads a `{...}` group starting at the `{` at `open`.
/// Returns the group content and the index just past the closing `}`.
fn read_group(chars: &[char], open: usize) -> Option<(String, usize)> {
    if chars.get(open) != Some(&'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut content = String::new();
    let mut index = open;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                depth += 1;
                if depth > 1 {
                    content.push('{');
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((content, index + 1));
                }
                content.push('}');
            }
            other => content.push(other),
        }
        index += 1;
    }
    None
}

/// Convert LaTeX / symbol commands in a Markdown string to Unicode.
///
/// Only the math delimiters and backslash commands are rewritten; Markdown
/// structural syntax is preserved so tables, headings and code blocks render
/// normally.
pub fn render(input: &str) -> String {
    // Remove math delimiters first.
    let s = input
        .replace("\\[", "")
        .replace("\\]", "")
        .replace("\\(", "")
        .replace("\\)", "")
        .replace("$$", "")
        .replace('$', "");

    // Replace backslash commands.
    let greek = greek();
    let symbols = symbols();

    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
            // Gather the command name.
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_alphabetic() {
                j += 1;
            }
            let cmd: String = chars[i + 1..j].iter().collect();
            let lookup: &str = &cmd;
            let replacement = greek.get(lookup).or_else(|| symbols.get(lookup)).copied();
            if let Some(rep) = replacement {
                out.push_str(rep);
                i = j;
                continue;
            }
            // Special case for \mathbb{R} -> ℝ etc.
            if lookup == "mathbb" {
                let k = j;
                if k + 2 < chars.len() && chars[k] == '{' && chars[k + 2] == '}' {
                    let u = match chars[k + 1] {
                        'R' => Some("ℝ"),
                        'Z' => Some("ℤ"),
                        'N' => Some("ℕ"),
                        'Q' => Some("ℚ"),
                        'C' => Some("ℂ"),
                        'H' => Some("ℍ"),
                        _ => None,
                    };
                    if let Some(u) = u {
                        out.push_str(u);
                        i = k + 3;
                        continue;
                    }
                }
                // Unrecognised \mathbb{...}: drop the command, keep braces.
                i = j;
                continue;
            }
            // \frac{a}{b} -> a/b (pi lays this out as a stacked fraction).
            if lookup == "frac" || lookup == "dfrac" || lookup == "tfrac" {
                let k = j;
                if k < chars.len() && chars[k] == '{' {
                    if let Some((numerator, after_numerator)) = read_group(&chars, k) {
                        if after_numerator < chars.len() && chars[after_numerator] == '{' {
                            if let Some((denominator, after_denominator)) =
                                read_group(&chars, after_numerator)
                            {
                                out.push_str(&numerator);
                                out.push('/');
                                out.push_str(&denominator);
                                i = after_denominator;
                                continue;
                            }
                        }
                    }
                }
                i = j;
                continue;
            }
            // \sqrt{x} -> √x (drop the braces)
            if lookup == "sqrt" {
                let k = j;
                if k < chars.len() && chars[k] == '{' {
                    let mut m = k + 1;
                    while m < chars.len() && chars[m] != '}' {
                        m += 1;
                    }
                    if m < chars.len() {
                        out.push('√');
                        for c in &chars[k + 1..m] {
                            out.push(*c);
                        }
                        i = m + 1;
                        continue;
                    }
                }
                out.push('√');
                i = j;
                continue;
            }
            // Fall through: keep the backslash.
        }
        out.push(chars[i]);
        i += 1;
    }

    // Sub/superscripts must run after command replacement.
    let out = replace_superscripts(&out);
    replace_subscripts(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_latex_symbols() {
        assert_eq!(render("$\\alpha^2 + \\beta$"), "α² + β");
        assert_eq!(render("\\to \\Rightarrow"), "→ ⇒");
        assert_eq!(render("x \\in \\mathbb{R}"), "x ∈ ℝ");
        assert_eq!(render("\\sum_{i=1}^{n} i"), "∑ᵢ₌₁ⁿ i");
        assert_eq!(render("\\sqrt{x} \\neq 0"), "√x ≠ 0");
        assert_eq!(render("a \\leq b \\land c"), "a ≤ b ∧ c");
        assert_eq!(render("\\pi \\approx 3.14"), "π ≈ 3.14");
    }

    #[test]
    fn converts_frac() {
        assert_eq!(render("\\frac{n(n+1)}{2}"), "n(n+1)/2");
        assert_eq!(render("\\frac{a}{b}"), "a/b");
    }

    #[test]
    fn leaves_markdown_tables_intact() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(render(md), md);
    }
}
