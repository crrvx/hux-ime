// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

// hux-ime（虎虚）fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libhux_addon）。
#include <fcitx-config/configuration.h>
#include <fcitx-config/option.h>
#include <fcitx-config/iniparser.h>
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

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

#include "hux_abi.h"

namespace {

/// 注意：`CommonCandidateList::setCursorIndex` 是**页内索引**（越界抛异常），
/// 绝对索引必须用 `setGlobalCursorIndex` + `setPage`；页大小来自配置 `PageSize`。
/// 页大小范围与 core `host::MAX_PAGE_SIZE` 一致（数字直选 0=第 10 个）。
constexpr int kPageSizeMin = 1;
constexpr int kPageSizeMax = 10;

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
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> tabLearning{{
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
    fcitx::OptionWithAnnotation<bool, fcitx::ToolTipAnnotation> panelPreedit{{
        .parent = this,
        .path{"PanelPreedit"},
        .description{"候选窗口显示预编辑"},
        .defaultValue = false,
        .annotation{"仅宿主显示项，不经引擎：候选窗口是否显示预编辑文本。"}}};
    // 值选项（`int` 等）列在布尔选项之后。
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        highFreqLimit{{
            .parent = this,
            .path{"HighFreqLimit"},
            .description{"高频字过滤上限"},
            .defaultValue = 1500,
            .constrain = fcitx::IntConstrain(0, 20000),
            .annotation{"仅使用最优码组句的高频字数量上限；0 = 不限制。修改后需重启生效。"}}};
    fcitx::Option<int, fcitx::IntConstrain, fcitx::DefaultMarshaller<int>,
                  fcitx::ToolTipAnnotation>
        pageSize{{
            .parent = this,
            .path{"PageSize"},
            .description{"每页候选个数"},
            .defaultValue = 5,
            .constrain = fcitx::IntConstrain(kPageSizeMin, kPageSizeMax),
            .annotation{"候选列表每页个数（1–10）；数字直选 `0` 对应第 10 个。"}}};);

/// 快捷键设置（配置页「快捷键」分区；`KeyList` 可多项，与全局设置同款）。
FCITX_CONFIGURATION(
    HuxHotkeyConfig,
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        soundToCharShapeKeys{{
            .parent = this,
            .path{"SoundToCharShapeKey"},
            .description{"音反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_colon, fcitx::KeyState::Alt)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后输入拼音（支持拼写缩写），候选为对应词语、"
                        "注释显示虎码。"}}};
    fcitx::KeyListOptionWithAnnotation<fcitx::ToolTipAnnotation>
        charToSoundShapeKeys{{
            .parent = this,
            .path{"CharToSoundShapeKey"},
            .description{"字反查"},
            .defaultValue = fcitx::KeyList{
                fcitx::Key(FcitxKey_quotedbl, fcitx::KeyState::Alt)},
            .constrain = fcitx::KeyListConstrain(
                fcitx::KeyConstrainFlag::AllowModifierLess),
            .annotation{"可多项。按下后显示光标左侧汉字的拼音与虎码"
                        "（需应用支持周边文本）。"}}};
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

/// 配置 schema：fcitx5-configtool 依据它自动生成设置页（fcitx://config/addon/hux）；
/// 分区结构参照全局设置（`Option<SubConfig>` → 分组标题，选项带悬浮说明）。
FCITX_CONFIGURATION(
    HuxConfig,
    fcitx::Option<HuxBehaviorConfig> behavior{this, "Behavior", "行为"};
    fcitx::Option<HuxHotkeyConfig> hotkeys{this, "Hotkey", "快捷键"};);

/// 「虎虚」状态菜单开关：勾选态取自引擎运行时选项（`options.yaml`），激活即翻转并落盘。
class HuxToggleAction : public fcitx::Action {
public:
    HuxToggleAction(hux_engine *engine, const char *option, const char *label)
        : engine_(engine), option_(option), label_(label) {
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
        if (value >= 0) {
            hux_engine_set_option(engine_, option_.c_str(), value == 0 ? 1 : 0);
        }
    }

private:
    hux_engine *engine_;
    std::string option_;
    std::string label_;
};

class HuxEngine;

/// 每输入上下文会话（fcitx5 `InputContextProperty`）：持引擎侧会话 id，销毁时释放。
/// 组合/候选/学习暂存按会话隔离，互不干扰。
class HuxSession : public fcitx::InputContextProperty {
public:
    HuxSession(hux_engine *engine, uint64_t id) : engine_(engine), id_(id) {}

    ~HuxSession() override { hux_engine_session_free(engine_, id_); }

    uint64_t id() const { return id_; }

private:
    hux_engine *engine_;
    uint64_t id_;
};

/// 面板候选：点击（`select`）按全局索引选中并上屏（与空格相同的确认/学习链）。
class HuxCandidateWord : public fcitx::CandidateWord {
public:
    HuxCandidateWord(fcitx::Text text, fcitx::Text comment, HuxEngine *owner,
                     int32_t index)
        : CandidateWord(std::move(text)), owner_(owner), index_(index) {
        setComment(std::move(comment));
    }

    void select(fcitx::InputContext *inputContext) const override;

private:
    HuxEngine *owner_;
    int32_t index_;
};

class HuxEngine : public fcitx::InputMethodEngine {
public:
    explicit HuxEngine(fcitx::Instance *instance)
        : instance_(instance),
          sessionFactory_([this](fcitx::InputContext & /*unused*/) {
              return new HuxSession(engine_, hux_engine_session_new(engine_));
          }) {
        fcitx::readAsIni(config_, "conf/hux.conf");
        hux_host host = {};
        host.user = this;
        host.commit = &HuxEngine::commitCallback;
        host.update = &HuxEngine::updateCallback;
        engine_ = hux_engine_new(&host);
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        // 每输入上下文一个会话（现存的与后续新建的都会经工厂创建）。
        instance_->inputContextManager().registerProperty("huxSession",
                                                          &sessionFactory_);
        applyConfig();
        setupStatusMenu();
    }
    ~HuxEngine() override {
        // 先注销并销毁全部会话（属性析构回调 `hux_engine_session_free`），再释放引擎。
        sessionFactory_.unregister();
        hux_engine_free(engine_);
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
        // 失焦/切换输入法/重置统一：直接丢弃当前组合（不提交）。客户端预编辑带
        // `DontCommit`（见 applyUpdate），核心失焦提交与前端协议 commit 串均为空；
        // 上游默认在切换输入法时提交候选/预编辑，本实现有意取「丢弃」契约。
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

    /// 状态菜单：注册「虎虚」子菜单与 5 项核心开关（构造时一次）。
    void setupStatusMenu() {
        static constexpr struct {
            const char *option;
            const char *label;
        } kToggles[] = {
            {"tiger_sentence_early_commit", "提前上屏"},
            {"tiger_sentence_early_commit_to_preedit", "提前上屏至预编辑"},
            {"tiger_sentence_allow_duplicate_single", "单字重码组句"},
            {"full_shape", "全角标点"},
            {"tiger_sentence_digit_select", "数字直选"},
        };
        menuAction_.setShortText("虎虚");
        for (const auto &toggle : kToggles) {
            auto action = std::make_unique<HuxToggleAction>(
                engine_, toggle.option, toggle.label);
            instance_->userInterfaceManager().registerAction(
                std::string("hux-") + toggle.option, action.get());
            menu_.addAction(action.get());
            toggleActions_.push_back(std::move(action));
        }
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
        if (context_ != nullptr && text != nullptr && *text != '\0') {
            context_->commitString(text);
        }
    }

    /// 应用 UI 快照：preedit（面板 + 客户端内联）+ 候选列表与高亮。
    void applyUpdate(const char *preedit, int32_t cursor,
                     const char *const *texts, const char *const *comments,
                     int32_t count, int32_t selected, const char *auxUp,
                     const char *auxDown) {
        if (context_ == nullptr) {
            return;
        }
        std::string preeditString = preedit != nullptr ? preedit : "";
        fcitx::Text preeditText(preeditString);
        if (cursor >= 0 &&
            static_cast<size_t>(cursor) <= preeditString.size()) {
            preeditText.setCursor(cursor);
        }
        // 候选窗口预编辑：可配置关闭（关闭后仅候选与注释）。
        context_->inputPanel().setPreedit(
            config_.behavior->panelPreedit.value() ? preeditText
                                                   : fcitx::Text());
        // 客户端内联预编辑：跟随 fcitx5 全局预编辑设置（`isPreeditEnabled`）。
        // 整段标记 `DontCommit`（核心对密码框同款做法）：核心失焦提交与 Wayland v1
        // 前端的协议 commit 串都取 `toStringForCommit()`，标记后为空——契约「失焦/切换/
        // 重置直接丢弃组合」由此成立，而显示不受影响。
        fcitx::Text clientPreedit;
        if (context_->isPreeditEnabled() && !preeditString.empty()) {
            clientPreedit.append(preeditString, fcitx::TextFormatFlag::DontCommit);
            if (cursor >= 0 &&
                static_cast<size_t>(cursor) <= preeditString.size()) {
                clientPreedit.setCursor(cursor);
            }
        }
        context_->inputPanel().setClientPreedit(clientPreedit);
        context_->updatePreedit();

        // 候选：无候选时置 `nullptr` 清除（fcitx5 约定）。**不可**留下「存在但为空」的
        // 列表——其他组件会对它调用 `candidate(0)`（如 fcitx5-table 的
        // `TableState::keyEvent`），抛 `CommonCandidateList: invalid index` 并 abort。
        if (count <= 0) {
            context_->inputPanel().setCandidateList(nullptr);
        } else {
            auto candidateList = std::make_unique<fcitx::CommonCandidateList>();
            for (int32_t index = 0; index < count; ++index) {
                const char *text =
                    texts != nullptr && texts[index] != nullptr ? texts[index] : "";
                const char *comment =
                    comments != nullptr && comments[index] != nullptr
                        ? comments[index]
                        : "";
                candidateList->append<HuxCandidateWord>(
                    fcitx::Text(text), fcitx::Text(comment), this, index);
            }
            // 数字直选：面板显示 1–9 / 0 序号（与引擎页内定位一致）。
            if (config_.behavior->digitSelect.value()) {
                candidateList->setSelectionKey(digitSelectionKeys());
            }
            // 翻页交由 fcitx5 面板（页大小与引擎一致）：绝对索引 → 全局光标 + 所在页。
            candidateList->setPageSize(std::clamp(
                config_.behavior->pageSize.value(), kPageSizeMin, kPageSizeMax));
            // 防御：越界不设光标索引。
            const int index = std::min(std::max(selected, 0), count - 1);
            candidateList->setGlobalCursorIndex(index);
            const int page = index / candidateList->pageSize();
            if (page < candidateList->totalPages()) {
                candidateList->setPage(page);
            }
            context_->inputPanel().setCandidateList(std::move(candidateList));
        }
        // 字反查（⑧-2）：两排辅助文本（上排 = 光标左、下排 = 光标右）；空串清除。
        const auto auxText = [](const char *value) {
            return value != nullptr && *value != '\0' ? fcitx::Text(value) : fcitx::Text();
        };
        context_->inputPanel().setAuxUp(auxText(auxUp));
        context_->inputPanel().setAuxDown(auxText(auxDown));
        context_->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
        updateStatusArea(context_);
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
        options.tab_learning = behavior.tabLearning.value() ? 1 : 0;
        options.high_freq_limit = behavior.highFreqLimit.value();
        fillKeyList(&options.sound_to_char_shape, hotkeys.soundToCharShapeKeys.value());
        fillKeyList(&options.char_to_sound_shape,
                    hotkeys.charToSoundShapeKeys.value());
        options.page_size = behavior.pageSize.value();
        fillKeyList(&options.page_up, hotkeys.pageUpKeys.value());
        fillKeyList(&options.page_down, hotkeys.pageDownKeys.value());
        options.digit_select = behavior.digitSelect.value() ? 1 : 0;
        if (hux_engine_apply_settings(engine_, &options) == 0) {
            FCITX_WARN() << "hux: apply settings failed";
        }
    }

    HuxConfig config_;
    fcitx::Instance *instance_;
    hux_engine *engine_ = nullptr;
    /// 会话工厂（每输入上下文一个 [`HuxSession`]）。
    fcitx::FactoryFor<HuxSession> sessionFactory_;
    fcitx::InputContext *context_ = nullptr;
    fcitx::Menu menu_;
    fcitx::SimpleAction menuAction_;
    std::vector<std::unique_ptr<HuxToggleAction>> toggleActions_;
};

void HuxCandidateWord::select(fcitx::InputContext *inputContext) const {
    owner_->selectCandidate(inputContext, index_);
}

class HuxFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new HuxEngine(manager->instance());
    }
};

} // namespace

FCITX_ADDON_FACTORY(HuxFactory);
