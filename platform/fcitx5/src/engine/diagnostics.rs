// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 诊断：状态串（`hux_engine_status`）与配置 / 选项 / 学习库 / 热键诊断的唯一持有者。

use std::ffi::CString;
use std::sync::OnceLock;

use hux_core::scheme::Scheme;

/// 引擎的状态与诊断面：状态串基线 + 四类诊断 + 状态串本身 + 装载摘要。
///
/// 状态串 = 基线（装配说明）+ 配置诊断 + 选项保存错误 + 运行期学习库错误 + 热键绑定诊断
/// （见 [`Diagnostics::refresh_status`]）。构造与「重新部署」按同一口径写基线。
pub(crate) struct Diagnostics {
    /// 状态串基线（构造 / 重新部署时的装载说明；其余诊断按需拼在其后）。
    status_base: String,
    /// 选项保存失败的最近一条诊断（来自 `hux_cfg::OPTIONS_ERROR_PROPERTY`）。
    option_error: Option<String>,
    /// 学习库的**当前**诊断（构造 / 重新部署时读一次，运行期落库失败由
    /// [`Diagnostics::observe_learning_error`] 跟进）。
    learning_error: Option<String>,
    /// 构造 / 重新部署时已并入 `status_base` 的那条学习诊断（避免与运行期条目重复拼接）。
    learning_error_baseline: Option<String>,
    /// 配置页热键绑定里无法解析的项（`角色=键名`）。
    hotkey_notes: Vec<String>,
    /// 最近一次配置下发的逐角色诊断（角色缺失 / 类型不符，`config:` 前缀）。
    /// 方案已按缺省值回退，此串只是把「设置没生效」的原因暴露到状态里。
    config_notes: Vec<String>,
    /// 状态串（`hux_engine_status` 的指针来源；重建时替换，旧指针随即失效）。
    pub(crate) status: CString,
    /// 数据装载摘要（`hux_engine_data_info` 的指针来源）：**首次调用时算一次**并缓存；
    /// 配置下发 / 重新部署时置空（此前返回的指针随即失效，同 `status` 的契约）。
    data_info: OnceLock<CString>,
}

impl Diagnostics {
    /// 按装配基线构造：`notes` = 装配说明，`learning_error` = 装配时读到的学习库诊断。
    pub(crate) fn new(notes: Vec<String>, learning_error: Option<String>) -> Self {
        let status_base = notes.join("; ");
        Self {
            status: crate::ui::cstring_lossy(&status_base),
            status_base,
            option_error: None,
            learning_error_baseline: learning_error.clone(),
            learning_error,
            hotkey_notes: Vec::new(),
            config_notes: Vec::new(),
            data_info: OnceLock::new(),
        }
    }

    /// 重设状态串基线（「重新部署」换上新装配说明）并重建状态串。
    ///
    /// 运行期诊断字段（选项保存错误 / 热键诊断）保持不变——它们不随重新部署消失；
    /// 逐角色配置诊断由随后的配置下发重新产出，先清掉旧方案的（避免拼出过期诊断）。
    pub(crate) fn reset(&mut self, notes: Vec<String>, learning_error: Option<String>) {
        self.status_base = notes.join("; ");
        self.config_notes.clear();
        self.learning_error = learning_error.clone();
        self.learning_error_baseline = learning_error;
        self.refresh_status();
    }

    /// 数据装载摘要（`hux_engine_data_info`）：首次调用按方案算一次并缓存
    /// （ABI 入口是 `&self`，故用 `OnceLock`）；配置下发 / 重新部署时置空失效。
    pub(crate) fn data_info(&self, scheme: &dyn Scheme) -> &CString {
        self.data_info
            .get_or_init(|| crate::ui::cstring_lossy(&scheme.data_info()))
    }

    /// 让已缓存的装载摘要失效（下次 `data_info` 重算；此前返回的指针随之失效）。
    pub(crate) fn invalidate_data_info(&mut self) {
        self.data_info = OnceLock::new();
    }

    /// 选项保存失败诊断 → 状态串（`None` = 清除），有变化才重建。
    pub(crate) fn observe_option_error(&mut self, error: Option<String>) {
        if error != self.option_error {
            self.option_error = error;
            self.refresh_status();
        }
    }

    /// 学习库诊断变化 → 并入状态串（诊断未变则原地不动，不换状态串指针）。
    pub(crate) fn observe_learning_error(&mut self, current: Option<String>) {
        if current != self.learning_error {
            self.learning_error = current;
            self.refresh_status();
        }
    }

    /// 热键绑定诊断 → 状态串（配置页绑到无名字 keysym 时该绑定会被丢弃，此处点名）。
    pub(crate) fn observe_hotkey_notes(&mut self, notes: Vec<String>) {
        if notes != self.hotkey_notes {
            self.hotkey_notes = notes;
            self.refresh_status();
        }
    }

    /// 配置下发后的收尾：装载摘要失效 + 逐角色诊断并入状态串。
    ///
    /// 状态串只在诊断真有变化时重建：重建即换指针，宿主拿到旧指针的窗口越短越好，
    /// 而「每次下发都换指针」会让宿主侧的字符串比较永远不相等。
    pub(crate) fn observe_config_notes(&mut self, notes: Vec<String>) {
        if notes != self.config_notes {
            self.config_notes = notes;
            self.refresh_status();
        }
    }

    /// 重建状态串（唯一拼装点）：基线 + 配置诊断 + 选项保存错误 + 运行期学习库错误 + 热键诊断。
    fn refresh_status(&mut self) {
        let mut status = self.status_base.clone();
        if !self.config_notes.is_empty() {
            status.push_str("; ");
            status.push_str(&self.config_notes.join("; "));
        }
        if let Some(error) = &self.option_error {
            status.push_str("; options: ");
            status.push_str(error);
        }
        // 学习库：构造期那条已在基线里，只有**运行期新增/变化**的诊断在此拼接。
        if let Some(error) = &self.learning_error
            && Some(error) != self.learning_error_baseline.as_ref()
        {
            status.push_str("; learning: ");
            status.push_str(error);
        }
        // 配置页绑到无名字 keysym（媒体键等）时该绑定会被丢弃，此处点名。
        if !self.hotkey_notes.is_empty() {
            status.push_str("; hotkeys: 忽略无法识别的绑定 ");
            status.push_str(&self.hotkey_notes.join(", "));
        }
        self.status = crate::ui::cstring_lossy(&status);
    }
}
