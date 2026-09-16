//! ascii_composer（librime 原生处理器）等价物：Shift/Caps 切换 `ascii_mode`。
//!
//! 配置取自参照 schema（`tiger_sentence.schema.yaml` + 私有 `tiger_sentence_ascii`）：
//! `good_old_caps_lock: true`、`switch_key: {Caps_Lock: clear, Shift_L/R: commit_code,
//! Control_L/R: noop}`；缓冲态（`tiger_sentence_buffered_text` 非空）按参照
//! `ascii_component` 把 `commit_code`/`inline_ascii` 归一为 `commit_text`。
//!
//! 计时：Shift 敲击（按下后 [`TOGGLE_DURATION`] 秒内抬起）才切换；`now` 为秒（宿主墙钟）。
//! 空闲且 `ascii_mode` 时返回 [`AsciiResult::Rejected`]：按键直接交宿主（直通输入）。
//!
//! **fcitx5 适配（大小写状态）**：CapsLock 键事件的 caps 位语义随平台/前端而异（切换前或
//! 切换后），且按键本身不应触发切换。本实现**不处理 CapsLock 按键**（只放行交系统），
//! 而是观察每个按键事件携带的**系统 caps 状态**：状态变化时把 `ascii_mode` 同步为
//! 「caps 开 = 英文直通、caps 关 = 中文输入」（用 CapsLock 配置样式，参照为 `clear`）。
//! 探针金样不产生 caps 变化，参照的 Shift 轻击等路径保持不变。
//!
//! `inline_ascii` 的「组合结束即退出临时 ascii」由 [`AsciiComposer::on_context_update`]
//! 在每个按键后调用（参照连接 `update_notifier`；本实现按每键一次等价处理）。

use hashbrown::HashMap;

use crate::interaction::{ASCII_SWITCH_KEYS, ascii_switch_styles, buffered_text};
use crate::key::KeyEvent;
use crate::session::Context;

/// Shift 敲击判定窗口（参照 `AsciiComposer::toggle_duration_limit`，毫秒）。
pub const TOGGLE_DURATION: f64 = 0.5;

/// 参照 `ProcessResult` 的宿主侧映射：
/// `Accepted` = 吞键（停止处理器链）；`Rejected` = 交还宿主（不吞键，停止链）；
/// `Noop` = 继续后续处理器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsciiResult {
    Accepted,
    Rejected,
    Noop,
}

/// 参照 `AsciiModeSwitchStyle`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsciiSwitchStyle {
    Noop,
    Inline,
    CommitText,
    CommitCode,
    Clear,
    Set,
    Unset,
}

impl AsciiSwitchStyle {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "noop" => Some(Self::Noop),
            "inline_ascii" => Some(Self::Inline),
            "commit_text" => Some(Self::CommitText),
            "commit_code" => Some(Self::CommitCode),
            "clear" => Some(Self::Clear),
            "set_ascii_mode" => Some(Self::Set),
            "unset_ascii_mode" => Some(Self::Unset),
            _ => None,
        }
    }
}

/// `ascii_composer` 实例状态（每会话一份）。
#[derive(Clone, Debug)]
pub struct AsciiComposer {
    shift_pressed: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
    super_pressed: bool,
    toggle_expired: f64,
    inline_ascii: bool,
    /// 基础样式表（键名 → 样式）。
    styles: HashMap<String, AsciiSwitchStyle>,
    /// 缓冲态样式表（`commit_code`/`inline_ascii` → `commit_text`）。
    buffered_styles: HashMap<String, AsciiSwitchStyle>,
    good_old_caps_lock: bool,
    /// 最近观察到的系统 caps 状态（`None` = 尚未观察到；用于状态同步）。
    caps_state: Option<bool>,
}

impl Default for AsciiComposer {
    fn default() -> Self {
        Self::reference()
    }
}

impl AsciiComposer {
    /// 参照 schema 的默认配置。
    pub fn reference() -> Self {
        let source: HashMap<String, String> = [
            ("Caps_Lock", "clear"),
            ("Shift_L", "commit_code"),
            ("Shift_R", "commit_code"),
            ("Control_L", "noop"),
            ("Control_R", "noop"),
        ]
        .into_iter()
        .map(|(name, style)| (name.to_string(), style.to_string()))
        .collect();
        Self::from_config(&source, true)
    }

    /// 由样式表构造（`good_old_caps_lock` 对应 schema 同名配置）。
    pub fn from_config(source: &HashMap<String, String>, good_old_caps_lock: bool) -> Self {
        let buffered = ascii_switch_styles(source);
        let mut styles = HashMap::new();
        let mut buffered_styles = HashMap::new();
        for name in ASCII_SWITCH_KEYS {
            let style = source
                .get(name)
                .and_then(|value| AsciiSwitchStyle::parse(value))
                .unwrap_or(AsciiSwitchStyle::Noop);
            let buffered_style = buffered
                .get(name)
                .and_then(|value| AsciiSwitchStyle::parse(value))
                .unwrap_or(AsciiSwitchStyle::Noop);
            styles.insert(name.to_string(), style);
            buffered_styles.insert(name.to_string(), buffered_style);
        }
        Self {
            shift_pressed: false,
            ctrl_pressed: false,
            alt_pressed: false,
            super_pressed: false,
            toggle_expired: 0.0,
            inline_ascii: false,
            styles,
            buffered_styles,
            good_old_caps_lock,
            caps_state: None,
        }
    }

    /// 参照 `AsciiComposer::ProcessKeyEvent`（含 fcitx5 的 caps 状态同步）。
    pub fn process_key(
        &mut self,
        key_event: &KeyEvent,
        context: &mut Context,
        now: f64,
    ) -> AsciiResult {
        // 状态同步先于分类：本键即按最新模式处理（避免滞后一键）。
        self.sync_caps_state(key_event, context);
        self.classify_key(key_event, context, now)
    }

    /// 跟随系统 caps 状态（详见模块注释）：状态变化时把 `ascii_mode` 同步为 caps 值。
    fn sync_caps_state(&mut self, key_event: &KeyEvent, context: &mut Context) {
        if !self.good_old_caps_lock {
            return;
        }
        let caps = key_event.caps();
        if let Some(previous) = self.caps_state
            && previous != caps
            && context.get_option("ascii_mode") != caps
        {
            let style = self.caps_lock_style(context);
            self.switch_ascii_mode(caps, style, context);
        }
        self.caps_state = Some(caps);
    }

    fn classify_key(
        &mut self,
        key_event: &KeyEvent,
        context: &mut Context,
        now: f64,
    ) -> AsciiResult {
        let modifier_count = u8::from(key_event.shift())
            + u8::from(key_event.ctrl())
            + u8::from(key_event.alt())
            + u8::from(key_event.super_modifier());
        if modifier_count > 1 {
            self.reset_pressed();
            return AsciiResult::Noop;
        }
        if self.caps_lock_style(context) != AsciiSwitchStyle::Noop {
            let result = self.process_caps_lock(key_event, context);
            if result != AsciiResult::Noop {
                return result;
            }
        }
        let keycode = key_event.keycode;
        if keycode == 0xff30 {
            // XK_Eisu_toggle：字母数字切换键。
            if !key_event.release() {
                self.reset_pressed();
                self.toggle_with_key(keycode, context);
                return AsciiResult::Accepted;
            }
            return AsciiResult::Rejected;
        }
        let is_shift = keycode == 0xffe1 || keycode == 0xffe2;
        let is_ctrl = keycode == 0xffe3 || keycode == 0xffe4;
        let is_alt = keycode == 0xffe9 || keycode == 0xffea;
        let is_super = keycode == 0xffeb || keycode == 0xffec;
        if is_shift || is_ctrl || is_alt || is_super {
            if key_event.release() {
                if self.any_pressed() {
                    let matches = (is_shift && self.shift_pressed)
                        || (is_ctrl && self.ctrl_pressed)
                        || (is_alt && self.alt_pressed)
                        || (is_super && self.super_pressed);
                    if matches && now < self.toggle_expired {
                        self.toggle_with_key(keycode, context);
                    }
                    self.reset_pressed();
                    return AsciiResult::Noop;
                }
            } else if !self.any_pressed() {
                // 首次按下：记录候选切换键，抬起及时才切换。
                if is_shift {
                    self.shift_pressed = true;
                } else if is_ctrl {
                    self.ctrl_pressed = true;
                } else if is_alt {
                    self.alt_pressed = true;
                } else if is_super {
                    self.super_pressed = true;
                }
                self.toggle_expired = now + TOGGLE_DURATION;
            }
            return AsciiResult::Noop;
        }
        // 其他键：清空按键记录；Control/Alt/Super 组合与 Shift+space 交后续处理器。
        self.reset_pressed();
        if key_event.ctrl()
            || key_event.alt()
            || key_event.super_modifier()
            || (key_event.shift() && keycode == 0x20)
        {
            return AsciiResult::Noop;
        }
        if context.get_option("ascii_mode") {
            if !context.is_composing() {
                return AsciiResult::Rejected; // direct commit
            }
            // 组合中的 ascii 串内联编辑（inline ascii）。
            if !key_event.release() && (0x20..0x80).contains(&keycode) {
                context.push_input(&[keycode as u8]);
                return AsciiResult::Accepted;
            }
        }
        AsciiResult::Noop
    }

    /// 参照 `AsciiComposer::ProcessCapsLock`。
    fn process_caps_lock(&mut self, key_event: &KeyEvent, context: &mut Context) -> AsciiResult {
        let keycode = key_event.keycode;
        if keycode == 0xffe5 {
            // XK_Caps_Lock：**不处理敲击**（去按键化）：只放行交系统切换 caps 状态，
            // `ascii_mode` 由 [`AsciiComposer::sync_caps_state`] 跟随系统状态。
            self.reset_pressed();
            return if self.good_old_caps_lock {
                AsciiResult::Rejected
            } else {
                AsciiResult::Accepted
            };
        }
        if key_event.caps() {
            // `good_old_caps_lock = true`：Caps 状态不接管字母大小写，事件交宿主。
            if !self.good_old_caps_lock
                && !key_event.release()
                && !key_event.ctrl()
                && !key_event.alt()
                && !key_event.super_modifier()
            {
                // 参照 `!good_old_caps_lock`：反转大小写后直接上屏。
                let inverted = if (0x41..=0x5a).contains(&keycode) {
                    Some(keycode + 32)
                } else if (0x61..=0x7a).contains(&keycode) {
                    Some(keycode - 32)
                } else {
                    None
                };
                if let Some(inverted) = inverted {
                    let text = char::from_u32(inverted as u32)
                        .map(|ch| ch.to_string())
                        .unwrap_or_default();
                    context.direct_commit(&text);
                    return AsciiResult::Accepted;
                }
            }
            return AsciiResult::Rejected;
        }
        AsciiResult::Noop
    }

    /// 参照 `AsciiComposer::ToggleAsciiModeWithKey`。
    fn toggle_with_key(&mut self, keycode: i32, context: &mut Context) -> bool {
        let buffered = !buffered_text(context).is_empty();
        let Some(style) = self.style_for(keycode, buffered) else {
            return false;
        };
        let old_mode = context.get_option("ascii_mode");
        let new_mode = match style {
            AsciiSwitchStyle::Set => true,
            AsciiSwitchStyle::Unset => false,
            _ => !old_mode,
        };
        if old_mode == new_mode {
            return false;
        }
        self.switch_ascii_mode(new_mode, style, context);
        true
    }

    /// 参照 `AsciiComposer::SwitchAsciiMode`。
    fn switch_ascii_mode(&mut self, mode: bool, style: AsciiSwitchStyle, context: &mut Context) {
        if context.is_composing() {
            match style {
                AsciiSwitchStyle::Inline => {
                    self.inline_ascii = mode;
                }
                AsciiSwitchStyle::CommitText => {
                    context.confirm_current_selection();
                }
                AsciiSwitchStyle::CommitCode => {
                    context.refresh_non_confirmed_composition();
                    context.commit();
                }
                AsciiSwitchStyle::Clear | AsciiSwitchStyle::Set | AsciiSwitchStyle::Unset => {
                    context.clear();
                }
                AsciiSwitchStyle::Noop => {}
            }
        }
        context.set_option("ascii_mode", mode);
    }

    /// 参照 `AsciiComposer::OnContextUpdate`：临时 ascii 随组合结束退出。
    pub fn on_context_update(&mut self, context: &mut Context) {
        if self.inline_ascii && !context.is_composing() {
            self.inline_ascii = false;
            context.set_option("ascii_mode", false);
        }
    }

    fn style_for(&self, keycode: i32, buffered: bool) -> Option<AsciiSwitchStyle> {
        let name = match keycode {
            0xffe1 => "Shift_L",
            0xffe2 => "Shift_R",
            0xffe3 => "Control_L",
            0xffe4 => "Control_R",
            0xffe9 => "Alt_L",
            0xffea => "Alt_R",
            0xffeb => "Super_L",
            0xffec => "Super_R",
            0xffe5 => "Caps_Lock",
            0xff30 => "Eisu_toggle",
            _ => return None,
        };
        let table = if buffered {
            &self.buffered_styles
        } else {
            &self.styles
        };
        table.get(name).copied()
    }

    fn caps_lock_style(&self, context: &Context) -> AsciiSwitchStyle {
        let buffered = !buffered_text(context).is_empty();
        self.style_for(0xffe5, buffered)
            .unwrap_or(AsciiSwitchStyle::Noop)
    }

    fn any_pressed(&self) -> bool {
        self.shift_pressed || self.ctrl_pressed || self.alt_pressed || self.super_pressed
    }

    fn reset_pressed(&mut self) {
        self.shift_pressed = false;
        self.ctrl_pressed = false;
        self.alt_pressed = false;
        self.super_pressed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(repr: &str) -> KeyEvent {
        KeyEvent::from_repr(repr).expect("key repr")
    }

    fn press(
        composer: &mut AsciiComposer,
        context: &mut Context,
        repr: &str,
        now: f64,
    ) -> AsciiResult {
        composer.process_key(&key(repr), context, now)
    }

    fn composing() -> Context {
        let mut context = Context::new();
        context.set_option("ascii_mode", false);
        context.push_input(b"ab");
        context.drain_events();
        context
    }

    #[test]
    fn shift_tap_toggles_ascii_mode_and_commits_code() {
        let mut context = composing();
        let mut composer = AsciiComposer::reference();
        assert_eq!(
            press(&mut composer, &mut context, "Shift_L", 0.0),
            AsciiResult::Noop
        );
        assert_eq!(
            press(&mut composer, &mut context, "Release+Shift_L", 0.1),
            AsciiResult::Noop
        );
        assert!(context.get_option("ascii_mode"), "轻敲 Shift 应切到 ascii");
        assert!(context.input().is_empty(), "commit_code 应提交原始编码");
        assert_eq!(context.last_commit_text(), "ab");
        // 再次轻敲切回
        assert_eq!(
            press(&mut composer, &mut context, "Shift_R", 1.0),
            AsciiResult::Noop
        );
        assert_eq!(
            press(&mut composer, &mut context, "Release+Shift_R", 1.1),
            AsciiResult::Noop
        );
        assert!(!context.get_option("ascii_mode"));
    }

    #[test]
    fn shift_hold_beyond_window_does_not_toggle() {
        let mut context = composing();
        let mut composer = AsciiComposer::reference();
        assert_eq!(
            press(&mut composer, &mut context, "Shift_L", 0.0),
            AsciiResult::Noop
        );
        assert_eq!(
            press(&mut composer, &mut context, "Release+Shift_L", 0.6),
            AsciiResult::Noop
        );
        assert!(!context.get_option("ascii_mode"), "超过窗口不应切换");
        assert_eq!(context.input(), b"ab");
    }

    #[test]
    fn ascii_mode_passes_keys_through_when_idle() {
        let mut context = Context::new();
        context.set_option("ascii_mode", true);
        let mut composer = AsciiComposer::reference();
        // 空闲：直通（Rejected）
        assert_eq!(
            press(&mut composer, &mut context, "a", 0.0),
            AsciiResult::Rejected
        );
        // 组合中：内联 ascii 编辑（Accepted）
        context.push_input(b"a");
        assert_eq!(
            press(&mut composer, &mut context, "b", 0.0),
            AsciiResult::Accepted
        );
        assert_eq!(context.input(), b"ab");
    }

    #[test]
    fn caps_state_drives_ascii_mode_regardless_of_event_semantics() {
        // fcitx5（切换后语义）：CapsLock 按下事件已带新 caps 位 → 以系统状态为准。
        let mut context = composing();
        let mut composer = AsciiComposer::reference();
        // 首次观察：仅记录（不切换）
        assert_eq!(
            press(&mut composer, &mut context, "a", 0.0),
            AsciiResult::Noop
        );
        // 开：事件 caps=1 → 参照方向相反，状态同步纠正为 ascii 开
        assert_eq!(
            press(&mut composer, &mut context, "Lock+Caps_Lock", 0.1),
            AsciiResult::Rejected
        );
        assert!(context.get_option("ascii_mode"), "caps 开 → ascii 开");
        // 关：事件 caps=0 → 同步关闭（回到中文输入）
        assert_eq!(
            press(&mut composer, &mut context, "Caps_Lock", 0.2),
            AsciiResult::Rejected
        );
        assert!(!context.get_option("ascii_mode"), "caps 关 → ascii 关");
        // 系统状态未变时：Shift 轻击仍可显式切换
        assert_eq!(
            press(&mut composer, &mut context, "Shift_L", 0.3),
            AsciiResult::Noop
        );
        assert_eq!(
            press(&mut composer, &mut context, "Release+Shift_L", 0.4),
            AsciiResult::Noop
        );
        assert!(context.get_option("ascii_mode"));
    }

    #[test]
    fn caps_lock_keypress_does_not_switch() {
        // fcitx5 适配：CapsLock 敲击不触发切换（不以按键为准），只放行交系统。
        let mut context = composing();
        let mut composer = AsciiComposer::reference();
        assert_eq!(
            press(&mut composer, &mut context, "Caps_Lock", 0.0),
            AsciiResult::Rejected
        );
        assert!(!context.get_option("ascii_mode"));
        assert_eq!(context.input(), b"ab", "敲击不清空组合");
        assert_eq!(
            press(&mut composer, &mut context, "Release+Caps_Lock", 0.1),
            AsciiResult::Rejected
        );
        assert!(!context.get_option("ascii_mode"));
        assert_eq!(context.input(), b"ab");
        // caps 位随事件到来时（系统状态变化）才同步模式
        assert_eq!(
            press(&mut composer, &mut context, "Lock+a", 0.2),
            AsciiResult::Rejected
        );
        assert!(context.get_option("ascii_mode"), "caps 开 → ascii 开");
    }

    #[test]
    fn buffered_shift_uses_commit_text_style() {
        let mut context = Context::new();
        context.set_option("ascii_mode", false);
        context.set_property("tiger_sentence_buffered_text", "乙");
        context.push_input(b"~c");
        context.composition.segments.push(crate::session::Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            candidates: vec![crate::session::Candidate::new(
                "sentence_buffered",
                0,
                2,
                "c",
                "",
            )],
            ..Default::default()
        });
        context.drain_events();
        let mut composer = AsciiComposer::reference();
        assert_eq!(
            press(&mut composer, &mut context, "Shift_L", 0.0),
            AsciiResult::Noop
        );
        assert_eq!(
            press(&mut composer, &mut context, "Release+Shift_L", 0.1),
            AsciiResult::Noop
        );
        assert!(context.get_option("ascii_mode"));
        // commit_text：确认当前选中（不直接提交，输入保留）
        assert!(context.composition.back().unwrap().selected);
        assert_eq!(context.input(), b"~c");
    }
}
