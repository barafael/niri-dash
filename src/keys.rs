//! A key press as the dashboard sees it: the key, and which modifiers were down. The same type
//! comes from GPUI's keystrokes, from the stress run's key names (`"ctrl+shift+H"`) and from the
//! tests, so one handler serves all three.

use gpui::Keystroke;

/// What was pressed. Letters are the character typed, so Shift+j is `Char('J')`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Escape,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Shift,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Char(char),
    Named(Named),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Key {
    pub const fn plain(code: Code) -> Self {
        Self {
            code,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        }
    }

    pub const fn named(n: Named) -> Self {
        Self::plain(Code::Named(n))
    }

    pub const fn char(c: char) -> Self {
        Self::plain(Code::Char(c))
    }

    /// The key GPUI reports. `None` for what isn't a key the dashboard has a use for (a lone
    /// modifier is `Named::Shift`, so that the hints can tell one from a letter).
    pub fn from_keystroke(k: &Keystroke) -> Self {
        let m = &k.modifiers;
        let code = match k.key.as_str() {
            "escape" => Code::Named(Named::Escape),
            "enter" => Code::Named(Named::Enter),
            "tab" if m.shift => Code::Named(Named::BackTab),
            "tab" => Code::Named(Named::Tab),
            "backspace" => Code::Named(Named::Backspace),
            "delete" => Code::Named(Named::Delete),
            "up" => Code::Named(Named::Up),
            "down" => Code::Named(Named::Down),
            "left" => Code::Named(Named::Left),
            "right" => Code::Named(Named::Right),
            "home" => Code::Named(Named::Home),
            "end" => Code::Named(Named::End),
            "pageup" => Code::Named(Named::PageUp),
            "pagedown" => Code::Named(Named::PageDown),
            "shift" => Code::Named(Named::Shift),
            "space" => Code::Char(' '),
            key => {
                // What would be typed, if there is anything: Shift+j is "J". Without the key
                // character (Ctrl held), a letter with Shift down is its capital.
                let typed = k
                    .key_char
                    .as_deref()
                    .filter(|t| t.chars().count() == 1 && !m.control)
                    .or(Some(key).filter(|t| t.chars().count() == 1));
                match typed.and_then(|t| t.chars().next()) {
                    Some(c) if m.shift && !m.control && k.key_char.is_none() => {
                        Code::Char(c.to_ascii_uppercase())
                    }
                    Some(c) if m.shift && m.control => Code::Char(c.to_ascii_uppercase()),
                    Some(c) => Code::Char(c),
                    None => Code::Named(Named::Other),
                }
            }
        };
        Self {
            code,
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            meta: m.platform,
        }
    }

    /// A key by name: `"j"`, `"J"`, `"enter"`, `"esc"`, `"ctrl+h"`, `"ctrl+shift+H"`, `"backtab"`.
    /// A capital letter is the shifted key.
    pub fn parse(name: &str) -> Option<Self> {
        let mut key = Self::char(' ');
        let mut rest = name;
        while let Some((head, tail)) = rest
            .split_once('+')
            .filter(|(h, t)| !h.is_empty() && !t.is_empty())
        {
            match head {
                "ctrl" => key.ctrl = true,
                "alt" => key.alt = true,
                "shift" => key.shift = true,
                "meta" | "super" => key.meta = true,
                _ => return None,
            }
            rest = tail;
        }
        key.code = match rest {
            "esc" | "escape" => Code::Named(Named::Escape),
            "enter" | "return" => Code::Named(Named::Enter),
            "tab" if key.shift => Code::Named(Named::BackTab),
            "tab" => Code::Named(Named::Tab),
            "backtab" => {
                key.shift = true;
                Code::Named(Named::BackTab)
            }
            "backspace" => Code::Named(Named::Backspace),
            "delete" => Code::Named(Named::Delete),
            "up" => Code::Named(Named::Up),
            "down" => Code::Named(Named::Down),
            "left" => Code::Named(Named::Left),
            "right" => Code::Named(Named::Right),
            "home" => Code::Named(Named::Home),
            "end" => Code::Named(Named::End),
            "space" => Code::Char(' '),
            one if one.chars().count() == 1 => {
                let c = one.chars().next()?;
                if c.is_ascii_uppercase() {
                    key.shift = true;
                }
                Code::Char(c)
            }
            _ => return None,
        };
        Some(key)
    }

    /// The text this key types, if it types any: what a text field would take.
    pub fn text(&self) -> Option<char> {
        match self.code {
            Code::Char(c) if !self.ctrl && !self.alt && !self.meta => Some(c),
            _ => None,
        }
    }

    pub fn is(&self, c: char) -> bool {
        self.code == Code::Char(c)
    }

    pub fn is_named(&self, n: Named) -> bool {
        self.code == Code::Named(n)
    }

    /// No Ctrl, Alt or Meta (Shift is part of what was typed).
    pub fn bare(&self) -> bool {
        !self.ctrl && !self.alt && !self.meta
    }

    /// The key as GPUI's keystroke syntax writes it (`"ctrl-shift-h"`), for tests that send keys
    /// through the window.
    pub fn to_gpui(self) -> String {
        let mut s = String::new();
        let (mut shift, mut key) = (self.shift, String::new());
        match self.code {
            Code::Char(' ') => key.push_str("space"),
            Code::Char(c) => {
                if c.is_ascii_uppercase() {
                    shift = true;
                }
                key.push(c.to_ascii_lowercase());
            }
            Code::Named(n) => {
                key.push_str(match n {
                    Named::Escape => "escape",
                    Named::Enter => "enter",
                    Named::Tab => "tab",
                    Named::BackTab => {
                        shift = true;
                        "tab"
                    }
                    Named::Backspace => "backspace",
                    Named::Delete => "delete",
                    Named::Up => "up",
                    Named::Down => "down",
                    Named::Left => "left",
                    Named::Right => "right",
                    Named::Home => "home",
                    Named::End => "end",
                    Named::PageUp => "pageup",
                    Named::PageDown => "pagedown",
                    Named::Shift => "shift",
                    Named::Other => "f24",
                });
            }
        }
        for (on, name) in [
            (self.ctrl, "ctrl-"),
            (self.alt, "alt-"),
            (shift && key != "shift", "shift-"),
            (self.meta, "super-"),
        ] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(&key);
        s
    }

    /// For the log of a stress run, and failure messages.
    pub fn describe(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "ctrl+"), (self.alt, "alt+"), (self.meta, "meta+")] {
            if on {
                s.push_str(name);
            }
        }
        match self.code {
            Code::Char(' ') => s.push_str("space"),
            Code::Char(c) => s.push(c),
            Code::Named(n) => s.push_str(&format!("{n:?}").to_lowercase()),
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse_to_keys() {
        assert_eq!(Key::parse("j"), Some(Key::char('j')));
        let up = Key::parse("J").unwrap();
        assert!(up.shift && up.is('J'));
        let c = Key::parse("ctrl+shift+H").unwrap();
        assert!(c.ctrl && c.shift && c.is('H'));
        assert_eq!(Key::parse("esc"), Some(Key::named(Named::Escape)));
        assert!(Key::parse("backtab").unwrap().is_named(Named::BackTab));
        assert_eq!(Key::parse("nonsense+"), None);
        assert!(Key::parse("space").unwrap().is(' '));
    }

    #[test]
    fn a_keystroke_is_the_text_typed() {
        let k = Key::from_keystroke(&Keystroke::parse("shift-j").unwrap());
        assert!(k.is('J'), "{k:?}");
        let k = Key::from_keystroke(&Keystroke::parse("ctrl-h").unwrap());
        assert!(k.ctrl && k.is('h'), "{k:?}");
        let k = Key::from_keystroke(&Keystroke::parse("escape").unwrap());
        assert!(k.is_named(Named::Escape));
    }
}
