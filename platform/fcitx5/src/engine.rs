// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 引擎：共享数据（词库/解码/选项/学习库）+ 按输入上下文隔离的多会话。

use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::abi::{HostCallback, core_modifiers};
use crate::learning_store::{self, LearningStore};
use crate::paths::{data_dirs, default_model_path, user_data_dir};
use crate::session::{ReverseLookupState, Session};
use hux_cfg::roles::{
    OptionKeys, ROLE_ALLOW_DUPLICATE_SINGLE, ROLE_HIGH_FREQ_LIMIT, ROLE_LEARNING_ON_TAB,
    ROLE_MIN_RETAINED_INPUT_LENGTH, ROLE_PAGE_CYCLE, ROLE_PAGE_DOWN_KEYS, ROLE_PAGE_SIZE,
    ROLE_PAGE_UP_KEYS, ROLE_REVERSE_LOOKUP_CHARACTER_KEYS, ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
    RUNTIME_OPTION_ROLES,
};
use hux_cfg::{CandidateLayout, OptionsStore, Settings};

use hux_core::key::KeyEvent;
use hux_core::scheme::{KeyOutcome, OptionDecl, Scheme, SchemeConfig, Value};
use hux_core::session::{Context, Event};
use hux_scheme_tiger::scheme::{ASSETS, TigerScheme};

pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
        .unwrap_or(0.0)
}

impl Engine {
    /// 状态菜单可切换的运行时开关（顺序即菜单顺序 = C ABI 的 `HUX_OPTION_*` 角色序）：
    /// 方案声明的 4 项 + rime 标准的 `full_shape`。方案未声明的角色**不出现在菜单里**。
    pub(crate) fn runtime_options(&self) -> Vec<&'static str> {
        RUNTIME_OPTION_ROLES
            .iter()
            .filter_map(|role| self.option_roles.key(role))
            .collect()
    }

    /// 运行时空开关白名单查询（按键路径每键都会问一次，故不做分配）。
    fn is_runtime_option(&self, name: &str) -> bool {
        RUNTIME_OPTION_ROLES
            .iter()
            .any(|role| self.option_roles.key(role) == Some(name))
    }

    /// 单字重码选项的**生效值**（会话 → 存储 → 设置缺省；角色无键时按 `true` 计，同迁移前）。
    fn effective_duplicate(&self) -> bool {
        self.option_roles
            .key(ROLE_ALLOW_DUPLICATE_SINGLE)
            .and_then(|name| self.option_value(name))
            .unwrap_or(true)
    }

    /// 下发配置袋（设置派生的角色 + 运行时选项的生效值），方案据此自算学习 mode。
    ///
    /// 按键路径每次都会问一次：设置未变且单字重码值不变时直接返回，不重建配置袋。
    pub(crate) fn push_scheme_config(&mut self) {
        let duplicate = self.effective_duplicate();
        if !self.config_dirty && self.applied_duplicate == Some(duplicate) {
            return;
        }
        self.config_dirty = false;
        self.applied_duplicate = Some(duplicate);
        let config =
            scheme_config(&self.settings).with(ROLE_ALLOW_DUPLICATE_SINGLE, Value::Bool(duplicate));
        self.apply_scheme_config(config);
    }

    /// 下发一个配置袋并收录诊断。
    ///
    /// 方案的 `apply_config` 返回逐角色诊断（角色缺失 / 类型不符）：方案已按缺省值回退，
    /// 平台把诊断并入状态串（与装配期 `config:` 诊断同风格），避免运行期静默降级。
    pub(crate) fn apply_scheme_config(&mut self, config: SchemeConfig) {
        let result = self.scheme.apply_config(&config);
        let notes: Vec<String> = match result {
            Ok(()) => Vec::new(),
            Err(errors) => errors
                .iter()
                .map(|error| format!("config: {error}"))
                .collect(),
        };
        if notes != self.config_notes {
            self.config_notes = notes;
            self.refresh_status();
        }
    }
}

/// 事件泵每轮按键的最大轮数（选项事件可能触发确认，进而产生新事件）。
pub(crate) const EVENT_PUMP_ROUNDS: usize = 4;

/// 解析方案的选项声明：返回（角色 → 键表, 可选错误诊断）。
///
/// **全有或全无**：`OptionKeys::resolve` 只要发现任一问题（缺角色 / 重复 / 空声明）即 `Err`，
/// 本函数随之返回 `OptionKeys::default()`——**所有**角色都不接线（诊断进状态串，列出问题清单），
/// 而不是「只让出问题的那个角色不接线」。宿主菜单与持久化于是跳过全部角色选项，
/// 绝不静默落到别的键上。
pub(crate) fn resolve_option_roles(declarations: &[OptionDecl]) -> (OptionKeys, Option<String>) {
    match OptionKeys::resolve(declarations) {
        Ok(roles) => (roles, None),
        Err(error) => (OptionKeys::default(), Some(error.to_string())),
    }
}

/// 配置页热键绑定里**无法解析为 rime 键名**的项（返回 `角色=键名` 列表）。
///
/// 反向路径：ABI 把 fcitx5 的 `keysym + 状态位` 经 `KeyEvent::repr()` 转成键名交给本层，
/// 而配置页可以绑到**没有名字的 keysym**（媒体键 / 厂商扩展键）：`repr()` 只能输出
/// `0x1008ff14` / `(unknown)` 这类形式，`KeyEvent::from_repr` 不认 ⇒ 该绑定在
/// `Settings::host_options` 与方案 `host_options_from` 的 `filter_map` 处**静默消失**。
/// 这里点名，进 `hux_engine_status`（`hotkeys:` 前缀；C++ 壳在应用设置后落日志）。
pub(crate) fn unparsable_key_bindings(settings: &Settings) -> Vec<String> {
    let mut notes = Vec::new();
    for (role, reprs) in [
        (
            ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            &settings.reverse_lookup_pronunciation_keys,
        ),
        (
            ROLE_REVERSE_LOOKUP_CHARACTER_KEYS,
            &settings.reverse_lookup_character_keys,
        ),
        (ROLE_PAGE_UP_KEYS, &settings.page_up_keys),
        (ROLE_PAGE_DOWN_KEYS, &settings.page_down_keys),
    ] {
        for repr in reprs
            .iter()
            .filter(|repr| KeyEvent::from_repr(repr).is_none())
        {
            notes.push(format!("{role}={repr}"));
        }
    }
    notes
}

/// hux 自身设置 → 方案配置袋（平台是装配根：只有这里知道「设置 → 角色」的对应关系）。
///
/// 角色词汇归 `hux-cfg`；本函数只搬运设置值，方案的运行时选项值（单字重码）由
/// [`Engine::push_scheme_config`] 追加。
pub(crate) fn scheme_config(settings: &Settings) -> SchemeConfig {
    let host = settings.host_options();
    SchemeConfig::new()
        .with(ROLE_HIGH_FREQ_LIMIT, Value::Count(settings.high_freq_limit))
        .with(
            ROLE_MIN_RETAINED_INPUT_LENGTH,
            Value::Count(settings.min_retained()),
        )
        .with(ROLE_PAGE_SIZE, Value::Count(host.page_size))
        .with(ROLE_PAGE_CYCLE, Value::Bool(host.page_cycle))
        .with(
            ROLE_PAGE_UP_KEYS,
            Value::Texts(settings.page_up_keys.clone()),
        )
        .with(
            ROLE_PAGE_DOWN_KEYS,
            Value::Texts(settings.page_down_keys.clone()),
        )
        .with(
            ROLE_REVERSE_LOOKUP_PRONUNCIATION_KEYS,
            Value::Texts(settings.reverse_lookup_pronunciation_keys.clone()),
        )
        .with(
            ROLE_REVERSE_LOOKUP_CHARACTER_KEYS,
            Value::Texts(settings.reverse_lookup_character_keys.clone()),
        )
        .with(ROLE_LEARNING_ON_TAB, Value::Bool(settings.learning_on_tab))
}

/// 模型路径的**来源**：构造与「重新部署」按同一来源重新解析。
///
/// 「重新部署」要能拿到新装入的模型，故默认查找（[`ModelSource::Auto`]）在重新部署时
/// 重新查找；而显式指定的路径（`HUX_MODEL` / 调用方传入）保持权威，不会退化成默认查找。
#[derive(Clone, Debug)]
pub(crate) enum ModelSource {
    /// 显式指定的模型路径（`HUX_MODEL` 或调用方传入）：重新部署沿用。
    Fixed(PathBuf),
    /// 未指定：在各数据目录里查找方案声明的模型资产（重新部署时重新查找）。
    Auto,
}

impl ModelSource {
    /// 解析出模型路径（`None` = 不装载模型）。
    pub(crate) fn resolve(&self, dirs: &[PathBuf]) -> Option<PathBuf> {
        match self {
            Self::Fixed(path) => Some(path.clone()),
            Self::Auto => default_model_path(dirs, ASSETS),
        }
    }
}

/// 装配方案（数据 + 模型）并解析选项角色：构造与「重新部署」共用同一条路径
/// （两处各拼一份时漏一项即成为「重新部署后配置没下发」这类哑失败）。
///
/// `notes` 是装配诊断基线（数据目录 + 装载说明 + 角色解析错误），进状态串。
pub(crate) fn assemble_scheme(
    dirs: &[PathBuf],
    model: Option<PathBuf>,
    config: &SchemeConfig,
) -> (TigerScheme, OptionKeys, Vec<String>) {
    let mut notes = vec![format!(
        "dirs: {}",
        dirs.iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    )];
    let (scheme, scheme_notes) = TigerScheme::load(dirs, model, config);
    notes.extend(scheme_notes);
    // 选项键的唯一来源 = 方案的声明；**缺角色即报错**（状态串可见），缺的角色不参与
    // 选项接线（无键 → 宿主跳过该项），不静默落到别的键上。
    let (option_roles, roles_error) = resolve_option_roles(scheme.option_declarations());
    if let Some(error) = roles_error {
        notes.push(format!("options: {error}"));
    }
    (scheme, option_roles, notes)
}

pub struct Engine {
    pub(crate) host: Option<HostCallback>,
    /// 方案（平台经 `dyn Scheme` 驱动，不直接引用方案模块；共享资源与会话态都在方案内）。
    pub(crate) scheme: Box<dyn Scheme>,
    pub(crate) sessions: HashMap<u64, Session>,
    pub(crate) next_session: u64,
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    pub(crate) options: Option<OptionsStore>,
    /// 外部配置（fcitx5 配置界面 / 测试；默认 = 内建缺省）。
    pub(crate) settings: Settings,
    /// 学习库（用户目录不可用时为禁用占位）。
    pub(crate) learning: LearningStore,
    /// 角色 → 选项键（装配处由方案声明解析；缺角色即报错，见 [`Engine::new_with_dirs`]）。
    pub(crate) option_roles: OptionKeys,
    /// 角色序（= [`RUNTIME_OPTION_ROLES`]）的选项键 C 字符串；缺失角色为 `None`
    /// （`hux_engine_option_key` 返回 NULL，宿主跳过该项）。
    pub(crate) option_keys: Vec<Option<CString>>,
    /// 设置派生的配置袋是否需要重下发（`apply_settings` 置位）。
    config_dirty: bool,
    /// 上次下发的单字重码生效值（`None` = 尚未下发）。
    applied_duplicate: Option<bool>,
    /// 本次按键「已提交且未消费」：宿主层应消费该键并以 `forwardKey` 重发，
    /// 保证「提交 → 按键」送达顺序（对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
    pub forward_after_commit: bool,
    pub(crate) status: CString,
    /// 状态串基线（构造时的加载说明；选项保存出错时拼在其后）。
    pub(crate) status_base: String,
    /// 选项保存失败的最近一条诊断（来自 [`hux_cfg::OPTIONS_ERROR_PROPERTY`]）。
    pub(crate) option_error: Option<String>,
    /// 学习库的**当前**诊断（构造期读一次，运行期落库失败由 [`Engine::observe_learning_error`] 跟进）。
    pub(crate) learning_error: Option<String>,
    /// 构造期已并入 `status_base` 的那条学习诊断（避免与运行期条目重复拼接）。
    pub(crate) learning_error_baseline: Option<String>,
    /// 配置页热键绑定里无法解析的项（`角色=键名`，见 [`unparsable_key_bindings`]）。
    pub(crate) hotkey_notes: Vec<String>,
    /// 最近一次配置下发的逐角色诊断（角色缺失 / 类型不符，`config:` 前缀）。
    /// 方案已按缺省值回退，此串只是把「设置没生效」的原因暴露到状态里。
    pub(crate) config_notes: Vec<String>,
    /// 只读数据目录（构造时解析；「重新部署」据此重新查找模型资产）。
    dirs: Vec<PathBuf>,
    /// 模型路径来源（见 [`ModelSource`]）。
    model_source: ModelSource,
    /// 模型摘要（`hux_engine_model_info` 的指针来源）：**重新部署后替换**，
    /// 此前返回的指针随即失效（同 `status` 的契约）。
    pub(crate) model_info: CString,
}
impl Engine {
    pub(crate) fn new(host: Option<HostCallback>) -> Self {
        let dirs = data_dirs();
        // 选项存于标准用户目录（与数据目录的开发覆盖解耦）。
        let options_dir = user_data_dir();
        // `HUX_MODEL` 显式覆盖（此时路径固定）；未设置则按数据目录查找
        // （`Auto` ⇒「重新部署」会重新查找，新装入的模型随之生效）。
        let model_source = match std::env::var_os("HUX_MODEL") {
            Some(path) => ModelSource::Fixed(PathBuf::from(path)),
            None => ModelSource::Auto,
        };
        Self::with_model_source(host, dirs, model_source, options_dir)
    }

    /// 测试用构造：目录 / 模型 / 选项目录全部显式注入。
    ///
    /// 模型传 `None` 即「未指定」⇒ 走默认查找（各数据目录里的方案模型资产）；夹具目录
    /// 里都没有模型文件，故与「不装模型」同效，而重新部署时按同一来源重新查找。
    #[cfg(test)]
    pub(crate) fn new_with_dirs(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        let model_source = match model_path {
            Some(path) => ModelSource::Fixed(path),
            None => ModelSource::Auto,
        };
        Self::with_model_source(host, dirs, model_source, options_dir)
    }

    fn with_model_source(
        host: Option<HostCallback>,
        dirs: Vec<PathBuf>,
        model_source: ModelSource,
        options_dir: Option<PathBuf>,
    ) -> Self {
        let settings = Settings::default();
        // 构造方案前先按设置装配配置袋；单字重码的初始值取设置缺省（尚无会话与存储）。
        let applied_duplicate = settings.allow_duplicate_single;
        let initial = scheme_config(&settings)
            .with(ROLE_ALLOW_DUPLICATE_SINGLE, Value::Bool(applied_duplicate));
        let (mut scheme, option_roles, mut notes) =
            assemble_scheme(&dirs, model_source.resolve(&dirs), &initial);
        // 选项：有存储则同步（参照 `M.options.sync`，同步写入由核心抑制观察）；
        // 无存储时直接用内建缺省。会话创建时逐个同步（见 `session_new`）。
        let options = options_dir.as_deref().map(|dir| {
            OptionsStore::load_with_defaults(dir, settings.store_defaults(&option_roles))
        });
        // 学习库：`<user dir>/<方案 id 哈希>.userdb/`（用户目录不可用则禁用）。
        let learning = match options_dir.as_deref() {
            Some(dir) => {
                LearningStore::open(dir, &learning_store::store_name(scheme.id()), wall_clock())
            }
            None => LearningStore::disabled("user data directory unavailable"),
        };
        // 构造期诊断进 `status_base`；运行期变化由 `observe_learning_error` 补进状态串
        // （此处留一份基线快照，避免同一条错误被拼两次）。
        let learning_error = learning.error.clone();
        if let Some(error) = &learning.error {
            notes.push(format!("learning: {error}"));
        } else {
            notes.push(format!("learning: {}", learning.name));
        }
        scheme.set_store_ready(learning.store_ready());
        let status_base = notes.join("; ");
        // 角色顺序与 `hux_abi.h` 的 `HUX_OPTION_*` 一致（ABI 边界用角色，不暴露方案键名）。
        let option_keys = RUNTIME_OPTION_ROLES
            .iter()
            .map(|role| option_roles.key(role).map(crate::ui::cstring_lossy))
            .collect();
        let model_info = crate::ui::cstring_lossy(scheme.model_info());
        Self {
            host,
            scheme: Box::new(scheme),
            sessions: HashMap::new(),
            next_session: 1,
            options,
            settings,
            learning,
            option_roles,
            option_keys,
            config_dirty: false,
            applied_duplicate: Some(applied_duplicate),
            forward_after_commit: false,
            status: crate::ui::cstring_lossy(&status_base),
            status_base,
            option_error: None,
            learning_error_baseline: learning_error.clone(),
            learning_error,
            hotkey_notes: Vec::new(),
            config_notes: Vec::new(),
            dirs,
            model_source,
            model_info,
        }
    }

    /// 新建会话（一个输入上下文注册/激活时创建）；选项/触发键/学习模式按共享状态初始化。
    pub fn session_new(&mut self) -> u64 {
        let mut context = Context::new();
        // 宿主缺省：`_auto_commit`（librime `express_editor` 默认 true）。
        context.set_option("_auto_commit", true);
        // 先写设置缺省，再由 `options.yaml` 持久化值覆盖。
        // 分流：**可持久化项**（存储有声明的缺省）只经 `store.sync` 写入——其写入带抑制名单，
        // 不会被随后的选项事件当成用户改动写进 `options.yaml`；其余（如 `ascii_punct`）直接写。
        // 参照实现同此：缺省经 `M.options.sync` 写入并由 `live.syncing` 抑制。
        for (name, value) in self.settings.option_defaults(&self.option_roles) {
            if self
                .options
                .as_ref()
                .is_some_and(|store| store.covers(name))
            {
                continue;
            }
            context.set_option(name, value);
        }
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut context);
        }
        // 运行时开关（状态菜单切换、不落盘的项）一律**以现存会话为模板**，避免新会话回退到设置缺省。
        // 本块与是否存在存储**无关**（上面的 `store.covers` 门只作用于设置项），故不能读作「无存储时才走」。
        // 取 `values().next()`（任一会话）是安全的：运行时开关在同一引擎内由同一份状态菜单维护、各会话恒等，
        // 因此结果与选取哪个会话无关（幂等）。若将来运行时开关允许按会话分叉，这里必须换成显式单一来源。
        if let Some(existing) = self.sessions.values().next() {
            for name in self.runtime_options() {
                let value = existing.context.get_option(name);
                if context.get_option(name) != value {
                    context.set_option(name, value);
                }
            }
        }
        let scheme_session = self.scheme.new_session(&mut context);
        let mut session = Session {
            context,
            scheme_session,
            reverse_lookup: ReverseLookupState::default(),
        };
        self.apply_layout_options(&mut session);
        let id = self.next_session;
        self.next_session += 1;
        self.sessions.insert(id, session);
        id
    }

    /// 释放会话（输入上下文销毁时）。
    pub fn session_free(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.remove(&session_id) {
            self.scheme.free_session(session.scheme_session);
        }
    }

    /// 取会话执行闭包后放回（借用拆分：会话与共享状态互不重叠）。
    pub(crate) fn with_session<R>(
        &mut self,
        session_id: u64,
        f: impl FnOnce(&mut Self, &mut Session) -> R,
    ) -> Option<R> {
        let mut session = self.sessions.remove(&session_id)?;
        let result = f(self, &mut session);
        self.sessions.insert(session_id, session);
        Some(result)
    }

    /// 处理一次按键：返回是否消费；副作用（提交/preedit/候选）经宿主回调送出。
    ///
    /// 提交且未消费（`express_editor` 的 `char_handler = DirectCommit`）时置
    /// [`Engine::forward_after_commit`]：宿主层据此消费该键并以 `forwardKey` 重发，
    /// 保证客户端先收到提交、后收到按键。
    pub fn key(&mut self, session_id: u64, keysym: u32, states: u32, release: bool) -> bool {
        match self.with_session(session_id, |engine, session| {
            engine.key_in(session, keysym, states, release)
        }) {
            Some(consumed) => consumed,
            None => {
                // 未知 / 已释放会话不得沿用**上一次**按键留下的粘性转发位——否则
                // `hux_engine_key` 会只回 `HUX_KEY_FORWARD_AFTER_COMMIT`（无 CONSUMED），
                // 宿主据此 `filterAndAccept` + `forwardKey` 一个并不存在的提交。
                self.forward_after_commit = false;
                false
            }
        }
    }

    pub(crate) fn key_in(
        &mut self,
        session: &mut Session,
        keysym: u32,
        states: u32,
        release: bool,
    ) -> bool {
        self.forward_after_commit = false;
        let key = KeyEvent::new(keysym as i32, core_modifiers(states, release));
        // 字反查段：←/→/↑/↓ **交应用处理**（应用光标随动），本层不消费也不改动输入；
        // 两排在应用回传周边文本后的下一次按键（含 release）时刷新。
        if !release
            && self.scheme.auxiliary_lookup_active(&session.context)
            && matches!(key.repr().as_str(), "Left" | "Right" | "Up" | "Down")
        {
            return false;
        }
        let now = wall_clock();
        // 方案内部完成：处理器（翻译/锁/早提交）→ 未消费则宿主链（含提交点学习）。
        let consumed =
            match self
                .scheme
                .process_key(session.scheme_session, &mut session.context, &key, now)
            {
                Ok(KeyOutcome::Consumed) => true,
                Ok(KeyOutcome::Forward) => false,
                Err(error) => {
                    eprintln!("hux: processor error: {error}");
                    false
                }
            };
        self.finish(session, now, Some(consumed));
        consumed
    }

    /// 候选点击（面板候选 `CandidateWord::select`，按会话）：按全局索引选中并上屏。
    /// 走与 `space` 相同的确认/学习链；越界/无可选段返回 `false`。
    pub fn select_candidate(&mut self, session_id: u64, index: usize) -> bool {
        match self.with_session(session_id, |engine, session| {
            engine.select_candidate_in(session, index)
        }) {
            Some(selected) => selected,
            None => {
                // 同 `key`：候选点击也走 `hux_engine_key` 之外的路径，粘性位必须清掉。
                self.forward_after_commit = false;
                false
            }
        }
    }

    pub(crate) fn select_candidate_in(&mut self, session: &mut Session, index: usize) -> bool {
        self.forward_after_commit = false;
        let now = wall_clock();
        match self
            .scheme
            .select_candidate(session.scheme_session, &mut session.context, index, now)
        {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                eprintln!("hux: select candidate error: {error}");
                return false;
            }
        }
        self.finish(session, now, None);
        true
    }

    /// 提交泵 + 学习落库 + 组合重建 + UI 刷新（按键与候选点击共用）。
    ///
    /// `key_forward`：按键路径传入消费结果（据提交计算 `forward_after_commit`）；
    /// 候选点击传 `None`（非按键路径，恒不转发）。
    pub(crate) fn finish(&mut self, session: &mut Session, now: f64, key_forward: Option<bool>) {
        let mut commits = Vec::new();
        let mut invalidated = false;
        // 事件泵：选项事件可能触发确认（进而产生提交），循环至排空。
        // 上限是防御性的（选项事件链不可能无限展开）；超限的残余事件留到下一次按键处理。
        for _ in 0..EVENT_PUMP_ROUNDS {
            let events = session.context.drain_events();
            if events.is_empty() {
                break;
            }
            for event in events {
                match event {
                    Event::Commit(text) => {
                        invalidated = true;
                        commits.push(text);
                    }
                    Event::Option(name) => {
                        self.observe_option(&mut session.context, &name);
                    }
                    Event::Update => {}
                }
            }
        }
        let committed = !commits.is_empty();
        for text in commits {
            self.host_commit(&text);
        }
        self.forward_after_commit = match key_forward {
            Some(consumed) => !consumed && committed,
            None => false,
        };
        // 学习：核心暂存 → 落库；刷新打分（未组合时，60 秒节流）；应用索引。
        let submitted = self.scheme.take_learning_events(session.scheme_session);
        if !submitted.is_empty() {
            self.learning.confirm(&submitted);
        }
        // 运行期落库失败（磁盘满 / 库被改成只读 / 锁异常）不进状态串的话，用户只看到
        // 「学习不生效」。
        self.observe_learning_error();
        self.push_scheme_config();
        if !session.context.is_composing() {
            self.learning.refresh_scores(now);
        }
        let version = self.learning.index_version();
        self.scheme
            .apply_learning_index(session.scheme_session, version, self.learning.index());
        // 组合重建（参照 `ConcreteEngine::Compose`，含 update 通知器）
        // （非组合清暂存；缓冲且实况输入为空时隐藏候选）。
        if let Err(error) =
            self.scheme
                .rebuild(session.scheme_session, &mut session.context, invalidated)
        {
            eprintln!("hux: rebuild error: {error}");
        }
        self.refresh_reverse_lookup_aux(session);
        self.push_update(session);
    }

    /// 当前组合末段是否为字反查段（进入/退出由方案处理器负责：触发字符推入/清空组合）；
    /// 查码段内方向键交应用处理（见 `key_in` 开头的早退）。
    /// 当前组合末段是否为字反查段。
    pub(crate) fn reverse_lookup_tagged(&self, session: &Session) -> bool {
        self.scheme.auxiliary_lookup_active(&session.context)
    }

    /// 重算两排提示（上排 = 光标左侧拼音、下排 = 虎码）；不在查码段则清空。
    pub(crate) fn refresh_reverse_lookup_aux(&mut self, session: &mut Session) {
        let tagged = self.reverse_lookup_tagged(session);
        let state = &mut session.reverse_lookup;
        if !tagged || !state.valid {
            // 不在查码段，或周边文本不可用（如终端）：提示能否呈现取决于前端，
            // 统一清空两排。
            state.aux_up.clear();
            state.aux_down.clear();
            return;
        }
        let (up, down) = self.scheme.auxiliary_rows(&state.text, state.cursor);
        state.aux_up = up;
        state.aux_down = down;
    }

    /// 宿主送入应用侧周边文本（字符制光标；`None` = 应用不支持/不可用）。
    pub fn set_surrounding(&mut self, session_id: u64, text: Option<&str>, cursor_chars: usize) {
        self.with_session(session_id, |engine, session| {
            engine.set_surrounding_in(session, text, cursor_chars);
        });
    }

    pub(crate) fn set_surrounding_in(
        &mut self,
        session: &mut Session,
        text: Option<&str>,
        cursor_chars: usize,
    ) {
        let state = &mut session.reverse_lookup;
        match text {
            Some(text) => {
                state.valid = true;
                state.text = text.to_string();
                state.cursor = cursor_chars.min(state.text.chars().count());
            }
            None => {
                state.valid = false;
                state.text.clear();
                state.cursor = 0;
            }
        }
        if self.reverse_lookup_tagged(session) {
            self.refresh_reverse_lookup_aux(session);
        }
    }

    /// 重置会话（`deactivate`/`reset`；组合不跨输入上下文保留）。
    pub fn reset(&mut self, session_id: u64) {
        self.with_session(session_id, |engine, session| engine.reset_in(session));
    }

    pub(crate) fn reset_in(&mut self, session: &mut Session) {
        // 契约：重置即丢弃——先清掉未派发的事件（含可能的提交），避免下次按键补上屏。
        session.context.drain_events();
        session.reverse_lookup = ReverseLookupState::default();
        session.context.clear();
        self.scheme
            .reset_session(session.scheme_session, &mut session.context);
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.push_update(session);
    }

    /// 选项事件 → 记录/持久化（参照 `M.options` 的选项通知器）。
    pub(crate) fn observe_option(&mut self, context: &mut Context, name: &str) {
        if let Some(options) = self.options.as_mut() {
            options.observe(context, name);
        }
        // 保存失败会写入 [`hux_cfg::OPTIONS_ERROR_PROPERTY`]：并入状态串，
        // 使 `hux_engine_status` 能反映出来（此前该属性全仓无读取方）。
        let error = context
            .get_property(hux_cfg::OPTIONS_ERROR_PROPERTY)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if error != self.option_error {
            self.option_error = error;
            self.refresh_status();
        }
    }

    /// 重建状态串（基线 + 配置诊断 + 选项保存错误 + 运行期学习库错误 + 热键绑定诊断，若有）。
    pub(crate) fn refresh_status(&mut self) {
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

    /// 学习库诊断变化 → 并入状态串。
    ///
    /// 此前 `learning.error` 只在 `new_with_dirs` 里读一次：打开失败可见，而**运行期写盘失败**
    /// （LevelDB `put` 返回错误）既无日志也不进 `hux_engine_status`。这里在落库路径上调一次，
    /// 诊断变化即刷新状态串（与 `*_options_error` 同风格）。
    pub(crate) fn observe_learning_error(&mut self) {
        let current = self.learning.error.clone();
        if current != self.learning_error {
            self.learning_error = current;
            self.refresh_status();
        }
    }

    /// 运行时开关当前值（状态菜单；全局）：任一会话的生效值，无会话时回退存储/设置缺省。
    pub fn option_value(&self, name: &str) -> Option<bool> {
        if !self.is_runtime_option(name) {
            return None;
        }
        if let Some(session) = self.sessions.values().next() {
            return Some(session.context.get_option(name));
        }
        self.options
            .as_ref()
            .and_then(|store| store.value(name))
            .or_else(|| self.settings.option_default(&self.option_roles, name))
    }

    /// 设置运行时开关（状态菜单）：白名单校验 → 写入全部会话 → 持久化（`options.yaml`）。
    pub fn set_option_value(&mut self, name: &str, value: bool) -> bool {
        if !self.is_runtime_option(name) {
            return false;
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        if ids.is_empty() {
            // 无会话（尚未绑定输入上下文）：直写存储，避免状态菜单切换被静默丢弃。
            let saved = self
                .options
                .as_mut()
                .map(|store| store.set_value(name, value));
            self.push_scheme_config();
            return saved.unwrap_or(true);
        }
        for id in ids {
            self.with_session(id, |engine, session| {
                if session.context.get_option(name) != value {
                    session.context.set_option(name, value);
                }
                engine.observe_option(&mut session.context, name);
            });
        }
        self.push_scheme_config();
        true
    }

    /// 应用外部配置（fcitx5 配置界面 / 测试）：全部项即时生效（含高频字上限——方案据此重建词库）。
    /// 顺序：设置写入缺省 → `options.yaml` 持久化值覆盖（含状态菜单开关）→ 触发键/学习模式刷新。
    /// 作用于全部会话（选项为引擎级）。
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.config_dirty = true;
        // 配置页热键绑定里无法解析的项：点名（此前在 `filter_map` 处静默消失）。
        let hotkey_notes = unparsable_key_bindings(&self.settings);
        if hotkey_notes != self.hotkey_notes {
            self.hotkey_notes = hotkey_notes;
            self.refresh_status();
        }
        let defaults = self.settings.option_defaults(&self.option_roles);
        if let Some(store) = self.options.as_mut() {
            store.set_defaults(self.settings.store_defaults(&self.option_roles));
        }
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| {
                // 同 `session_new`：可持久化项交由 `store.sync`（带抑制），其余直接写。
                for (name, value) in &defaults {
                    if engine
                        .options
                        .as_ref()
                        .is_some_and(|store| store.covers(name))
                    {
                        continue;
                    }
                    if session.context.get_option(name) != *value {
                        session.context.set_option(name, *value);
                    }
                }
                if let Some(store) = engine.options.as_mut() {
                    store.sync(&mut session.context);
                }
                engine.apply_layout_options(session);
            });
        }
        // 设置 + 运行时选项一起下发（含学习 mode 自算），见 [`Engine::push_scheme_config`]。
        self.push_scheme_config();
    }

    /// 候选布局（context 选项）：host `selector` 读取 `_vertical` 决定 ←→/↑↓ 语义。
    /// 仅「竖排」置位；「横排/跟随全局」维持横排键语义（面板排列见 C++ `LayoutHint`）。
    pub(crate) fn apply_layout_options(&self, session: &mut Session) {
        let vertical = self.settings.candidate_layout == CandidateLayout::Vertical;
        if session.context.get_option("_vertical") != vertical {
            session.context.set_option("_vertical", vertical);
        }
    }

    /// 重新部署：按当前设置重新装配方案（数据 + 模型），并**重置全部现有会话状态**。
    ///
    /// 平台侧会话 id 不变（宿主的输入上下文与 id 的对应关系保持，IC 不需要重建），
    /// 方案侧会话全部重建 ⇒ 组合、候选、学习暂存、反查态一并作废（宿主负责清面板）。
    /// 模型路径按**同一来源**重新解析：默认查找会拿到新装入的模型，显式指定（`HUX_MODEL`）
    /// 则沿用原路径。返回 `true` = 已重新装配。
    pub fn redeploy(&mut self) -> bool {
        // 模型路径：与构造同一来源（见 [`ModelSource`]）。
        let model = self.model_source.resolve(&self.dirs);
        // 配置袋与构造同源：设置派生的角色 + 运行时选项的生效值（单字重码）。
        let config = scheme_config(&self.settings).with(
            ROLE_ALLOW_DUPLICATE_SINGLE,
            Value::Bool(self.effective_duplicate()),
        );
        let (mut scheme, option_roles, mut notes) = assemble_scheme(&self.dirs, model, &config);
        // 学习库沿用（不重开）：就绪状态与诊断按构造期同一口径并入基线，
        // 使新方案拿到 store_ready，状态串也不因重新部署而丢掉这条诊断。
        if let Some(error) = &self.learning.error {
            notes.push(format!("learning: {error}"));
        } else {
            notes.push(format!("learning: {}", self.learning.name));
        }
        scheme.set_store_ready(self.learning.store_ready());
        // 释放旧方案的会话（平台侧 id 与宿主输入上下文不受影响），再换上重新装配的方案。
        let old_sessions: Vec<_> = self
            .sessions
            .values()
            .map(|session| session.scheme_session)
            .collect();
        for scheme_session in old_sessions {
            self.scheme.free_session(scheme_session);
        }
        self.scheme = Box::new(scheme);
        // 角色表随方案重新解析（角色缺失时宿主菜单跳过该项，诊断进状态串）。
        self.option_roles = option_roles;
        self.option_keys = RUNTIME_OPTION_ROLES
            .iter()
            .map(|role| self.option_roles.key(role).map(crate::ui::cstring_lossy))
            .collect();
        self.status_base = notes.join("; ");
        // 逐角色诊断由随后的配置下发重新产出，先清掉旧方案的（避免拼出过期诊断）。
        self.config_notes.clear();
        self.refresh_status();
        // 逐会话重置：平台 id 保留，方案侧会话重建（触发键 / 最小保留量随新方案刷新）。
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            self.with_session(id, |engine, session| engine.redeploy_session(session));
        }
        // 新方案拿到配置袋（与构造同源；学习索引在下一次按键时下发）。
        self.config_dirty = true;
        self.applied_duplicate = None;
        self.push_scheme_config();
        // 模型摘要：指针在此替换（此前返回的指针随即失效，见 `hux_abi.h`）。
        self.model_info = crate::ui::cstring_lossy(self.scheme.model_info());
        true
    }

    /// 重新部署时重置单个会话：平台 id 不变，方案侧会话重建。
    ///
    /// 顺序与 [`Engine::reset_in`] 一致（先丢弃未派发事件，再清组合与反查态）；
    /// 不推 UI 快照——宿主在重新部署动作里统一清面板（旧候选已随会话作废）。
    fn redeploy_session(&mut self, session: &mut Session) {
        session.context.drain_events();
        session.reverse_lookup = ReverseLookupState::default();
        session.context.clear();
        session.scheme_session = self.scheme.new_session(&mut session.context);
        if let Some(options) = self.options.as_mut() {
            options.sync(&mut session.context);
        }
        self.apply_layout_options(session);
    }

    /// 提交回调（`engine:commit_text`）。
    pub(crate) fn host_commit(&self, text: &str) {
        let Some(host) = &self.host else {
            return;
        };
        let Some(commit) = host.commit else {
            return;
        };
        // SAFETY: 函数指针与 `user` 由宿主提供且在本调用期间有效。
        let text = crate::ui::cstring_lossy(text);
        unsafe { commit(host.user, text.as_ptr()) };
    }
}
