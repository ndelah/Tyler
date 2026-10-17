//! Hyprland-style key chord parsing (`"SUPER + SHIFT + LEFT"`) and the
//! mapping from key names to macOS virtual key codes (`kVK_*` from
//! `HIToolbox/Events.h`). Kept platform-independent so it can be tested on
//! any host; the macOS event tap matches on the resulting codes.

use std::fmt;

/// Modifier bit set. Values are local; the macOS layer converts
/// `CGEventFlags` into this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);
    pub const SHIFT: Modifiers = Modifiers(1);
    pub const CTRL: Modifiers = Modifiers(1 << 1);
    pub const ALT: Modifiers = Modifiers(1 << 2);
    pub const CMD: Modifiers = Modifiers(1 << 3);

    pub fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Parses a single physical modifier name (not `SUPER`).
    pub fn parse_physical(name: &str) -> Option<Modifiers> {
        match name.to_ascii_uppercase().as_str() {
            "SHIFT" => Some(Self::SHIFT),
            "CTRL" | "CONTROL" => Some(Self::CTRL),
            "ALT" | "OPT" | "OPTION" => Some(Self::ALT),
            "CMD" | "COMMAND" => Some(Self::CMD),
            _ => None,
        }
    }

    /// Parses what `SUPER` means on this Mac, e.g. `"cmd"` or `"ctrl+alt"`.
    pub fn parse_combo(spec: &str) -> Option<Modifiers> {
        spec.split('+')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .try_fold(Modifiers::NONE, |acc, part| {
                Self::parse_physical(part).map(|m| acc | m)
            })
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Modifiers;
    fn bitor(self, rhs: Modifiers) -> Modifiers {
        Modifiers(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Modifiers) {
        self.0 |= rhs.0;
    }
}

impl fmt::Display for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        for (m, name) in [
            (Self::CTRL, "CTRL"),
            (Self::ALT, "ALT"),
            (Self::SHIFT, "SHIFT"),
            (Self::CMD, "CMD"),
        ] {
            if self.contains(m) {
                parts.push(name);
            }
        }
        write!(f, "{}", parts.join(" + "))
    }
}

/// A resolved hotkey: macOS virtual key code plus exact modifier set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub keycode: u16,
    pub mods: Modifiers,
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = key_name(self.keycode).unwrap_or("?");
        if self.mods.is_empty() {
            write!(f, "{key}")
        } else {
            write!(f, "{} + {key}", self.mods)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordError {
    Empty,
    UnknownModifier(String),
    UnknownKey(String),
    MissingKey,
}

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordError::Empty => write!(f, "empty key chord"),
            ChordError::UnknownModifier(m) => write!(f, "unknown modifier `{m}`"),
            ChordError::UnknownKey(k) => write!(f, "unknown key `{k}`"),
            ChordError::MissingKey => write!(f, "chord has modifiers but no key"),
        }
    }
}

impl std::error::Error for ChordError {}

/// Parses `"SUPER + SHIFT + code:10"` style chords. `super_mods` is what the
/// `SUPER`/`MOD`/`$mod` token expands to.
pub fn parse_chord(spec: &str, super_mods: Modifiers) -> Result<Chord, ChordError> {
    let parts: Vec<&str> = spec
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let Some((key, mods)) = parts.split_last() else {
        return Err(ChordError::Empty);
    };
    let mut modifiers = Modifiers::NONE;
    for m in mods {
        let upper = m.to_ascii_uppercase();
        modifiers |= match upper.as_str() {
            "SUPER" | "MOD" | "$MOD" | "MAINMOD" | "$MAINMOD" => super_mods,
            _ => Modifiers::parse_physical(m)
                .ok_or_else(|| ChordError::UnknownModifier((*m).to_string()))?,
        };
    }
    if Modifiers::parse_physical(key).is_some() || key.eq_ignore_ascii_case("SUPER") {
        return Err(ChordError::MissingKey);
    }
    let keycode = keycode_for(key).ok_or_else(|| ChordError::UnknownKey((*key).to_string()))?;
    Ok(Chord {
        keycode,
        mods: modifiers,
    })
}

/// Hyprland binds digits as X11 keycodes (`code:10` = 1 … `code:19` = 0,
/// `code:20` = minus, `code:21` = equal) so they work on any layout.
fn xkb_code_to_name(code: u16) -> Option<&'static str> {
    Some(match code {
        10 => "1",
        11 => "2",
        12 => "3",
        13 => "4",
        14 => "5",
        15 => "6",
        16 => "7",
        17 => "8",
        18 => "9",
        19 => "0",
        20 => "MINUS",
        21 => "EQUAL",
        34 => "BRACKETLEFT",
        35 => "BRACKETRIGHT",
        49 => "GRAVE",
        _ => return None,
    })
}

/// (canonical name, aliases, macOS kVK code)
const KEYS: &[(&str, &[&str], u16)] = &[
    ("A", &[], 0x00),
    ("S", &[], 0x01),
    ("D", &[], 0x02),
    ("F", &[], 0x03),
    ("H", &[], 0x04),
    ("G", &[], 0x05),
    ("Z", &[], 0x06),
    ("X", &[], 0x07),
    ("C", &[], 0x08),
    ("V", &[], 0x09),
    ("B", &[], 0x0B),
    ("Q", &[], 0x0C),
    ("W", &[], 0x0D),
    ("E", &[], 0x0E),
    ("R", &[], 0x0F),
    ("Y", &[], 0x10),
    ("T", &[], 0x11),
    ("1", &[], 0x12),
    ("2", &[], 0x13),
    ("3", &[], 0x14),
    ("4", &[], 0x15),
    ("6", &[], 0x16),
    ("5", &[], 0x17),
    ("EQUAL", &["="], 0x18),
    ("9", &[], 0x19),
    ("7", &[], 0x1A),
    ("MINUS", &["-"], 0x1B),
    ("8", &[], 0x1C),
    ("0", &[], 0x1D),
    ("BRACKETRIGHT", &["]", "RIGHTBRACKET"], 0x1E),
    ("O", &[], 0x1F),
    ("U", &[], 0x20),
    ("BRACKETLEFT", &["[", "LEFTBRACKET"], 0x21),
    ("I", &[], 0x22),
    ("P", &[], 0x23),
    ("RETURN", &["ENTER"], 0x24),
    ("L", &[], 0x25),
    ("J", &[], 0x26),
    ("APOSTROPHE", &["'", "QUOTE"], 0x27),
    ("K", &[], 0x28),
    ("SEMICOLON", &[";"], 0x29),
    ("BACKSLASH", &["\\"], 0x2A),
    ("COMMA", &[","], 0x2B),
    ("SLASH", &["/"], 0x2C),
    ("N", &[], 0x2D),
    ("M", &[], 0x2E),
    ("PERIOD", &["."], 0x2F),
    ("TAB", &[], 0x30),
    ("SPACE", &[], 0x31),
    ("GRAVE", &["`"], 0x32),
    ("BACKSPACE", &[], 0x33),
    ("ESCAPE", &["ESC"], 0x35),
    ("F17", &[], 0x40),
    ("F18", &[], 0x4F),
    ("F19", &[], 0x50),
    ("F20", &[], 0x5A),
    ("F5", &[], 0x60),
    ("F6", &[], 0x61),
    ("F7", &[], 0x62),
    ("F3", &[], 0x63),
    ("F8", &[], 0x64),
    ("F9", &[], 0x65),
    ("F11", &[], 0x67),
    ("F13", &["PRINT"], 0x69),
    ("F16", &[], 0x6A),
    ("F14", &[], 0x6B),
    ("F10", &[], 0x6D),
    ("F12", &[], 0x6F),
    ("F15", &[], 0x71),
    ("HOME", &[], 0x73),
    ("PAGEUP", &["PRIOR"], 0x74),
    ("DELETE", &["FORWARDDELETE"], 0x75),
    ("F4", &[], 0x76),
    ("END", &[], 0x77),
    ("F2", &[], 0x78),
    ("PAGEDOWN", &["NEXT"], 0x79),
    ("F1", &[], 0x7A),
    ("LEFT", &[], 0x7B),
    ("RIGHT", &[], 0x7C),
    ("DOWN", &[], 0x7D),
    ("UP", &[], 0x7E),
];

/// Resolves a key name (case-insensitive, Hyprland/xkb names accepted).
pub fn keycode_for(name: &str) -> Option<u16> {
    if let Some(code) = name.strip_prefix("code:") {
        return xkb_code_to_name(code.parse().ok()?).and_then(keycode_for);
    }
    let upper = name.to_ascii_uppercase();
    KEYS.iter()
        .find(|(canonical, aliases, _)| *canonical == upper || aliases.iter().any(|a| *a == upper))
        .map(|(_, _, code)| *code)
}

pub fn key_name(keycode: u16) -> Option<&'static str> {
    KEYS.iter()
        .find(|(_, _, c)| *c == keycode)
        .map(|(n, _, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_super_chords() {
        let c = parse_chord("SUPER + SHIFT + LEFT", Modifiers::ALT).unwrap();
        assert_eq!(c.keycode, 0x7B);
        assert_eq!(c.mods, Modifiers::ALT | Modifiers::SHIFT);
    }

    #[test]
    fn super_can_expand_to_multiple_modifiers() {
        let sup = Modifiers::parse_combo("ctrl+alt").unwrap();
        let c = parse_chord("SUPER + W", sup).unwrap();
        assert_eq!(c.mods, Modifiers::CTRL | Modifiers::ALT);
        assert_eq!(c.keycode, 0x0D);
    }

    #[test]
    fn hyprland_digit_codes_map_to_number_row() {
        assert_eq!(keycode_for("code:10"), keycode_for("1"));
        assert_eq!(keycode_for("code:19"), keycode_for("0"));
        assert_eq!(keycode_for("code:20"), keycode_for("MINUS"));
        assert_eq!(keycode_for("code:21"), keycode_for("EQUAL"));
        assert_eq!(keycode_for("code:99"), None);
    }

    #[test]
    fn names_are_case_insensitive_with_aliases() {
        assert_eq!(keycode_for("return"), Some(0x24));
        assert_eq!(keycode_for("Enter"), Some(0x24));
        assert_eq!(keycode_for("grave"), Some(0x32));
        assert_eq!(keycode_for("`"), Some(0x32));
    }

    #[test]
    fn rejects_bad_chords() {
        assert_eq!(parse_chord("", Modifiers::CMD), Err(ChordError::Empty));
        assert_eq!(
            parse_chord("SUPER + SHIFT", Modifiers::CMD),
            Err(ChordError::MissingKey)
        );
        assert!(matches!(
            parse_chord("HYPER + A", Modifiers::CMD),
            Err(ChordError::UnknownModifier(_))
        ));
        assert!(matches!(
            parse_chord("SUPER + NOPE", Modifiers::CMD),
            Err(ChordError::UnknownKey(_))
        ));
    }

    #[test]
    fn keycodes_are_unique() {
        let mut codes: Vec<u16> = KEYS.iter().map(|k| k.2).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len());
    }

    #[test]
    fn display_round_trip() {
        let c = parse_chord("SUPER + SHIFT + RETURN", Modifiers::CMD).unwrap();
        assert_eq!(c.to_string(), "SHIFT + CMD + RETURN");
        assert_eq!(parse_chord(&c.to_string(), Modifiers::NONE).unwrap(), c);
    }
}

/// Tracks consumed physical keys so releasing modifiers before the key does
/// not leak an orphan key-up to the foreground application.
#[derive(Default)]
pub struct ConsumedKeys(std::collections::HashSet<u16>);

impl ConsumedKeys {
    pub fn filter(&mut self, keycode: u16, down: bool, bound: bool) -> bool {
        if down {
            if bound {
                self.0.insert(keycode);
            }
            self.0.contains(&keycode)
        } else {
            self.0.remove(&keycode)
        }
    }
}

#[cfg(test)]
mod consumed_tests {
    use super::*;

    #[test]
    fn release_is_consumed_after_modifiers_or_config_change() {
        let mut keys = ConsumedKeys::default();
        assert!(keys.filter(12, true, true));
        assert!(keys.filter(12, true, false)); // repeat after modifier release
        assert!(keys.filter(12, false, false));
        assert!(!keys.filter(12, true, false)); // next ordinary press passes
        assert!(!keys.filter(12, false, false));
    }

    #[test]
    fn unmatched_release_passes_even_if_modifiers_now_match() {
        let mut keys = ConsumedKeys::default();
        assert!(!keys.filter(13, true, false));
        assert!(!keys.filter(13, false, true));
    }
}
