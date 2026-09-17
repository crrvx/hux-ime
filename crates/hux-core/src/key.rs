//! 键事件与 rime 键名表，对应 librime `key_event.{h,cc}` 与 `key_table.cc`。
//!
//! `repr()`/`Parse` 逐字节复刻 librime 行为；键名表由 `tools/generators/gen_key_table.py`
//! 从 librime 源码生成（见 `key_table.rs` 的来源哈希）。

use crate::key_table::{KEYS_BY_KEYVAL, KEYS_BY_NAME, MODIFIER_NAMES};
use hashbrown::HashMap;
use std::sync::OnceLock;

pub const K_SHIFT_MASK: i32 = 1 << 0;
pub const K_LOCK_MASK: i32 = 1 << 1;
pub const K_CONTROL_MASK: i32 = 1 << 2;
pub const K_ALT_MASK: i32 = 1 << 3;
pub const K_SUPER_MASK: i32 = 1 << 26;
pub const K_HYPER_MASK: i32 = 1 << 27;
pub const K_META_MASK: i32 = 1 << 28;
pub const K_RELEASE_MASK: i32 = 1 << 30;
/// librime `kModifierMask`。
pub const K_MODIFIER_MASK: i32 = 0x5f00_1fff;
/// X11 `XK_VoidSymbol`。
pub const XK_VOID_SYMBOL: i32 = 0x00ff_ffff;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    pub keycode: i32,
    pub modifier: i32,
}

impl KeyEvent {
    pub fn new(keycode: i32, modifier: i32) -> Self {
        Self { keycode, modifier }
    }

    /// 参照 `KeyEvent::Parse`：单字节 repr 直接作为键值；
    /// 否则按 `+` 分割（前段为修饰名，末段为键名）。
    pub fn from_repr(repr: &str) -> Option<Self> {
        if repr.is_empty() {
            return None;
        }
        if repr.len() == 1 {
            return Some(Self::new(repr.as_bytes()[0] as i32, 0));
        }
        let mut modifier = 0;
        let mut parts = repr.split('+').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                let keycode = keycode_by_name(part)?;
                if keycode == XK_VOID_SYMBOL {
                    return None;
                }
                return Some(Self::new(keycode, modifier));
            }
            modifier |= modifier_by_name(part)?;
        }
        None
    }

    /// 参照 `KeyEvent::repr`：修饰名按位序前缀，未命名的键值输出十六进制。
    pub fn repr(&self) -> String {
        let mut out = String::new();
        if self.modifier != 0 {
            let mut remaining = self.modifier & K_MODIFIER_MASK;
            let mut bit = 0;
            while remaining != 0 {
                if remaining & 1 != 0
                    && let Some(name) = modifier_name(remaining << bit)
                {
                    out.push_str(name);
                    out.push('+');
                }
                remaining >>= 1;
                bit += 1;
            }
        }
        if let Some(name) = key_name(self.keycode) {
            out.push_str(name);
            return out;
        }
        if self.keycode <= 0xffff {
            return format!("{out}0x{:04x}", self.keycode);
        }
        if self.keycode <= 0x00ff_ffff {
            return format!("{out}0x{:06x}", self.keycode);
        }
        "(unknown)".to_string()
    }

    pub fn shift(&self) -> bool {
        self.modifier & K_SHIFT_MASK != 0
    }

    pub fn ctrl(&self) -> bool {
        self.modifier & K_CONTROL_MASK != 0
    }

    pub fn alt(&self) -> bool {
        self.modifier & K_ALT_MASK != 0
    }

    pub fn caps(&self) -> bool {
        self.modifier & K_LOCK_MASK != 0
    }

    pub fn super_modifier(&self) -> bool {
        self.modifier & K_SUPER_MASK != 0
    }

    pub fn release(&self) -> bool {
        self.modifier & K_RELEASE_MASK != 0
    }
}

/// 参照 `RimeGetKeyName`：按键值查键名（同序首个匹配）。
pub fn key_name(keycode: i32) -> Option<&'static str> {
    name_by_keycode().get(&keycode).copied()
}

/// 参照 `RimeGetKeycodeByName`：按名字查键值（同序首个匹配）。
pub fn keycode_by_name(name: &str) -> Option<i32> {
    keycode_by_name_map().get(name).copied()
}

/// 参照 `RimeGetModifierByName`：返回位掩码。
pub fn modifier_by_name(name: &str) -> Option<i32> {
    MODIFIER_NAMES
        .iter()
        .enumerate()
        .find_map(|(index, entry)| match entry {
            Some(value) if *value == name => Some(1 << index),
            _ => None,
        })
}

/// 参照 `RimeGetModifierName`：返回最低置位对应的名字（该位无名为 None）。
pub fn modifier_name(mut value: i32) -> Option<&'static str> {
    for entry in MODIFIER_NAMES.iter() {
        if value == 0 {
            break;
        }
        if value & 1 != 0 {
            return *entry;
        }
        value >>= 1;
    }
    None
}

fn name_by_keycode() -> &'static HashMap<i32, &'static str> {
    static MAP: OnceLock<HashMap<i32, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::with_capacity(KEYS_BY_NAME.len());
        for (keyval, name) in KEYS_BY_NAME.iter() {
            map.entry(*keyval).or_insert(*name);
        }
        map
    })
}

fn keycode_by_name_map() -> &'static HashMap<&'static str, i32> {
    static MAP: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::with_capacity(KEYS_BY_KEYVAL.len());
        for (keyval, name) in KEYS_BY_KEYVAL.iter() {
            map.entry(*name).or_insert(*keyval);
        }
        map
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_known_librime_vectors() {
        assert_eq!(KeyEvent::new(0x61, 0).repr(), "a");
        assert_eq!(KeyEvent::new(0x41, 0).repr(), "A");
        assert_eq!(KeyEvent::new(0x3b, 0).repr(), "semicolon");
        assert_eq!(KeyEvent::new(0x20, 0).repr(), "space");
        assert_eq!(KeyEvent::new(0xfe20, 0).repr(), "ISO_Left_Tab");
        assert_eq!(KeyEvent::new(0xff0d, 0).repr(), "Return");
        assert_eq!(KeyEvent::new(0xff08, 0).repr(), "BackSpace");
        assert_eq!(KeyEvent::new(0xff51, 0).repr(), "Left");
        assert_eq!(KeyEvent::new(0x61, K_SHIFT_MASK).repr(), "Shift+a");
        assert_eq!(KeyEvent::new(0xff09, K_SHIFT_MASK).repr(), "Shift+Tab");
        assert_eq!(KeyEvent::new(0x1234, 0).repr(), "0x1234");
        assert_eq!(KeyEvent::new(0x123456, 0).repr(), "0x123456");
        assert_eq!(KeyEvent::new(0x1234567, 0).repr(), "(unknown)");
    }

    #[test]
    fn parse_round_trips() {
        assert_eq!(KeyEvent::from_repr("a"), Some(KeyEvent::new(0x61, 0)));
        assert_eq!(
            KeyEvent::from_repr("Shift+Tab"),
            Some(KeyEvent::new(0xff09, K_SHIFT_MASK))
        );
        assert_eq!(
            KeyEvent::from_repr("Control+Alt+Delete"),
            Some(KeyEvent::new(0xffff, K_CONTROL_MASK | K_ALT_MASK))
        );
        assert_eq!(KeyEvent::from_repr("nope"), None);
        assert_eq!(KeyEvent::from_repr("Shift+nope"), None);
        assert_eq!(KeyEvent::from_repr(""), None);
    }
}
