// hux-ime（虎句方案）fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libhux_addon）。
#include <fcitx-config/configuration.h>
#include <fcitx-config/option.h>
#include <fcitx-config/iniparser.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/candidatelist.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputmethodentry.h>
#include <fcitx/surroundingtext.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>

#include <algorithm>
#include <memory>
#include <string>

#include "hux_abi.h"

namespace {

/// 候选页大小（与 core `host::DEFAULT_PAGE_SIZE` 一致；参照 schema `menu/page_size: 5`）。
/// 注意：`CommonCandidateList::setCursorIndex` 是**页内索引**（越界抛异常），
/// 绝对索引必须用 `setGlobalCursorIndex` + `setPage`。
constexpr int kCandidatePageSize = 5;

/// 配置 schema：fcitx5-configtool 依据它自动生成设置页（fcitx://config/addon/hux）。
FCITX_CONFIGURATION(
    HuxConfig,
    fcitx::Option<bool> earlyCommit{this, "EarlyCommit", "提前上屏（组合中证据成熟即上屏）", true};
    fcitx::Option<bool> earlyCommitToPreedit{this, "EarlyCommitToPreedit", "提前上屏至预编辑（缓冲，不直接提交）", false};
    fcitx::Option<bool> allowDuplicateSingle{this, "AllowDuplicateSingle", "单字重码参与组句", true};
    fcitx::Option<bool> fullShape{this, "FullShape", "全角标点", false};
    fcitx::Option<bool> asciiPunct{this, "AsciiPunct", "ASCII 标点直通（不做中文标点映射）", false};
    fcitx::Option<bool> tabLearning{this, "TabLearning", "Tab 选字写入学习库", true};
    fcitx::Option<int, fcitx::IntConstrain> highFreqLimit{
        this, "HighFreqLimit", "高频字过滤上限（重启生效）", 1500, fcitx::IntConstrain(0, 20000)};
    // 单键选项须显式放宽「允许无修饰键」：默认 KeyConstrain 会拒绝 ` / ; 这类
    // 无修饰键（配置工具的按键录制会报「不满足约束」）。
    fcitx::Option<fcitx::Key, fcitx::KeyConstrain> pinyinLookupKey{
        this, "PinyinLookupKey", "音查虎：用拼音查虎码",
        fcitx::Key(FcitxKey_semicolon, fcitx::KeyState::Alt),
        fcitx::KeyConstrain(fcitx::KeyConstrainFlag::AllowModifierLess)};
    fcitx::Option<fcitx::Key, fcitx::KeyConstrain> characterLookupKey{
        this, "CharacterLookupKey", "字查音+虎：查光标左侧汉字的拼音与虎码",
        fcitx::Key(FcitxKey_apostrophe, fcitx::KeyState::Alt),
        fcitx::KeyConstrain(fcitx::KeyConstrainFlag::AllowModifierLess)};
    fcitx::Option<bool> panelPreedit{this, "PanelPreedit", "候选窗口显示预编辑文本", false};);

class HuxEngine : public fcitx::InputMethodEngine {
public:
    HuxEngine() {
        fcitx::readAsIni(config_, "conf/hux.conf");
        hux_host host = {};
        host.user = this;
        host.commit = &HuxEngine::commitCallback;
        host.update = &HuxEngine::updateCallback;
        engine_ = hux_engine_new(&host);
        if (const char *status = hux_engine_status(engine_)) {
            FCITX_INFO() << "hux: " << status;
        }
        applyConfig();
    }
    ~HuxEngine() override { hux_engine_free(engine_); }

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
        context_ = inputContext;
        // 应用侧周边文本（字查音+虎用；应用不支持时 valid=0）。
        const auto &surrounding = inputContext->surroundingText();
        if (surrounding.isValid()) {
            hux_engine_set_surrounding(engine_, surrounding.text().c_str(),
                                             static_cast<int32_t>(surrounding.cursor()), 1);
        } else {
            hux_engine_set_surrounding(engine_, nullptr, 0, 0);
        }
        const int32_t disposition =
            hux_engine_key(engine_, key.sym(), key.states().toInteger(),
                                 keyEvent.isRelease() ? 1 : 0);
        context_ = nullptr;
        if (disposition & HUX_KEY_FORWARD_AFTER_COMMIT) {
            // 已提交且未消费：先让提交送达，再由本层重发按键（与核心
            // `KeyEventOrderFix` 修法一致），避免前端在 keyEvent 返回后立刻
            // 转发按键导致「字母先于候选上屏」。
            keyEvent.filterAndAccept();
            inputContext->forwardKey(keyEvent.origKey(), keyEvent.isRelease(),
                                     keyEvent.time());
        } else if (disposition & HUX_KEY_CONSUMED) {
            keyEvent.filterAndAccept();
        }
    }

    void activate(const fcitx::InputMethodEntry &entry,
                  fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        resetSession(event);
    }

    void deactivate(const fcitx::InputMethodEntry &entry,
                    fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        resetSession(event);
    }

    void reset(const fcitx::InputMethodEntry &entry,
               fcitx::InputContextEvent &event) override {
        FCITX_UNUSED(entry);
        resetSession(event);
    }

private:
    /// 清空会话与面板（activate/deactivate/reset 共用）。
    void resetSession(fcitx::InputContextEvent &event) {
        context_ = event.inputContext();
        hux_engine_reset(engine_);
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
        context_->inputPanel().setPreedit(config_.panelPreedit.value() ? preeditText
                                                                      : fcitx::Text());
        // 客户端内联预编辑：跟随 fcitx5 全局预编辑设置（`isPreeditEnabled`）。
        context_->inputPanel().setClientPreedit(context_->isPreeditEnabled() ? preeditText
                                                                            : fcitx::Text());
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
                candidateList->append<fcitx::DisplayOnlyCandidateWord>(
                    fcitx::Text(text), fcitx::Text(comment));
            }
            // 翻页交由 fcitx5 面板（页大小与引擎一致）：绝对索引 → 全局光标 + 所在页。
            candidateList->setPageSize(kCandidatePageSize);
            // 防御：越界不设光标索引。
            const int index = std::min(std::max(selected, 0), count - 1);
            candidateList->setGlobalCursorIndex(index);
            const int page = index / candidateList->pageSize();
            if (page < candidateList->totalPages()) {
                candidateList->setPage(page);
            }
            context_->inputPanel().setCandidateList(std::move(candidateList));
        }
        // 字查音+虎（⑧-2）：两排辅助文本（上排 = 光标左、下排 = 光标右）；空串清除。
        const auto auxText = [](const char *value) {
            return value != nullptr && *value != '\0' ? fcitx::Text(value) : fcitx::Text();
        };
        context_->inputPanel().setAuxUp(auxText(auxUp));
        context_->inputPanel().setAuxDown(auxText(auxDown));
        context_->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    /// 把 schema 值经 C ABI 推给 Rust 侧（`Settings::apply_settings`）。
    void applyConfig() {
        if (engine_ == nullptr) {
            return;
        }
        hux_options options = {};
        options.early_commit = config_.earlyCommit.value() ? 1 : 0;
        options.early_commit_to_preedit = config_.earlyCommitToPreedit.value() ? 1 : 0;
        options.allow_duplicate_single = config_.allowDuplicateSingle.value() ? 1 : 0;
        options.full_shape = config_.fullShape.value() ? 1 : 0;
        options.ascii_punct = config_.asciiPunct.value() ? 1 : 0;
        options.tab_learning = config_.tabLearning.value() ? 1 : 0;
        options.high_freq_limit = config_.highFreqLimit.value();
        const auto fillKey = [](int32_t *sym, int32_t *states, const fcitx::Key &key) {
            *sym = static_cast<int32_t>(key.sym());
            *states = static_cast<int32_t>(key.states().toInteger());
        };
        fillKey(&options.pinyin_lookup_sym, &options.pinyin_lookup_states, config_.pinyinLookupKey.value());
        fillKey(&options.character_lookup_sym, &options.character_lookup_states, config_.characterLookupKey.value());
        if (hux_engine_apply_settings(engine_, &options) == 0) {
            FCITX_WARN() << "hux: apply settings failed";
        }
    }

    HuxConfig config_;
    hux_engine *engine_;
    fcitx::InputContext *context_ = nullptr;
};

class HuxFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        FCITX_UNUSED(manager);
        return new HuxEngine;
    }
};

} // namespace

FCITX_ADDON_FACTORY(HuxFactory);
