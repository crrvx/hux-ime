// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 状态访问与同步：查询、直写持久化值、外部配置重放、上下文同步与变更观察。

use super::{OPTIONS_ERROR_MESSAGE, OPTIONS_ERROR_PROPERTY, OptionsStore};
use hux_core::collections::Map;
use hux_core::session::{Context, set_property_if_changed};

impl OptionsStore {
    /// 更新设置层缺省（配置界面变化后调用；随后由 [`OptionsStore::set_values`] 把设置值写成
    /// 持久化值，故 `options.yaml` 的**旧值**不再压制配置页）。
    /// 该选项是否由本存储管理（有声明的缺省 ⇒ 可持久化）。
    ///
    /// 平台据此分流：可持久化项交由 [`OptionsStore::sync`] 写入（其写入带抑制名单，
    /// 不会被 [`OptionsStore::observe`] 当成用户改动落盘），其余非持久化项直接写上下文。
    pub fn covers(&self, name: &str) -> bool {
        self.options.covers(name)
    }

    /// 直写持久化值并保存（**无会话**时状态菜单切换仍须落盘；返回是否保存成功）。
    pub fn set_value(&mut self, name: &str, value: bool) -> bool {
        if !self.options.set_value(name, value) {
            return self.options.covers(name);
        }
        self.save().is_ok()
    }

    pub fn set_defaults(&mut self, defaults: Map<String, bool>) {
        self.options.defaults = defaults;
    }

    /// 外部配置（配置页）为准：把设置值写成持久化值，使 `options.yaml` 里的旧值不再压制设置值。
    ///
    /// 与 [`OptionsStore::observe`] 的分工：`observe` 记录**用户改动**（状态菜单），本方法把
    /// **外部配置**重放进同一份存储——配置页与状态菜单于是共用单一事实来源。调用前须先经
    /// [`OptionsStore::set_defaults`] 登记这些角色（未登记的键忽略）。返回值 = 保存结果
    /// （无变化时不落盘，视为成功）。
    pub fn set_values(&mut self, values: &Map<String, bool>) -> bool {
        let mut changed = false;
        for (name, value) in values.iter() {
            changed |= self.options.set_value(name, *value);
        }
        if !changed {
            return true;
        }
        self.save().is_ok()
    }

    /// 参照 `M.options.sync`：把持久化值（缺省回退内建缺省）同步进上下文选项。
    pub fn sync(&mut self, context: &mut Context) {
        self.options.sync(context);
    }

    /// 单项生效值（持久化值 → 设置缺省）；未知项返回 `None`。
    pub fn value(&self, name: &str) -> Option<bool> {
        self.options
            .values
            .get(name)
            .or_else(|| self.options.defaults.get(name))
            .copied()
    }

    /// 选项变更（上下文事件）：记录；有变更则保存并维护错误属性。
    pub fn observe(&mut self, context: &mut Context, name: &str) {
        if !self.options.observe(context, name) {
            return;
        }
        let error = match self.save() {
            Ok(()) => "",
            Err(_) => OPTIONS_ERROR_MESSAGE,
        };
        set_property_if_changed(context, OPTIONS_ERROR_PROPERTY, error);
    }
}
