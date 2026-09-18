//! The terminal font pi renders with.
//!
//! pi has no font of its own: it draws its UI in the terminal's font, which is
//! whatever the terminal emulator is configured with. This machine runs
//! **Ghostty** with an empty config, and Ghostty's built-in default font is
//! **JetBrains Mono** (Ghostty embeds it). So to match pi's rendering exactly,
//! the viewer bundles the same font and uses it as the default family.
//!
//! JetBrains Mono is licensed under the SIL Open Font License 1.1
//! (see `fonts/OFL.txt`); its advance width is exactly 0.6 em, which is why
//! the viewer's fit-to-window column estimate uses that ratio.

use iced::Font;

/// Family name as declared in the bundled `name` tables.
pub const FAMILY: &str = "JetBrains Mono";

pub const REGULAR: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");
pub const BOLD: &[u8] = include_bytes!("../fonts/JetBrainsMono-Bold.ttf");
pub const ITALIC: &[u8] = include_bytes!("../fonts/JetBrainsMono-Italic.ttf");
pub const BOLD_ITALIC: &[u8] = include_bytes!("../fonts/JetBrainsMono-BoldItalic.ttf");

/// All bundled faces, for iced's font loader.
pub fn all() -> [&'static [u8]; 4] {
    [REGULAR, BOLD, ITALIC, BOLD_ITALIC]
}

/// The font to use for document text and UI chrome.
pub fn monospace() -> Font {
    Font::with_name(FAMILY)
}

/// Bold variant of the bundled family.
pub fn bold() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..monospace()
    }
}

/// Italic variant of the bundled family.
pub fn italic() -> Font {
    Font {
        style: iced::font::Style::Italic,
        ..monospace()
    }
}

/// Bold + italic variant of the bundled family.
pub fn bold_italic() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        style: iced::font::Style::Italic,
        ..monospace()
    }
}

#[cfg(test)]
mod tests {
    /// The `FAMILY` constant must match the name table of every bundled face,
    /// otherwise iced/cosmic-text silently falls back to a system font. Names
    /// are stored as UTF-16BE in the TTF `name` table.
    #[test]
    fn bundled_family_name_matches_constant() {
        let needle: Vec<u8> = super::FAMILY
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();

        for face in super::all() {
            assert!(
                face.windows(needle.len()).any(|window| window == needle),
                "{:?} not found in a bundled face",
                super::FAMILY
            );
        }
    }

    #[test]
    fn faces_are_distinct() {
        let faces = super::all();
        for (index, face) in faces.iter().enumerate() {
            for other in faces.iter().skip(index + 1) {
                assert_ne!(face, other, "bundled faces must be distinct");
            }
        }
    }
}
