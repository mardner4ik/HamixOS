use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Modifiers,
}

impl KeyPress {
    pub fn new(key: Key, mods: Modifiers) -> KeyPress {
        KeyPress { key, mods }
    }

    pub fn is(&self, shortcut: &str) -> bool {
        Shortcut::parse(shortcut).map(|s| s.matches(self)).unwrap_or(false)
    }

    pub fn ctrl(&self, c: char) -> bool {
        self.mods.ctrl && !self.mods.alt && matches!(self.key, Key::Char(k) if k.eq_ignore_ascii_case(&c))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    pub mods: Modifiers,
}

impl Shortcut {
    pub fn parse(text: &str) -> Option<Shortcut> {
        let mut mods = Modifiers::default();
        let mut key = None;
        for part in text.split('+').map(|p| p.trim()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" => mods.alt = true,
                "shift" => mods.shift = true,
                "enter" | "return" => key = Some(Key::Enter),
                "esc" | "escape" => key = Some(Key::Escape),
                "tab" => key = Some(Key::Tab),
                "del" | "delete" => key = Some(Key::Delete),
                "backspace" => key = Some(Key::Backspace),
                "up" => key = Some(Key::Up),
                "down" => key = Some(Key::Down),
                "left" => key = Some(Key::Left),
                "right" => key = Some(Key::Right),
                "home" => key = Some(Key::Home),
                "end" => key = Some(Key::End),
                "pageup" => key = Some(Key::PageUp),
                "pagedown" => key = Some(Key::PageDown),
                "space" => key = Some(Key::Char(' ')),
                "plus" => key = Some(Key::Char('+')),
                "minus" => key = Some(Key::Char('-')),
                f if f.len() >= 2 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => key = Some(Key::F(f[1..].parse().ok()?)),
                s if s.chars().count() == 1 => key = s.chars().next().map(Key::Char),
                _ => return None,
            }
        }
        key.map(|key| Shortcut { key, mods })
    }

    pub fn matches(&self, press: &KeyPress) -> bool {
        let same_key = match (self.key, press.key) {
            (Key::Char(a), Key::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        };
        same_key && self.mods.ctrl == press.mods.ctrl && self.mods.alt == press.mods.alt && (self.mods.shift == press.mods.shift || matches!(self.key, Key::Char(_)) && !self.mods.shift)
    }

    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.mods.ctrl {
            out.push_str("Ctrl+");
        }
        if self.mods.alt {
            out.push_str("Alt+");
        }
        if self.mods.shift {
            out.push_str("Shift+");
        }
        match self.key {
            Key::Char(' ') => out.push_str("Space"),
            Key::Char(c) => out.extend(c.to_uppercase()),
            Key::F(n) => out.push_str(&alloc::format!("F{}", n)),
            other => out.push_str(&alloc::format!("{:?}", other)),
        }
        out
    }
}
