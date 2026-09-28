// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 装配：目录 / 模型 / 方案数据 / 选项存储 / 学习库的来源与解析（构造与「重新部署」共用）。

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hux_cfg::roles::OptionKeys;
use hux_cfg::{OptionsStore, Settings};
use hux_core::scheme::{Scheme, SchemeConfig, Value};
use hux_scheme_tiger::scheme::{ASSETS, TigerScheme};

use crate::engine::config::{RuntimeOptions, resolve_option_roles, scheme_config};
use crate::learning_store::{self, LearningStore};
use crate::paths::{data_dirs, default_model_path, user_data_dir};

/// 平台层读系统时钟（内核不读时钟）。
pub(crate) fn wall_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as f64)
        .unwrap_or(0.0)
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

/// 装配输入：数据目录 / 选项目录 / 模型来源，以及「目录是否按进程环境解析」。
///
/// 构造与「重新部署」共用本结构：[`Assembly::resolve_dirs`] 按同一规则取目录，
/// [`Assembly::load`] 按同一顺序装载方案数据 → 选项存储 → 学习库。
pub(crate) struct Assembly {
    /// 「重新部署」是否按进程环境重算目录（生产 `true`；测试注入固定目录时为 `false`）。
    ///
    /// 生产路径按 [`data_dirs`] / [`user_data_dir`] 解析（`HUX_DATA_DIRS` 覆盖、否则 XDG 规则）；
    /// 这些环境变量都是进程级的、在同一进程内不会变，故重新部署重算得到的仍是同一组路径——
    /// 重算的意义是**重新走一遍构造期的读取**，而不是换一组值。测试注入的目录固定不变，
    /// 但其**内容**同样会重读（「替换数据文件后重新部署」据此可测）。
    dirs_from_env: bool,
    /// 当前只读数据目录（构造解析；注入来源时保持不变）。
    dirs: Vec<PathBuf>,
    /// 当前选项目录（同上；`None` = 无用户目录：仅用内建缺省、学习库禁用）。
    options_dir: Option<PathBuf>,
    /// 模型路径来源（见 [`ModelSource`]）。
    model_source: ModelSource,
}

/// 一次装配的产物：[`Assembly::load`] 的返回口径（构造与「重新部署」共用）。
pub(crate) struct Assembled {
    /// 已装载的方案（尚未装入引擎）。
    pub(crate) scheme: TigerScheme,
    /// 角色 → 选项键（由方案声明解析；缺角色即报错，见 [`resolve_option_roles`]）。
    pub(crate) option_roles: OptionKeys,
    /// 选项存储（用户目录不可用时为 `None`，此时仅用内建缺省）。
    pub(crate) options: Option<OptionsStore>,
    /// 学习库（用户目录不可用时为禁用占位）。
    pub(crate) learning: LearningStore,
    /// 学习库的**当前**诊断（装配时读一次；运行期落库失败由诊断层跟进）。
    pub(crate) learning_error: Option<String>,
    /// 「打开模型目录」入口的路径（UTF-8 串，`None` = 给不出任何路径 ⇒ ABI 返回 NULL）。
    pub(crate) model_path: Option<CString>,
    /// 模型摘要（`hux_engine_model_info` 的指针来源）。
    pub(crate) model_info: CString,
    /// 装配诊断基线（数据目录 + 装载说明 + 模型 + 角色解析错误 + 学习库）。
    pub(crate) notes: Vec<String>,
}

impl Assembly {
    /// 生产构造：目录按进程环境解析（`HUX_DATA_DIRS` 覆盖、否则 XDG 规则），重新部署时重算。
    pub(crate) fn from_env(model_source: ModelSource) -> Self {
        Self {
            dirs_from_env: true,
            dirs: data_dirs(),
            options_dir: user_data_dir(),
            model_source,
        }
    }

    /// 按指定目录构造：目录 / 模型 / 选项目录全部显式注入，**不经 XDG 缺省**（来源记为注入，
    /// 故「重新部署」沿用它们）。供测试与平台内装配使用；生产装配走 [`Assembly::from_env`]。
    pub(crate) fn injected(
        dirs: Vec<PathBuf>,
        model_path: Option<PathBuf>,
        options_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            dirs_from_env: false,
            dirs,
            options_dir,
            model_source: Self::model_source(model_path),
        }
    }

    /// 模型来源口径（唯一）：给出路径即固定来源，未给出则按数据目录查找。
    pub(crate) fn model_source(model_path: Option<PathBuf>) -> ModelSource {
        match model_path {
            Some(path) => ModelSource::Fixed(path),
            None => ModelSource::Auto,
        }
    }

    /// 目录的来源：按进程环境重算（注入来源时保持不变）。构造与「重新部署」同一规则。
    pub(crate) fn resolve_dirs(&mut self) {
        if self.dirs_from_env {
            self.dirs = data_dirs();
            self.options_dir = user_data_dir();
        }
    }

    /// 选项目录是否可用。不可用 ⇒ 学习库是禁用占位（没有句柄要释放，也没有库文件要重读）。
    pub(crate) fn has_options_dir(&self) -> bool {
        self.options_dir.is_some()
    }

    /// 从当前目录 / 模型来源装齐方案数据、选项存储与学习库：构造与「重新部署」唯一装配路径。
    ///
    /// 顺序即读取顺序：模型路径 → 方案数据（词库 / 词先验 / 标点 / 模型）→ 选项存储
    /// （重读 `options.yaml`）→ 学习库（重开，重读 `e/` 事件）。运行时开关的生效值由调用方
    /// 按同一口径取好（`runtime`；构造期尚无会话与存储，取设置缺省）。
    ///
    /// 调用方须保证旧学习库句柄已释放——同一路径二次打开会撞上 LevelDB 的独占锁
    /// （rusty-leveldb 的 `LOCK`）；库名依赖方案 id，故按**新**方案的 id 打开。
    pub(crate) fn load(&self, settings: &Settings, runtime: RuntimeOptions) -> Assembled {
        // 模型解析一次、两处用：装配（决定装载哪个文件）与「打开模型目录」入口的路径。
        let model = self.model_source.resolve(&self.dirs);
        let model_path = menu_model_path(model.clone(), &self.dirs);
        let (mut scheme, option_roles, mut notes) = assemble_scheme(
            &self.dirs,
            model,
            &scheme_config_with_runtime(settings, runtime),
        );
        // 选项：有存储则同步（参照 `M.options.sync`，同步写入由核心抑制观察）；
        // 无存储时直接用内建缺省。会话创建时逐个同步（见 `Engine::session_new`）。
        let options = options_store(self.options_dir.as_deref(), settings, &option_roles);
        // 学习库的打开与诊断口径见 `open_learning`。
        let learning = open_learning(self.options_dir.as_deref(), &mut scheme, &mut notes);
        let learning_error = learning.error.clone();
        let model_info = crate::ui::cstring_lossy(scheme.model_info());
        Assembled {
            scheme,
            option_roles,
            options,
            learning,
            learning_error,
            model_path,
            model_info,
            notes,
        }
    }
}

/// 「打开模型目录」入口用的路径（UTF-8 串，`None` = 给不出任何路径 ⇒ ABI 返回 NULL）。
///
/// 解析到的模型文件优先（已装载 / 装载失败都是它）；没有模型时用
/// [`crate::paths::intended_model_path`] 给出**该放的位置**——文件可以不存在，其父目录正是
/// 「模型该放的地方」，宿主的首项据此把用户带到正确目录。
fn menu_model_path(model: Option<PathBuf>, dirs: &[PathBuf]) -> Option<CString> {
    model
        .or_else(|| crate::paths::intended_model_path(dirs, ASSETS))
        .map(|path| crate::ui::cstring_lossy(&path.to_string_lossy()))
}

/// 装配方案（数据 + 模型）并解析选项角色：构造与「重新部署」共用同一条路径
/// （两处各拼一份时漏一项即成为「重新部署后配置没下发」这类哑失败）。
///
/// `notes` 是装配诊断基线（数据目录 + 装载说明 + 角色解析错误），进状态串。
fn assemble_scheme(
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
    // 模型装载诊断：菜单只显示短名（`hux_engine_model_info`），格式标签 / 失败原因在这里补全，
    // 随状态串落日志（构造期与每次重新部署各一行）。
    notes.push(format!("model: {}", scheme.model_detail()));
    // 选项键的唯一来源 = 方案的声明；**缺角色即报错**（状态串可见），缺的角色不参与
    // 选项接线（无键 → 宿主跳过该项），不静默落到别的键上。
    let (option_roles, roles_error) = resolve_option_roles(scheme.option_declarations());
    if let Some(error) = roles_error {
        notes.push(format!("options: {error}"));
    }
    (scheme, option_roles, notes)
}

/// hux 自身设置 + 运行时开关的生效值 → 完整配置袋。
///
/// 设置派生的角色见 [`scheme_config`]；运行时开关（单字重码 / 全字集 / 过滤非汉字）
/// 由调用方按[会话 → 存储 → 设置缺省]取好（构造期尚无会话与存储，取设置值）。
pub(crate) fn scheme_config_with_runtime(
    settings: &Settings,
    runtime: RuntimeOptions,
) -> SchemeConfig {
    scheme_config(settings)
        .with(
            hux_cfg::roles::ROLE_ALLOW_DUPLICATE_SINGLE,
            Value::Bool(runtime.duplicate),
        )
        .with(
            hux_cfg::roles::ROLE_FULL_CHARSET,
            Value::Bool(runtime.full_charset),
        )
        .with(
            hux_cfg::roles::ROLE_FILTER_NON_HAN,
            Value::Bool(runtime.filter_non_han),
        )
}

/// 角色序（= [`hux_cfg::roles::RUNTIME_OPTION_ROLES`]）的选项键 C 字符串。
///
/// 角色顺序与 `hux_abi.h` 的 `HUX_OPTION_*` 一致（ABI 边界用角色，不暴露方案键名）；
/// 缺失角色为 `None`（宿主跳过该项）。构造与「重新部署」共用这一份实现。
pub(crate) fn option_keys(roles: &OptionKeys) -> Vec<Option<CString>> {
    hux_cfg::roles::RUNTIME_OPTION_ROLES
        .iter()
        .map(|role| roles.key(role).map(crate::ui::cstring_lossy))
        .collect()
}

/// 选项存储的装配（构造与「重新部署」共用）：用户目录不可用时为 `None`（此时仅用内建缺省）。
fn options_store(
    options_dir: Option<&Path>,
    settings: &Settings,
    roles: &OptionKeys,
) -> Option<OptionsStore> {
    options_dir.map(|dir| OptionsStore::load_with_defaults(dir, settings.store_defaults(roles)))
}

/// 打开学习库并把它并入装配说明（构造与「重新部署」共用同一口径）。
///
/// 库在 `<user dir>/<方案 id 哈希>.userdb/`；用户目录不可用时为禁用占位。
/// 诊断统一拼成 `learning: <错误|库名>`，并把「存储是否可写」告诉方案。
/// 调用方须保证旧句柄已释放——同一路径二次打开会撞上 LevelDB 的独占锁
/// （rusty-leveldb 的 `LOCK`）；库名依赖方案 id，故按传入方案的 id 打开。
fn open_learning(
    options_dir: Option<&Path>,
    scheme: &mut TigerScheme,
    notes: &mut Vec<String>,
) -> LearningStore {
    let learning = match options_dir {
        Some(dir) => {
            LearningStore::open(dir, &learning_store::store_name(scheme.id()), wall_clock())
        }
        None => LearningStore::disabled("user data directory unavailable"),
    };
    if let Some(error) = &learning.error {
        notes.push(format!("learning: {error}"));
    } else {
        notes.push(format!("learning: {}", learning.name));
    }
    scheme.set_store_ready(learning.store_ready());
    learning
}
