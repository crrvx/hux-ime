// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// hux-ime（虎虚）fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libhux_platform_fcitx5）。
#include <fcitx-config/configuration.h>
#include <fcitx-config/enum.h>
#include <fcitx-config/option.h>
#include <fcitx-config/iniparser.h>
#include <fcitx-utils/i18n.h>
#include <fcitx/action.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/candidatelist.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputmethodentry.h>
#include <fcitx/instance.h>
#include <fcitx/menu.h>
#include <fcitx/statusarea.h>
#include <fcitx/surroundingtext.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx/userinterfacemanager.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/trackableobject.h>

#include <algorithm>
#include <functional>
#include <iterator>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "hux_abi.h"

namespace {

/// 本层专属日志类别 `hux`。
///
/// `fcitx::Log::setLogRule` 按**类别名**匹配规则，而 `FCITX_DEBUG()` 走的是名为 `default`
/// 的类别（`log.h`：`FCITX_LOG(LEVEL)` → `FCITX_LOGC(::fcitx::Log::defaultCategory, LEVEL)`；
/// `log.cpp` 里默认类别名就是 `"default"`）。因此这里显式定义一个名为 `hux` 的类别，
/// 让 `fcitx5 --verbose='hux=5'` 只打开本层 DEBUG。
FCITX_DEFINE_LOG_CATEGORY(huxLog, "hux");

/// 本层调试日志（默认级别 Info ⇒ 平时静默；`hux=5` 时输出）。
#define HUX_DEBUG() FCITX_LOGC(huxLog, Debug)

/// 注意：`CommonCandidateList::setCursorIndex` 是**页内索引**（越界抛异常），
/// 绝对索引必须用 `setGlobalCursorIndex` + `setPage`；页大小来自配置 `PageSize`。
/// 页大小范围与 core `host::MAX_PAGE_SIZE` 一致（数字直选 0=第 10 个）。
constexpr int kPageSizeMin = 1;
constexpr int kPageSizeMax = 10;

/// 本 addon 的配置文件（相对 fcitx5 的 `PkgConfig` 目录）：构造时 `fcitx::readAsIni`
/// 读入、状态菜单里的宿主开关用 `fcitx::safeSaveAsIni` 写回——两者必须同路径、同 API 家族，
/// 否则「界面上改了但重启就丢」或写进另一个文件。
constexpr const char *kConfigPath = "conf/hux.conf";

/// 数字直选键序（1–9、0=第 10 个）：候选面板序号显示用。
const fcitx::KeyList &digitSelectionKeys() {
    static const fcitx::KeyList keys = {
        fcitx::Key(FcitxKey_1), fcitx::Key(FcitxKey_2), fcitx::Key(FcitxKey_3),
        fcitx::Key(FcitxKey_4), fcitx::Key(FcitxKey_5), fcitx::Key(FcitxKey_6),
        fcitx::Key(FcitxKey_7), fcitx::Key(FcitxKey_8), fcitx::Key(FcitxKey_9),
        fcitx::Key(FcitxKey_0)};
    return keys;
}

/// fcitx5 `KeyList` → C ABI 键位列表（超出上限的项忽略）。
void fillKeyList(hux_key_list *dest, const fcitx::KeyList &keys) {
    dest->count = 0;
    for (const fcitx::Key &key : keys) {
        if (dest->count >= HUX_MAX_KEYS) {
            break;
        }
        dest->sym[dest->count] = static_cast<int32_t>(key.sym());
        dest->states[dest->count] =
            static_cast<int32_t>(key.states().toInteger());
        ++dest->count;
    }
}

/// 候选排列（参照 `style` 语义；默认跟随 fcitx5 全局「候选竖排」设置）。
enum class HuxCandidateLayout { FollowGlobal, Horizontal, Vertical };
FCITX_CONFIG_ENUM_NAME(HuxCandidateLayout, "跟随全局", "横排", "竖排");
FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxCandidateLayout, "跟随全局", "横排", "竖排");

/// 预编辑内容。
enum class HuxPreeditMode { CandidateCode, RawInput, Hidden };
FCITX_CONFIG_ENUM_NAME(HuxPreeditMode, "候选分码", "原始输入", "不显示");
FCITX_CONFIG_ENUM_I18N_ANNOTATION(HuxPreeditMode, "候选分码", "原始输入", "不显示");

/// 枚举注解 + 悬浮说明（`EnumI18n` 与 `Tooltip` 并存）。
template <typename EnumAnnotation>
struct EnumAnnotationWithTooltip : EnumAnnotation {
    explicit EnumAnnotationWithTooltip(std::string tooltip)
        : tooltip_(std::move(tooltip)) {}

    bool skipDescription() const { return false; }
    bool skipSave() const { return false; }
    void dumpDescription(fcitx::RawConfig &config) const {
        EnumAnnotation::dumpDescription(config);
        config.setValueByPath("Tooltip", tooltip_);
    }

private:
    std::string tooltip_;
};

/// 行为设置（配置页「行为」分区）。
FCITX_CONFIGURATION(
    HuxBehaviorConfig,
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> earlyCommit{{
        .parent = this,
        .path{"EarlyCommit"},
        .description{"提前上屏"},
        .defaultValue = true,
        .annotation{"组合中证据成熟即提交当前候选；关闭后仅在空格/回车确认时上屏。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation>
        earlyCommitToPreedit{{
            .parent = this,
            .path{"EarlyCommitToPreedit"},
            .description{"提前上屏至预编辑"},
            .defaultValue = false,
            .annotation{"提前上屏改为写入预编辑（缓冲，不直接提交），继续输入可修正。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation>
        allowDuplicateSingle{{
            .parent = this,
            .path{"AllowDuplicateSingle"},
            .description{"单字重码参与组句"},
            .defaultValue = true,
            .annotation{"允许同一单字的重码候选参与整句解码；关闭可减少同字重复。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> fullShape{{
        .parent = this,
        .path{"FullShape"},
        .description{"全角标点"},
        .defaultValue = false,
        .annotation{"标点输出全角形式（如 `,` → `，`）。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> asciiPunct{{
        .parent = this,
        .path{"AsciiPunct"},
        .description{"ASCII 标点直通"},
        .defaultValue = false,
        .annotation{"标点不做中文映射，直接输出 ASCII。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> learningOnTab{{
        .parent = this,
        .path{"TabLearning"},
        .description{"Tab 选字写入学习库"},
        .defaultValue = true,
        .annotation{"用 Tab 选字时记录学习事件，参与后续候选排序。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> digitSelect{{
        .parent = this,
        .path{"DigitSelect"},
        .description{"数字直选"},
        .defaultValue = true,
        .annotation{"开启后菜单可见时 `1`–`9` 直接上屏当前页候选、`0` = 第 10 个；"
                    "关闭时数字仍作编码选重后缀。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> pageCycle{{
        .parent = this,
        .path{"PageCycle"},
        .description{"翻页循环"},
        .defaultValue = false,
        .annotation{"候选翻页在末页再翻回首页、首页向上翻到末页。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> panelPreedit{{
        .parent = this,
        .path{"PanelPreedit"},
        .description{"候选窗口显示预编辑"},
        .defaultValue = true,
        .annotation{"仅宿主显示项，不经引擎：候选窗口是否显示预编辑文本；"
                    "状态菜单「虎虚」里可就地切换（即时生效并写入本配置文件）。"}}};
    // 值选项（`int` 等）列在布尔选项之后。
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        highFreqLimit{{
            .parent = this,
            .path{"HighFreqLimit"},
            .description{"高频字过滤上限"},
            .defaultValue = 1500,
            .constrain = fcitx::IntConstrain(0, 20000),
            .annotation{"仅使用最优码组句的高频字数量上限；0 = 不限制。保存后即时生效。"}}};
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        pageSize{{
            .parent = this,
            .path{"PageSize"},
            .description{"每页候选个数"},
            .defaultValue = 5,
            .constrain = fcitx::IntConstrain(kPageSizeMin, kPageSizeMax),
            .annotation{"候选列表每页个数（1–10）；数字直选 `0` 对应第 10 个。"}}};
    fcitx::OptionWithAnnotation<
        HuxCandidateLayout,
        EnumAnnotationWithTooltip<HuxCandidateLayoutI18NAnnotation>>
        candidateLayout{{
            .parent = this,
            .path{"CandidateLayout"},
            .description{"候选排列"},
            .defaultValue = HuxCandidateLayout::FollowGlobal,
            .annotation{"跟随全局：候选窗排列随 fcitx5 全局「候选竖排」；"
                        "横排：↑↓ 选字、←→ 移动；竖排：←→ 选字、↑↓ 移动。"}}};
    fcitx::OptionWithAnnotation<HuxPreeditMode,
                                EnumAnnotationWithTooltip<HuxPreeditModeI18NAnnotation>>
        preeditMode{{
            .parent = this,
            .path{"PreeditMode"},
            .description{"预编辑内容"},
            .defaultValue = HuxPreeditMode::CandidateCode,
            .annotation{"候选分码：按词分段显示（如 `ab cd`）；原始输入：按输入原文；"
                        "不显示：仅候选与注释。"}}};
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        minRetainedInputLength{{
            .parent = this,
            .path{"MinRetainedRawLength"},
            .description{"提前上屏最短保留码数"},
            .defaultValue = 0,
            .constrain = fcitx::IntConstrain(0, 20),
            .annotation{"提前上屏与空码上屏共用的最短保留编码数；0 = 不额外限制"
                        "（概率型早提交仍不少于 3）。"}}};);

/// 与状态菜单（引擎运行时选项）共享的 schema 开关：角色 + 所在分区的字段。
///
/// 成员指针让字段名由编译器检查；`path()`（分区路径 + 字段路径）供宿主判断「配置文件里显式
/// 写过这一项吗」，故字段改名不会让该判断失配。共享开关分布在两个分区（「行为」「字集」），
/// 分区类型不同、字段类型相同，故按分区各实例化一张表。
template <typename Partition>
struct SharedRuntimeOption {
    using Option = fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation>;
    using Member = Option Partition::*;
    int32_t role;
    Member field;
};

/// 快捷键设置（配置页「快捷键」分区；`KeyList` 可多项，与全局设置同款）。
///
/// 两个反查默认键按**无修饰的 keysym** 声明（`` ` `` = `grave`、`~` = `asciitilde`）：
/// `~` 在物理键盘上是 Shift+`` ` ``，但前端上报的是该 level 的 keysym（`asciitilde` +
/// Shift），而 fcitx5 `Key::normalize()`（「usually used when key is from frontend」）
/// 对本身就产字符的非字母键会去掉 Shift ⇒ 引擎实际收到 `asciitilde` + 无修饰。
/// 旧默认 `Alt+:` / `Alt+"` 走的是同一条归一化（`:` = Shift+`;` → `colon` + Alt），
/// 故此处声明形态与之一致即可命中真实按键。
FCITX_CONFIGURATION(
    HuxHotkeyConfig,
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        reverseLookupPronunciationKeys{{
            .parent = this,
            .path{"SoundToCharShapeKey"},
            .description{"音反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_grave, fcitx::KeyState::NoState)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后输入拼音（支持拼写缩写），候选为对应词语、"
                        "注释显示虎码；触发键本身不再作为普通字符输入。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        reverseLookupCharacterKeys{{
            .parent = this,
            .path{"CharToSoundShapeKey"},
            .description{"字反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_asciitilde, fcitx::KeyState::NoState)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后显示光标左侧汉字的拼音与虎码"
                        "（需应用支持周边文本）；触发键本身不再作为普通字符输入。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation> pageUpKeys{{
        .parent = this,
        .path{"PageUpKey"},
        .description{"上翻页"},
        .defaultValue = fcitx::KeyList{
            fcitx::Key(FcitxKey_minus, fcitx::KeyState::NoState),
            fcitx::Key(FcitxKey_bracketleft, fcitx::KeyState::NoState)},
        .constrain =
            fcitx::KeyListConstrain(fcitx::KeyConstrainFlag::AllowModifierLess),
        .annotation{"可多项。有候选时生效（首屏不动）；Page_Up 键始终可用。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation> pageDownKeys{{
        .parent = this,
        .path{"PageDownKey"},
        .description{"下翻页"},
        .defaultValue = fcitx::KeyList{
            fcitx::Key(FcitxKey_equal, fcitx::KeyState::NoState),
            fcitx::Key(FcitxKey_bracketright, fcitx::KeyState::NoState)},
        .constrain =
            fcitx::KeyListConstrain(fcitx::KeyConstrainFlag::AllowModifierLess),
        .annotation{"可多项。有候选时生效；Page_Down 键始终可用。"}}};);

/// 字集设置（配置页「字集」分区）：决定装载哪几张码表。
///
/// 两项都只改**数据装载范围**，不动解码/排序语义；引擎在设置变更后重装码表，故保存即生效。
FCITX_CONFIGURATION(
    HuxCharsetConfig,
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> fullCharset{{
        .parent = this,
        .path{"FullCharset"},
        .description{"启用全字集"},
        .defaultValue = true,
        .annotation{"关闭只装主表码表（9,794 字），开启并装载追加码表让生僻字可打；"
                    "代价是启动多约 0.25 s、常驻多约 88 MB。保存后即时生效。"}}};
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> filterNonHan{{
        .parent = this,
        .path{"FilterNonHan"},
        .description{"过滤非汉字"},
        .defaultValue = true,
        .annotation{"过滤追加码表里的部首/笔画/注音/假名等非汉字符号（现数据 931 条）；"
                    "主表自带的标点/假名不受影响。保存后即时生效。"}}};);

/// 配置 schema：fcitx5-configtool 依据它自动生成设置页（fcitx://config/addon/hux）；
/// 分区结构参照全局设置（`Option<SubConfig>` → 分组标题，选项带悬浮说明）。
FCITX_CONFIGURATION(
    HuxConfig,
    fcitx::Option<HuxBehaviorConfig> behavior{this, "Behavior", "行为"};
    fcitx::Option<HuxCharsetConfig> charset{this, "Charset", "字集"};
    fcitx::Option<HuxHotkeyConfig> hotkeys{this, "Hotkey", "快捷键"};);

/// 「虎虚」状态菜单开关：勾选态取自引擎运行时选项（`options.yaml`），激活即翻转并落盘。
///
/// 这些开关与配置页的「行为」分区是**同一批项**（`HUX_OPTION_*` 角色 ↔ schema 字段）。
/// 翻转后经 `onChanged` 让宿主把新值写回自己的 schema 并落盘：配置页读的就是该 schema，
/// 于是「配置页改了」与「状态菜单改了」互相立刻可见（反向由 Rust 侧
/// `Engine::apply_settings` 把设置值写回 `options.yaml`，见该函数契约）。
class HuxToggleAction : public fcitx::Action {
public:
    HuxToggleAction(hux_engine *engine, const char *option, const char *label,
                    std::function<void(bool)> onChanged)
        : engine_(engine),
          option_(option),
          label_(label),
          onChanged_(std::move(onChanged)) {
        setCheckable(true);
    }

    std::string shortText(fcitx::InputContext * /*unused*/) const override {
        return label_;
    }

    std::string icon(fcitx::InputContext * /*unused*/) const override {
        return {};
    }

    bool isChecked(fcitx::InputContext * /*unused*/) const override {
        return hux_engine_option_value(engine_, option_.c_str()) == 1;
    }

    void activate(fcitx::InputContext * /*unused*/) override {
        const int32_t value = hux_engine_option_value(engine_, option_.c_str());
        if (value < 0) {
            return;
        }
        const bool next = value == 0;
        hux_engine_set_option(engine_, option_.c_str(), next ? 1 : 0);
        // 引擎侧可能保存失败（options.yaml 只读/磁盘满），但会话值已经翻转；schema 仍按
        // 引擎**实际**生效值镜像，避免两侧显示不一致。
        if (onChanged_) {
            onChanged_(hux_engine_option_value(engine_, option_.c_str()) == 1);
        }
    }

private:
    hux_engine *engine_;
    std::string option_;
    std::string label_;
    std::function<void(bool)> onChanged_;
};

class HuxEngine;

/// 宿主项开关：勾选态与点击都归宿主，不走引擎的运行时选项（那些走 [`HuxToggleAction`]，
/// 键由方案声明、可被 `options.yaml` 覆盖）。本类的项就是配置项本身，故状态现取现用，
/// 配置被外部改动后菜单也不需要另行同步。
class HuxHostToggleAction : public fcitx::Action {
public:
    HuxHostToggleAction(std::string label, std::function<bool()> checked,
                        std::function<void(fcitx::InputContext *)> toggled)
        : label_(std::move(label)),
          checked_(std::move(checked)),
          toggled_(std::move(toggled)) {
        setCheckable(true);
    }

    std::string shortText(fcitx::InputContext * /*unused*/) const override {
        return label_;
    }

    std::string icon(fcitx::InputContext * /*unused*/) const override {
        return {};
    }

    bool isChecked(fcitx::InputContext * /*unused*/) const override {
        return checked_();
    }

    void activate(fcitx::InputContext *inputContext) override {
        toggled_(inputContext);
    }

private:
    std::string label_;
    std::function<bool()> checked_;
    std::function<void(fcitx::InputContext *)> toggled_;
};

/// 宿主项动作（「重新部署」与「模型」信息行）：文案每次取用时现算——「模型」行随重新
/// 部署变化，故宿主不另存一份；`activate` 可缺省（信息项不可点，点击为无操作）。
class HuxHostAction : public fcitx::Action {
public:
    HuxHostAction(std::function<std::string()> label,
                  std::function<void(fcitx::InputContext *)> activate = {})
        : label_(std::move(label)), activate_(std::move(activate)) {}

    std::string shortText(fcitx::InputContext * /*unused*/) const override {
        return label_();
    }

    std::string icon(fcitx::InputContext * /*unused*/) const override {
        return {};
    }

    void activate(fcitx::InputContext *inputContext) override {
        if (activate_) {
            activate_(inputContext);
        }
    }

private:
    std::function<std::string()> label_;
    std::function<void(fcitx::InputContext *)> activate_;
};

/// 最近一次 UI 快照（面板预编辑 / 候选 / 高亮 / 两排辅助文本）。
///
/// 宿主开关（「候选窗口显示预编辑」）切换后要**立即**重放当前界面，而关闭期间面板里
/// 已经没有预编辑原文了；所以 `applyUpdate` 顺手把快照记在**该输入上下文的会话**上
/// （会话随输入上下文销毁 ⇒ 不留任何跨调用存活的输入上下文指针，与 `~HuxEngine`
/// 的 UAF 契约同向）。
struct HuxUiSnapshot {
    bool valid = false;
    std::string preedit;
    int32_t cursor = -1;
    std::vector<std::string> texts;
    std::vector<std::string> comments;
    int32_t selected = 0;
    std::string auxUp;
    std::string auxDown;
};

/// 每输入上下文会话（fcitx5 `InputContextProperty`）：持引擎侧会话 id，销毁时释放。
/// 组合/候选/学习暂存按会话隔离，互不干扰。
class HuxSession : public fcitx::InputContextProperty {
public:
    HuxSession(hux_engine *engine, uint64_t id) : engine_(engine), id_(id) {}

    /// 析构时引擎**必须仍存活**：本类由 `HuxEngine::~HuxEngine` 的
    /// `sessionFactory_.unregister()` 统一销毁（见该析构函数的契约注释），
    /// 那时 `hux_engine_free` 尚未执行。
    ~HuxSession() override {
        HUX_DEBUG() << "hux: ~HuxSession id=" << id_;
        hux_engine_session_free(engine_, id_);
    }

    uint64_t id() const { return id_; }

    /// 最近一次 UI 快照（宿主开关切换后重放用）；随本会话一起销毁。
    HuxUiSnapshot &ui() { return ui_; }

private:
    hux_engine *engine_;
    uint64_t id_;
    HuxUiSnapshot ui_;
};

/// 面板候选：点击（`select`）按全局索引选中并上屏（与空格相同的确认/学习链）。
///
/// **生命周期**：候选列表归 `InputContext` 的输入面板所有，而 IC **晚于** addon 实例
/// （含本引擎）析构（`InstancePrivate` 先声明 `icManager_`、后声明 `addonManager_`，
/// 故 `~HuxEngine` 运行时 IC 全部存活）⇒ 引擎释放后，面板里可能仍留着本对象，
/// 用户点一下就走到已释放的引擎上。故这里只持 `TrackableObjectReference`（弱引用，
/// fcitx5 惯用法，见 `cloudpinyin_public.h`），失效即早退。
class HuxCandidateWord : public fcitx::CandidateWord {
public:
    HuxCandidateWord(fcitx::Text text, fcitx::Text comment,
                     fcitx::TrackableObjectReference<HuxEngine> owner,
                     int32_t index)
        : CandidateWord(std::move(text)), owner_(owner), index_(index) {
        setComment(std::move(comment));
    }

    void select(fcitx::InputContext *inputContext) const override;

private:
    fcitx::TrackableObjectReference<HuxEngine> owner_;
    int32_t index_;
};

/// 引擎 addon。
///
/// 继承 `fcitx::TrackableObject<HuxEngine>`：让「归 IC / 面板所有、可能比引擎活得久」的
/// UI 对象（`HuxCandidateWord`）持弱引用，避免悬垂（见 `HuxCandidateWord` 注释）。
class HuxEngine : public fcitx::InputMethodEngine,
                  public fcitx::TrackableObject<HuxEngine> {
public:
    explicit HuxEngine(fcitx::Instance *instance)
        : instance_(instance),
          sessionFactory_([this](fcitx::InputContext & /*unused*/) {
              return new HuxSession(engine_, hux_engine_session_new(engine_));
          }) {
        fcitx::readAsIni(config_, kConfigPath);
        hux_host host = {};
        host.user = this;
        host.commit = &HuxEngine::commitCallback;
        host.update = &HuxEngine::updateCallback;
        engine_ = hux_engine_new(&host);
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        // 码表装载摘要：装了几张码表、两个字集开关的生效值（引擎给出的一行文本，本层不解析）。
        if (const char *info = hux_engine_data_info(engine_)) {
            FCITX_INFO() << "hux: data " << info;
        }
        // 每输入上下文一个会话（现存的与后续新建的都会经工厂创建）。
        // 注册成功是 `~HuxEngine` 里 `unregister()` 能**销毁全部会话**的前提
        // （名字冲突时 fcitx5 直接返回 false 且不创建任何会话）；失败必须显式可见，
        // 否则「会话从不释放」会静默（见 `~HuxEngine` 契约注释）。
        if (!instance_->inputContextManager().registerProperty("huxSession",
                                                               &sessionFactory_)) {
            FCITX_WARN() << "hux: 会话属性注册失败（huxSession 名字冲突？）";
        }
        adoptStoredRuntimeOptions();
        applyConfig();
        setupStatusMenu();
    }
    /// 析构契约：**必须先 `sessionFactory_.unregister()`，再 `hux_engine_free(engine_)`**。
    ///
    /// 这不是风格问题，而是「每个 `HuxSession` 析构时都要拿**仍存活**的引擎调
    /// `hux_engine_session_free`」这一前提。依据（按 fcitx5 源码逐层核对；所用版本
    /// 5.1.22 与 master 在这几个文件上**逐字节相同**，master 最后改动这些文件的提交
    /// 早于 5.1.22 tag）：
    ///   1. `src/lib/fcitx/inputcontextproperty.cpp`：`InputContextPropertyFactory::unregister()`
    ///      → `d->manager_->unregisterProperty(d->name_)`（析构函数亦调 `unregister()`）；
    ///   2. `src/lib/fcitx/inputcontextmanager.cpp`：`InputContextManagerPrivate::unregisterProperty(name)`
    ///      **遍历 `inputContexts_`** 逐个 `inputContext.d_func()->unregisterProperty(slot)`；
    ///   3. `src/lib/fcitx/inputcontext_p.h`：`InputContextPrivate::unregisterProperty(slot)` 是
    ///      `properties_[slot] = std::move(properties_.back()); properties_.pop_back();`，
    ///      而 `properties_` 是 `std::vector<std::unique_ptr<InputContextProperty>>`
    ///      ⇒ 被覆盖的 `unique_ptr` 在赋值当场析构 ⇒ **每个 `HuxSession` 当场析构**
    ///      （各自跑 `hux_engine_session_free(engine_, id_)`，此刻引擎仍存活）；
    ///   4. `src/lib/fcitx/inputcontextproperty.h` 注释：工厂「must unregister before the
    ///      destruction of `InputContextManager`」——addon 实例（含本引擎）先死、IC 后死
    ///      （`InstancePrivate` 先声明 `icManager_` 再声明 `addonManager_`，反向析构
    ///      ⇒ `~AddonManager`（`unload()` → 删 addon 实例）在 IC 之前）。
    /// 若调换（先 `free` 再 `unregister`）：每个 `HuxSession` 会对已释放的引擎调
    /// `hux_engine_session_free` ⇒ UAF。**因此本顺序是安全性的前提，改动前请重读本节。**
    /// 真机核对方法（析构顺序）：`fcitx5 -r --verbose='hux=5'` 前台运行，退出后确认全部
    /// `~HuxSession` 日志**早于** `~HuxEngine`（顺序相反即命中悬垂路径）。
    ~HuxEngine() override {
        HUX_DEBUG() << "hux: ~HuxEngine";
        // 1) 注销工厂 ⇒ fcitx5 立刻销毁全部已注册会话（见上方契约注释）。
        sessionFactory_.unregister();
        // 2) 摘掉 IC 状态区里指向本对象成员的裸指针（此时本对象成员仍存活）。
        clearStatusAreas();
        // 3) 引擎最后释放；指针置空，任何迟到的宿主回调都只是无操作。
        hux_engine_free(engine_);
        engine_ = nullptr;
    }

    /// 配置 schema（fcitx5-configtool 生成设置页；保存到 ~/.config/fcitx5/conf/hux.conf）。
    const fcitx::Configuration *getConfig() const override { return &config_; }

    /// 用户在配置工具中保存后：落盘由框架负责，这里应用到引擎（即时生效项）。
    void setConfig(const fcitx::RawConfig &raw) override {
        config_.load(raw, true);
        applyConfig();
    }

    void keyEvent(const fcitx::InputMethodEntry &entry,
                  fcitx::KeyEvent &keyEvent) override {
        FCITX_UNUSED(entry);
        const auto &key = keyEvent.key();
        fcitx::InputContext *inputContext = keyEvent.inputContext();
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        // 应用侧周边文本（字反查用；应用不支持时 valid=0）。
        const auto &surrounding = inputContext->surroundingText();
        if (surrounding.isValid()) {
            hux_engine_set_surrounding(engine_, huxSession->id(),
                                       surrounding.text().c_str(),
                                       static_cast<int32_t>(surrounding.cursor()), 1);
        } else {
            hux_engine_set_surrounding(engine_, huxSession->id(), nullptr, 0, 0);
        }
        const int32_t disposition =
            hux_engine_key(engine_, huxSession->id(), key.sym(),
                           key.states().toInteger(),
                           keyEvent.isRelease() ? 1 : 0);
        context_ = nullptr;
        if (disposition & HUX_KEY_FORWARD_AFTER_COMMIT) {
            // 布局转换键（如系统 colemak + 方案自定义 us 布局）：交回核心处理——
            // 核心在 ReservedLast 阶段会提交**转换后**的字符并消费该键；若本层
            // 自行 forwardKey，客户端会按系统布局重新解释该键，从而得到未经
            // fcitx5 映射的字符。
            if (!(keyEvent.forward() &&
                  keyEvent.rawKey().sym() != keyEvent.origKey().sym())) {
                // 已提交且未消费：先让提交送达，再由本层重发按键（与核心
                // `KeyEventOrderFix` 修法一致），避免前端在 keyEvent 返回后立刻
                // 转发按键导致「字母先于候选上屏」。
                keyEvent.filterAndAccept();
                inputContext->forwardKey(keyEvent.rawKey(), keyEvent.isRelease(),
                                         keyEvent.time());
            }
        } else if (disposition & HUX_KEY_CONSUMED) {
            keyEvent.filterAndAccept();
        }
    }

    void activate(const fcitx::InputMethodEntry &entry,
                  fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        // 会话在输入上下文注册时已建；激活只刷新状态区。
        updateStatusArea(event.inputContext());
    }

    void deactivate(const fcitx::InputMethodEntry &entry,
                    fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        // 失焦：由核心/前端把客户端预编辑以原文提交（fcitx5 惯例，不保留组合）；
        // 切换输入法/重置：本层直接丢弃（不提交；上游默认在切换时提交，本实现取丢弃）。
        resetSession(event);
    }

    void reset(const fcitx::InputMethodEntry &entry,
               fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        resetSession(event);
    }

    /// 面板候选点击：以该输入上下文交给引擎（提交/预编辑/候选经回调送出）。
    void selectCandidate(fcitx::InputContext *inputContext, int32_t index) {
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        hux_engine_select_candidate(engine_, huxSession->id(), index);
        context_ = nullptr;
    }

private:
    /// 该输入上下文对应会话（构造时注册的工厂保证已存在）。
    HuxSession *session(fcitx::InputContext *inputContext) const {
        if (inputContext == nullptr) {
            return nullptr;
        }
        return static_cast<HuxSession *>(inputContext->property(&sessionFactory_));
    }

    /// 状态菜单：注册「虎虚」子菜单与核心开关（构造时一次）。
    ///
    /// **选项键经 ABI 取自引擎**（`hux_engine_option_key`，与 `HUX_OPTION_*` 角色一一对应），
    /// 宿主只保留 UI 文案——方案改名或换方案时菜单自动跟随，不会静默失效。
    /// 文案表按 **ABI 角色下标**取（`kLabels[role]`），故长度必须等于 `HUX_OPTION_COUNT`：
    /// 角色数增加而文案漏补时，这里是越界读（UB）；`static_assert` 把它变成编译失败
    /// （角色**调序**由 Rust 侧 `option_role_order_matches_the_abi_header` 抓）。
    void setupStatusMenu() {
        static constexpr const char *kLabels[] = {
            "提前上屏", "提前上屏至预编辑", "单字重码组句", "全角标点", "数字直选",
            "启用全字集", "过滤非汉字",
        };
        static_assert(std::size(kLabels) == HUX_OPTION_COUNT,
                      "状态菜单文案表长度必须等于 HUX_OPTION_COUNT（ABI 角色数）");
        menuAction_.setShortText("虎虚");
        // 状态区图标用本包自带的主题名（与 `conf/hux.inputmethod.conf` 的 `Icon` 一致）；
        // 不设时 fcitx5 回退到输入法条目图标——那个在缺 fcitx5-chinese-addons 的机器上是缺图占位。
        menuAction_.setIcon("hux");
        const int32_t roles = hux_engine_option_role_count();
        for (int32_t role = 0; role < roles; ++role) {
            const char *option = hux_engine_option_key(engine_, role);
            if (option == nullptr) {
                continue;
            }
            auto action = std::make_unique<HuxToggleAction>(
                engine_, option, kLabels[role],
                [this, role](bool value) { mirrorRuntimeRole(role, value); });
            instance_->userInterfaceManager().registerAction(
                std::string("hux-") + option, action.get());
            menu_.addAction(action.get());
            toggleActions_.push_back(std::move(action));
        }
        // 宿主项（不属 `HUX_OPTION_*` 角色，不进上面的文案表）：候选窗口显示预编辑。
        panelPreeditAction_ = std::make_unique<HuxHostToggleAction>(
            "候选窗口显示预编辑",
            [this] { return config_.behavior->panelPreedit.value(); },
            [this](fcitx::InputContext *inputContext) {
                togglePanelPreedit(inputContext);
            });
        instance_->userInterfaceManager().registerAction("hux-panel-preedit",
                                                         panelPreeditAction_.get());
        menu_.addAction(panelPreeditAction_.get());
        // 「重新部署」与「模型」信息行：文案现算（模型摘要来自引擎，见 `modelText`）。
        redeployAction_ = std::make_unique<HuxHostAction>(
            [] { return std::string("重新部署"); },
            [this](fcitx::InputContext *inputContext) { redeploy(inputContext); });
        instance_->userInterfaceManager().registerAction("hux-redeploy",
                                                         redeployAction_.get());
        menu_.addAction(redeployAction_.get());
        modelAction_ = std::make_unique<HuxHostAction>([this] { return modelText(); });
        instance_->userInterfaceManager().registerAction("hux-model",
                                                         modelAction_.get());
        menu_.addAction(modelAction_.get());
        menuAction_.setMenu(&menu_);
        instance_->userInterfaceManager().registerAction("hux-menu",
                                                         &menuAction_);
    }

    /// 把「虎虚」子菜单挂到当前输入上下文的状态区（仅在本输入法激活时显示）。
    void updateStatusArea(fcitx::InputContext *inputContext) {
        if (inputContext == nullptr) {
            return;
        }
        auto &statusArea = inputContext->statusArea();
        statusArea.clearGroup(fcitx::StatusGroup::InputMethod);
        statusArea.addAction(fcitx::StatusGroup::InputMethod, &menuAction_);
    }

    /// 摘掉**所有**输入上下文状态区里指向本对象成员的裸指针（`&menuAction_` 及其子菜单）。
    ///
    /// 归 IC 所有的对象比 addon 实例活得久（见 `~HuxEngine` 契约注释），故引擎释放前
    /// 必须把「虎虚」子菜单从每个 IC 摘除。这里用 `InputContextManager::foreach`
    /// （`inputcontextmanager.cpp`：遍历 `inputContexts_`，visitor 返回 false 即中止）
    /// + `StatusArea::clearGroup`（`statusarea.cpp`：逐个 `removeAction`）。
    ///
    /// 与 fcitx5 自带清理的关系：`StatusArea::addAction` 也连了 `Action::ObjectDestroyed`
    /// （动作析构时自摘 + `d->update()`），`~Element` 还会把自身从父节点摘除——本调用
    /// **不替代**它们，而是在自身成员仍有效时先把 UI 刷新一次，使状态区不再引用本对象
    /// 的任何成员；不把安全性寄托在「析构中途才触发的自清」上。
    void clearStatusAreas() {
        if (instance_ == nullptr) {
            return;
        }
        instance_->inputContextManager().foreach([](fcitx::InputContext *ic) {
            ic->statusArea().clearGroup(fcitx::StatusGroup::InputMethod);
            return true;
        });
    }

    /// 清空会话与面板（deactivate/reset 共用）。
    void resetSession(fcitx::InputContextEvent &event) {
        fcitx::InputContext *inputContext = event.inputContext();
        HuxSession *huxSession = session(inputContext);
        if (huxSession == nullptr) {
            return;
        }
        context_ = inputContext;
        hux_engine_reset(engine_, huxSession->id());
        context_ = nullptr;
    }

    /// 引擎 → 宿主的两个回调（`hux_host.user` 就是 `this`）。
    ///
    /// `user` 指针安全性：Rust 侧只在 `hux_engine_*` 调用**内部**同步回调，而引擎的
    /// 整个生命周期都包在 `~HuxEngine` 内（`hux_engine_free` 是析构体的一步），
    /// 故回调期间 `this` 必存活。`~HuxEngine` 里把 `engine_` 置空，使「引擎已释放后
    /// 仍被调用」这种本不该发生的路径退化成无操作而非解引用悬垂指针。
    static void commitCallback(void *user, const char *text) {
        static_cast<HuxEngine *>(user)->applyCommit(text);
    }

    static void updateCallback(void *user, const char *preedit, int32_t cursor,
                               const char *const *texts,
                               const char *const *comments, int32_t count,
                               int32_t selected, const char *auxUp,
                               const char *auxDown) {
        static_cast<HuxEngine *>(user)->applyUpdate(
            preedit, cursor, texts, comments, count, selected, auxUp, auxDown);
    }

    void applyCommit(const char *text) {
        if (engine_ == nullptr) {
            return;
        }
        if (context_ != nullptr && text != nullptr && *text != '\0') {
            context_->commitString(text);
        }
    }

    /// 应用 UI 快照：preedit（面板 + 客户端内联）+ 候选列表与高亮。
    void applyUpdate(const char *preedit, int32_t cursor,
                     const char *const *texts, const char *const *comments,
                     int32_t count, int32_t selected, const char *auxUp,
                     const char *auxDown) {
        // 引擎已释放（理论上不会发生，见 `commitCallback` 注释）：本函数后面还要经
        // `hux_engine_option_key(engine_, …)` 触碰引擎，故先早退。
        if (engine_ == nullptr || context_ == nullptr) {
            return;
        }
        HuxUiSnapshot snapshot;
        snapshot.valid = true;
        snapshot.preedit = preedit != nullptr ? preedit : "";
        snapshot.cursor = cursor;
        snapshot.selected = selected;
        snapshot.auxUp = auxUp != nullptr ? auxUp : "";
        snapshot.auxDown = auxDown != nullptr ? auxDown : "";
        for (int32_t index = 0; index < count; ++index) {
            snapshot.texts.emplace_back(
                texts != nullptr && texts[index] != nullptr ? texts[index] : "");
            snapshot.comments.emplace_back(
                comments != nullptr && comments[index] != nullptr
                    ? comments[index]
                    : "");
        }
        // 快照记在会话上：宿主开关切换后按它重放，不必等下一次按键。
        if (HuxSession *huxSession = session(context_)) {
            huxSession->ui() = snapshot;
        }
        render(context_, snapshot);
    }

    /// 按快照刷新一个输入上下文的输入面板（`applyUpdate` 与宿主开关共用）。
    void render(fcitx::InputContext *inputContext,
                const HuxUiSnapshot &snapshot) {
        // 同 `applyUpdate`：下面要经 `hux_engine_option_key(engine_, …)` 触碰引擎。
        if (engine_ == nullptr || inputContext == nullptr) {
            return;
        }
        const std::string &preeditString = snapshot.preedit;
        fcitx::Text preeditText(preeditString);
        if (snapshot.cursor >= 0 &&
            static_cast<size_t>(snapshot.cursor) <= preeditString.size()) {
            preeditText.setCursor(snapshot.cursor);
        }
        // 候选窗口预编辑：可配置关闭（关闭后仅候选与注释）。
        inputContext->inputPanel().setPreedit(
            config_.behavior->panelPreedit.value() ? preeditText
                                                   : fcitx::Text());
        // 客户端内联预编辑：跟随 fcitx5 全局预编辑设置（`isPreeditEnabled`）。
        // 失焦时由核心/前端以预编辑原文提交（fcitx5 惯例），因此不做 `DontCommit` 标记——
        // 标记后 KWin（input-method v1）在提交串为空时不发 commit，会在应用内留下可被保存的
        // marked text「残影」。
        inputContext->inputPanel().setClientPreedit(
            inputContext->isPreeditEnabled() ? preeditText : fcitx::Text());
        inputContext->updatePreedit();

        // 候选：无候选时置 `nullptr` 清除（fcitx5 约定）。**不可**留下「存在但为空」的
        // 列表——其他组件会对它调用 `candidate(0)`（如 fcitx5-table 的
        // `TableState::keyEvent`），抛 `CommonCandidateList: invalid index` 并 abort。
        if (snapshot.texts.empty()) {
            inputContext->inputPanel().setCandidateList(nullptr);
        } else {
            auto candidateList = std::make_unique<fcitx::CommonCandidateList>();
            for (size_t index = 0; index < snapshot.texts.size(); ++index) {
                candidateList->append<HuxCandidateWord>(
                    fcitx::Text(snapshot.texts[index]),
                    fcitx::Text(snapshot.comments[index]), watch(),
                    static_cast<int32_t>(index));
            }
            // 数字直选：面板显示 1–9 / 0 序号（与引擎页内定位一致）。
            // 取**运行时生效值**（状态菜单可切换、options.yaml 优先），而非仅读配置页设置。
            const char *digitSelectKey =
                hux_engine_option_key(engine_, HUX_OPTION_DIGIT_SELECT);
            const bool digitSelect =
                digitSelectKey != nullptr &&
                hux_engine_option_value(engine_, digitSelectKey) == 1;
            if (digitSelect) {
                candidateList->setSelectionKey(digitSelectionKeys());
            }
            // 翻页交由 fcitx5 面板（页大小与引擎一致）：绝对索引 → 全局光标 + 所在页。
            candidateList->setPageSize(std::clamp(
                config_.behavior->pageSize.value(), kPageSizeMin, kPageSizeMax));
            // 候选排列：仅显式选择横排/竖排时设置 LayoutHint（跟随全局时保持 NotSet，
            // 由 fcitx5 全局「候选竖排」设置决定）。
            const auto layout = config_.behavior->candidateLayout.value();
            if (layout != HuxCandidateLayout::FollowGlobal) {
                candidateList->setLayoutHint(
                    layout == HuxCandidateLayout::Vertical
                        ? fcitx::CandidateLayoutHint::Vertical
                        : fcitx::CandidateLayoutHint::Horizontal);
            }
            // 防御：越界不设光标索引。
            const int index = std::min(
                std::max(snapshot.selected, 0),
                static_cast<int32_t>(snapshot.texts.size()) - 1);
            candidateList->setGlobalCursorIndex(index);
            const int page = index / candidateList->pageSize();
            if (page < candidateList->totalPages()) {
                candidateList->setPage(page);
            }
            inputContext->inputPanel().setCandidateList(std::move(candidateList));
        }
        // 字反查（⑧-2）：两排辅助文本（上排 = 光标左、下排 = 光标右）；空串清除。
        const auto auxText = [](const std::string &value) {
            return !value.empty() ? fcitx::Text(value) : fcitx::Text();
        };
        inputContext->inputPanel().setAuxUp(auxText(snapshot.auxUp));
        inputContext->inputPanel().setAuxDown(auxText(snapshot.auxDown));
        inputContext->updateUserInterface(
            fcitx::UserInterfaceComponent::InputPanel);
        updateStatusArea(inputContext);
    }

    /// 宿主项「候选窗口显示预编辑」：改配置 → 落盘 → 按该输入上下文的最近一次快照
    /// **立即**重放（面板马上显示/隐藏预编辑，不必等下一次按键）。
    ///
    /// 没有快照（该上下文还没出过 UI）或从托盘调用而拿不到输入上下文时只落盘，
    /// 下一次 `applyUpdate` 自然按新值渲染。
    void togglePanelPreedit(fcitx::InputContext *inputContext) {
        // `Option::operator->` 是 const 限定（只读视图），写入口是 `mutableValue()`。
        HuxBehaviorConfig *behavior = config_.behavior.mutableValue();
        const bool value = !behavior->panelPreedit.value();
        behavior->panelPreedit.setValue(value);
        // 与构造时的 `readAsIni(config_, kConfigPath)` 对称：同路径、同 API 家族。
        if (!fcitx::safeSaveAsIni(config_, kConfigPath)) {
            FCITX_WARN() << "hux: 写入 " << kConfigPath
                         << " 失败（候选窗口显示预编辑）";
        }
        if (inputContext == nullptr) {
            return;
        }
        HuxSession *huxSession = session(inputContext);
        if (huxSession != nullptr && huxSession->ui().valid) {
            render(inputContext, huxSession->ui());
        }
        if (panelPreeditAction_ != nullptr) {
            panelPreeditAction_->update(inputContext);
        }
    }

    /// 「模型」行的文案 = `模型：` + 引擎给出的一行摘要（方案侧结构化产出，本层不解析）。
    std::string modelText() const {
        const char *info = hux_engine_model_info(engine_);
        return std::string("模型：") + (info != nullptr ? info : "不可用");
    }

    /// 重新部署：重读配置 → 引擎重走构造期读取 → 对齐共享开关 → 清面板 → 刷新状态菜单与日志。
    ///
    /// 会话 id 不变（宿主侧的输入上下文不需要重建），但组合与候选全部作废，故必须清面板
    /// ——否则面板上留着已失效的旧候选。
    ///
    /// 顺序说明：引擎的重新部署自己会重读 `options.yaml` 与学习库（见 `hux_engine_redeploy`
    /// 契约），故本层先让它重读、再 `adoptStoredRuntimeOptions()` 对齐没写进本配置文件的共享
    /// 开关，最后 `applyConfig()` 推设置——这样「手改 `options.yaml` 后重新部署」与「重启
    /// fcitx5」得到同一结果（配置文件显式写过的键仍以文件为准）。
    void redeploy(fcitx::InputContext *inputContext) {
        // 1) 重新读取本 addon 的配置文件（与构造同一入口）。
        fcitx::readAsIni(config_, kConfigPath);
        // 2) 引擎侧：重走构造期读取（目录 / 选项存储 / 学习库 / 方案数据与模型）+ 重置全部会话。
        if (hux_engine_redeploy(engine_) == 0) {
            FCITX_WARN() << "hux: 重新部署失败（引擎不可用）";
            return;
        }
        // 3) 与构造同一规则：配置文件没写过的共享键沿用引擎（重读后）的值；随后推设置
        //    （含快捷键绑定），设置值仍是权威并写回存储。
        adoptStoredRuntimeOptions();
        applyConfig();
        // 4) 清空各输入上下文的面板与会话里的 UI 快照。
        clearPanels();
        // 5) 「模型」行与状态菜单刷新（文案现算，这里只通知 UI 重取）。
        refreshHostActions(inputContext);
        // 6) 新状态串落日志：排查「重新部署后还是老样子」时先看这里。
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        // 重新装载后的码表摘要（同构造期那行）：装了几张、两个字集开关的生效值。
        if (const char *info = hux_engine_data_info(engine_)) {
            FCITX_INFO() << "hux: data " << info;
        }
    }

    /// 清空全部输入上下文的面板（预编辑 / 候选 / 两排辅助文本）与会话里的 UI 快照。
    ///
    /// 重新部署后引擎侧会话状态已重置（见 `hux_engine_redeploy` 契约），留在面板上的旧候选
    /// 不再有效；宿主侧会话仍然存活，故快照也要一并清掉（否则开关切换会把旧快照重放出来）。
    void clearPanels() {
        if (instance_ == nullptr) {
            return;
        }
        instance_->inputContextManager().foreach(
            [this](fcitx::InputContext *inputContext) {
                if (HuxSession *huxSession = session(inputContext)) {
                    huxSession->ui() = HuxUiSnapshot();
                }
                inputContext->inputPanel().setPreedit(fcitx::Text());
                inputContext->inputPanel().setClientPreedit(fcitx::Text());
                inputContext->inputPanel().setCandidateList(nullptr);
                inputContext->inputPanel().setAuxUp(fcitx::Text());
                inputContext->inputPanel().setAuxDown(fcitx::Text());
                inputContext->updatePreedit();
                inputContext->updateUserInterface(
                    fcitx::UserInterfaceComponent::InputPanel);
                return true;
            });
    }

    /// 通知状态菜单里的宿主项重新取文案/勾选态（重新部署后「模型」行会变）。
    void refreshHostActions(fcitx::InputContext *inputContext) {
        if (inputContext == nullptr) {
            return;
        }
        if (panelPreeditAction_ != nullptr) {
            panelPreeditAction_->update(inputContext);
        }
        if (redeployAction_ != nullptr) {
            redeployAction_->update(inputContext);
        }
        if (modelAction_ != nullptr) {
            modelAction_->update(inputContext);
        }
    }

    /// 把 schema 值经 C ABI 推给 Rust 侧（`Settings::apply_settings`）。
    void applyConfig() {
        if (engine_ == nullptr) {
            return;
        }
        hux_options options = {};
        const auto &behavior = config_.behavior.value();
        const auto &hotkeys = config_.hotkeys.value();
        options.early_commit = behavior.earlyCommit.value() ? 1 : 0;
        options.early_commit_to_preedit =
            behavior.earlyCommitToPreedit.value() ? 1 : 0;
        options.allow_duplicate_single =
            behavior.allowDuplicateSingle.value() ? 1 : 0;
        options.full_shape = behavior.fullShape.value() ? 1 : 0;
        options.ascii_punct = behavior.asciiPunct.value() ? 1 : 0;
        options.learning_on_tab = behavior.learningOnTab.value() ? 1 : 0;
        options.high_freq_limit = behavior.highFreqLimit.value();
        fillKeyList(&options.reverse_lookup_pronunciation,
                    hotkeys.reverseLookupPronunciationKeys.value());
        fillKeyList(&options.reverse_lookup_character,
                    hotkeys.reverseLookupCharacterKeys.value());
        options.page_size = behavior.pageSize.value();
        fillKeyList(&options.page_up, hotkeys.pageUpKeys.value());
        fillKeyList(&options.page_down, hotkeys.pageDownKeys.value());
        options.digit_select = behavior.digitSelect.value() ? 1 : 0;
        switch (behavior.candidateLayout.value()) {
        case HuxCandidateLayout::Horizontal:
            options.candidate_layout = 1;
            break;
        case HuxCandidateLayout::Vertical:
            options.candidate_layout = 2;
            break;
        default:
            options.candidate_layout = 0;
            break;
        }
        const auto preeditMode = behavior.preeditMode.value();
        options.preedit_mode = preeditMode == HuxPreeditMode::RawInput   ? 1
                               : preeditMode == HuxPreeditMode::Hidden ? 2
                                                                       : 0;
        options.page_cycle = behavior.pageCycle.value() ? 1 : 0;
        options.min_retained_input_length =
            behavior.minRetainedInputLength.value();
        const auto &charset = config_.charset.value();
        options.full_charset = charset.fullCharset.value() ? 1 : 0;
        options.filter_non_han = charset.filterNonHan.value() ? 1 : 0;
        if (hux_engine_apply_settings(engine_, &options) == 0) {
            FCITX_WARN() << "hux: apply settings failed";
        }
        // 配置页可能绑到**没有名字的 keysym**（媒体键 / 厂商扩展键）：Rust 侧无法把它解析成
        // rime 键名，该绑定会被丢弃——诊断经状态串的 `hotkeys:` 前缀送出。
        // 这里把最新状态串落到日志，使「绑定静默消失」变得可诊断；按契约指针只用一次
        // （`hux_engine_status` 在下一次状态刷新后失效，见 `hux_abi.h`）。
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        // 数据装载摘要：设置推送后字集开关可能刚被改写（配置页改了「启用全字集」/「过滤非汉字」，
        // 或启动时按 `conf/hux.conf` 对齐），补一行让日志落到**最终生效**的装载结果；
        // 指针同样按契约只用一次。
        if (const char *data = hux_engine_data_info(engine_)) {
            FCITX_INFO() << "hux: data " << data;
        }
    }

    /// 与状态菜单共享的开关：角色 ↔ 配置页 schema 字段（配置页读的就是它）。
    ///
    /// 表同时供「写回值」与「取配置文件路径」用，字段名由编译器检查；角色下标即
    /// `HUX_OPTION_*`（顺序由 Rust 侧钉住）。共享开关分属两个分区，故一张分区一张表；
    /// 表外的角色不镜像（引擎新增了 schema 还没有的角色时配置页看不到它，无需镜像；
    /// 配置页仍能改，反向由 Rust 侧 `apply_settings` 写回存储）。
    static constexpr SharedRuntimeOption<HuxBehaviorConfig>
        kSharedBehaviorOptions[] = {
            {HUX_OPTION_EARLY_COMMIT, &HuxBehaviorConfig::earlyCommit},
            {HUX_OPTION_EARLY_COMMIT_TO_PREEDIT,
             &HuxBehaviorConfig::earlyCommitToPreedit},
            {HUX_OPTION_ALLOW_DUPLICATE_SINGLE,
             &HuxBehaviorConfig::allowDuplicateSingle},
            {HUX_OPTION_FULL_SHAPE, &HuxBehaviorConfig::fullShape},
            {HUX_OPTION_DIGIT_SELECT, &HuxBehaviorConfig::digitSelect},
        };
    static constexpr SharedRuntimeOption<HuxCharsetConfig>
        kSharedCharsetOptions[] = {
            {HUX_OPTION_FULL_CHARSET, &HuxCharsetConfig::fullCharset},
            {HUX_OPTION_FILTER_NON_HAN, &HuxCharsetConfig::filterNonHan},
        };

    /// 共享开关的字段类型（两个分区同款）。
    using SharedOption = SharedRuntimeOption<HuxBehaviorConfig>::Option;

    /// 在某分区的开关表里按角色取字段（表外角色返回空）。
    template <typename Partition, size_t N>
    static typename SharedRuntimeOption<Partition>::Option *
    findSharedOption(Partition &partition,
                     const SharedRuntimeOption<Partition> (&table)[N],
                     int32_t role) {
        for (const auto &entry : table) {
            if (entry.role == role) {
                return &(partition.*(entry.field));
            }
        }
        return nullptr;
    }

    /// 角色对应的 schema 字段与它在本配置文件里的路径（表外角色返回 `{nullptr, ""}`）。
    ///
    /// 路径由 schema 自身拼出（分区 `path()` + 字段 `path()`）、不写字面量：分区或字段改名时
    /// 「文件里显式写过吗」的判断不会失配（否则启动对齐会悄悄退回缺省值）。
    std::pair<SharedOption *, std::string> sharedOption(int32_t role) {
        HuxBehaviorConfig *behavior = config_.behavior.mutableValue();
        if (auto *option =
                findSharedOption(*behavior, kSharedBehaviorOptions, role)) {
            return {option, config_.behavior.path() + "/" + option->path()};
        }
        HuxCharsetConfig *charset = config_.charset.mutableValue();
        if (auto *option =
                findSharedOption(*charset, kSharedCharsetOptions, role)) {
            return {option, config_.charset.path() + "/" + option->path()};
        }
        return {nullptr, {}};
    }

    /// 状态菜单翻转后的镜像：把新值写回本 schema（配置页读的就是它），可选落盘。
    ///
    /// 不落盘只用于启动时的对齐（[`HuxEngine::adoptStoredRuntimeOptions`]）：那次写入的值
    /// 马上会由 `applyConfig()` 推回引擎，磁盘上的旧文件无需改写。
    void mirrorRuntimeRole(int32_t role, bool value, bool persist = true) {
        const auto shared = sharedOption(role);
        if (shared.first == nullptr) {
            return;
        }
        shared.first->setValue(value);
        if (!persist) {
            return;
        }
        // 与构造时的 `readAsIni`、宿主开关的写法对称：同路径、同 API 家族。
        if (!fcitx::safeSaveAsIni(config_, kConfigPath)) {
            FCITX_WARN() << "hux: 写入 " << kConfigPath << " 失败（运行时开关镜像）";
        }
    }

    /// 启动对齐：配置文件里**没写过**的共享键沿用引擎（`options.yaml`）的现存值。
    ///
    /// 这些开关的持久化值由引擎持有（`options.yaml`）；用户在状态菜单里的改动若从未落进
    /// 本 schema（文件缺失 / 只保存过配置页的其它项），直接 `applyConfig()` 会把 schema 缺省
    /// 推给引擎，从而在启动时把用户设置重置。故先按**文件里出现过的键**为界补齐：文件显式写过
    /// 的键以文件为准（配置页权威），没写过的键沿用引擎值（此后也会随 `apply_settings` 写回存储）。
    void adoptStoredRuntimeOptions() {
        fcitx::RawConfig raw;
        fcitx::readAsIni(raw, kConfigPath);
        // 角色集合与顺序取自引擎（ABI）：宿主 schema 还没有的角色由 `sharedOption` 返回空、跳过。
        const int32_t roles = hux_engine_option_role_count();
        for (int32_t role = 0; role < roles; ++role) {
            const auto shared = sharedOption(role);
            if (shared.first == nullptr ||
                raw.valueByPath(shared.second) != nullptr) {
                continue; // 文件显式给出 ⇒ 以文件为准
            }
            const char *key = hux_engine_option_key(engine_, role);
            if (key == nullptr) {
                continue;
            }
            const int32_t value = hux_engine_option_value(engine_, key);
            if (value >= 0) {
                shared.first->setValue(value == 1);
            }
        }
    }

    HuxConfig config_;
    fcitx::Instance *instance_;
    hux_engine *engine_ = nullptr;
    /// 会话工厂（每输入上下文一个 [`HuxSession`]）。
    fcitx::FactoryFor<HuxSession> sessionFactory_;
    /// 当前引擎调用的目标输入上下文：只在一次 `hux_engine_*` 调用期间置位、随即清空。
    /// IC 的生命周期长于本对象（见 `~HuxEngine` 契约注释），故该指针本身不会悬垂；
    /// 引擎释放后由 `applyCommit`/`applyUpdate` 的 `engine_ == nullptr` 早退兜住。
    fcitx::InputContext *context_ = nullptr;
    fcitx::Menu menu_;
    fcitx::SimpleAction menuAction_;
    std::vector<std::unique_ptr<HuxToggleAction>> toggleActions_;
    /// 宿主项开关「候选窗口显示预编辑」（引擎选项之外的一项，见 `HuxHostToggleAction`）。
    std::unique_ptr<HuxHostToggleAction> panelPreeditAction_;
    /// 宿主项「重新部署」。
    std::unique_ptr<HuxHostAction> redeployAction_;
    /// 宿主项「模型」信息行（不可点）。
    std::unique_ptr<HuxHostAction> modelAction_;
};

void HuxCandidateWord::select(fcitx::InputContext *inputContext) const {
    // 引擎（addon 实例）可能已先于本候选对象析构（IC 晚于 addon，见类注释）：
    // 弱引用失效 ⇒ 直接返回，**不触碰**已释放的引擎。
    HuxEngine *owner = owner_.get();
    if (owner == nullptr) {
        return;
    }
    owner->selectCandidate(inputContext, index_);
}

class HuxFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new HuxEngine(manager->instance());
    }
};

} // namespace

// C 布局守卫（与 Rust `crates/hux-ffi/src/lib.rs` 的 `c_layout_matches_header` 对应）：
// 本壳逐字段填充 `hux_options`、Rust 侧逐字段读取，字段顺序/宽度漂移在两侧都能编译通过，
// 故在此钉住尺寸与关键偏移——改 `hux_abi.h` 时必须同步三处。
static_assert(sizeof(hux_key_list) == 4 + 2 * HUX_MAX_KEYS * 4,
              "hux_key_list 布局与 Rust 契约不一致");
static_assert(sizeof(hux_options) == 15 * 4 + 4 * sizeof(hux_key_list),
              "hux_options 布局与 Rust 契约不一致");
static_assert(offsetof(hux_options, reverse_lookup_character) == 7 * 4 + sizeof(hux_key_list),
              "hux_options 字段顺序与 Rust 契约不一致");
static_assert(offsetof(hux_options, min_retained_input_length) == 12 * 4 + 4 * sizeof(hux_key_list),
              "hux_options 末尾字段偏移与 Rust 契约不一致");
static_assert(offsetof(hux_options, full_charset) == 13 * 4 + 4 * sizeof(hux_key_list),
              "hux_options 字集字段偏移与 Rust 契约不一致");

FCITX_ADDON_FACTORY(HuxFactory);
