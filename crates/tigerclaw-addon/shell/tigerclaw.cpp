// 虎爪（虎句方案）fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libtigerclaw_addon）。
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
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>

#include <algorithm>
#include <memory>
#include <string>

#include "tigerclaw_abi.h"

namespace {

/// 配置 schema：fcitx5-configtool 依据它自动生成设置页（fcitx://config/addon/tigerclaw）。
FCITX_CONFIGURATION(
    TigerclawConfig,
    fcitx::Option<bool> earlyCommit{this, "EarlyCommit", "提前上屏（组合中证据成熟即上屏）", true};
    fcitx::Option<bool> earlyCommitToPreedit{this, "EarlyCommitToPreedit", "提前上屏至预编辑（缓冲，不直接提交）", false};
    fcitx::Option<bool> allowDuplicateSingle{this, "AllowDuplicateSingle", "单字重码参与组句", true};
    fcitx::Option<bool> fullShape{this, "FullShape", "全角标点", false};
    fcitx::Option<bool> asciiPunct{this, "AsciiPunct", "ASCII 标点直通（不做中文标点映射）", false};
    fcitx::Option<bool> tabLearning{this, "TabLearning", "Tab 选字写入学习库", true};
    fcitx::Option<int, fcitx::IntConstrain> highFreqLimit{
        this, "HighFreqLimit", "高频字过滤上限（重启生效）", 1500, fcitx::IntConstrain(0, 20000)};
    fcitx::Option<fcitx::Key> reversePinyinKey{this, "ReversePinyinKey", "反查-拼音（点击录制按键）", fcitx::Key(FcitxKey_grave)};
    fcitx::Option<fcitx::Key> reverseHanziKey{this, "ReverseHanziKey", "反查-汉字（点击录制按键）", fcitx::Key(FcitxKey_grave, fcitx::KeyState::Shift)};
    fcitx::Option<fcitx::Key> quickInputKey{this, "QuickInputKey", "快速输入（点击录制按键）", fcitx::Key(FcitxKey_semicolon)};);

class TigerclawEngine : public fcitx::InputMethodEngine {
public:
    TigerclawEngine() {
        fcitx::readAsIni(config_, "conf/tigerclaw.conf");
        tigerclaw_host host = {};
        host.user = this;
        host.commit = &TigerclawEngine::commitCallback;
        host.update = &TigerclawEngine::updateCallback;
        engine_ = tigerclaw_engine_new(&host);
        if (const char *status = tigerclaw_engine_status(engine_)) {
            FCITX_INFO() << "tigerclaw: " << status;
        }
        applyConfig();
    }
    ~TigerclawEngine() override { tigerclaw_engine_free(engine_); }

    /// 配置 schema（fcitx5-configtool 生成设置页；保存到 ~/.config/fcitx5/conf/tigerclaw.conf）。
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
        context_ = keyEvent.inputContext();
        const int consumed =
            tigerclaw_engine_key(engine_, key.sym(), key.states().toInteger(),
                                 keyEvent.isRelease() ? 1 : 0);
        context_ = nullptr;
        if (consumed) {
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
        tigerclaw_engine_reset(engine_);
        context_ = nullptr;
    }

    static void commitCallback(void *user, const char *text) {
        static_cast<TigerclawEngine *>(user)->applyCommit(text);
    }

    static void updateCallback(void *user, const char *preedit, int32_t cursor,
                               const char *const *texts,
                               const char *const *comments, int32_t count,
                               int32_t selected) {
        static_cast<TigerclawEngine *>(user)->applyUpdate(
            preedit, cursor, texts, comments, count, selected);
    }

    void applyCommit(const char *text) {
        if (context_ != nullptr && text != nullptr && *text != '\0') {
            context_->commitString(text);
        }
    }

    /// 应用 UI 快照：preedit（面板 + 客户端内联）+ 候选列表与高亮。
    void applyUpdate(const char *preedit, int32_t cursor,
                     const char *const *texts, const char *const *comments,
                     int32_t count, int32_t selected) {
        if (context_ == nullptr) {
            return;
        }
        std::string preeditString = preedit != nullptr ? preedit : "";
        fcitx::Text preeditText(preeditString);
        if (cursor >= 0 &&
            static_cast<size_t>(cursor) <= preeditString.size()) {
            preeditText.setCursor(cursor);
        }
        context_->inputPanel().setPreedit(preeditText);
        // 客户端内联预编辑：跟随 fcitx5 全局预编辑设置（`isPreeditEnabled`）。
        context_->inputPanel().setClientPreedit(context_->isPreeditEnabled() ? preeditText
                                                                            : fcitx::Text());
        context_->updatePreedit();

        auto candidateList = std::make_unique<fcitx::CommonCandidateList>();
        for (int32_t index = 0; index < count; ++index) {
            const char *text =
                texts != nullptr && texts[index] != nullptr ? texts[index] : "";
            const char *comment = comments != nullptr && comments[index] != nullptr
                                      ? comments[index]
                                      : "";
            candidateList->append<fcitx::DisplayOnlyCandidateWord>(
                fcitx::Text(text), fcitx::Text(comment));
        }
        if (count > 0) {
            // 防御：越界不设光标索引。
            const int index = std::min(std::max(selected, 0), count - 1);
            candidateList->setCursorIndex(index);
        }
        context_->inputPanel().setCandidateList(std::move(candidateList));
        context_->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    /// 把 schema 值经 C ABI 推给 Rust 侧（`Settings::apply_settings`）。
    void applyConfig() {
        if (engine_ == nullptr) {
            return;
        }
        tigerclaw_options options = {};
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
        fillKey(&options.reverse_pinyin_sym, &options.reverse_pinyin_states, config_.reversePinyinKey.value());
        fillKey(&options.reverse_hanzi_sym, &options.reverse_hanzi_states, config_.reverseHanziKey.value());
        fillKey(&options.quick_input_sym, &options.quick_input_states, config_.quickInputKey.value());
        if (tigerclaw_engine_apply_settings(engine_, &options) == 0) {
            FCITX_WARN() << "tigerclaw: apply settings failed";
        }
    }

    TigerclawConfig config_;
    tigerclaw_engine *engine_;
    fcitx::InputContext *context_ = nullptr;
};

class TigerclawFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        FCITX_UNUSED(manager);
        return new TigerclawEngine;
    }
};

} // namespace

FCITX_ADDON_FACTORY(TigerclawFactory);
