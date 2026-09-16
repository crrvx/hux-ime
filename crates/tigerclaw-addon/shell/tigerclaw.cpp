// 虎整句 fcitx5 addon 的 C++ 薄壳：只做 fcitx5 接口适配，逻辑在 Rust（libtigerclaw_addon）。
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

#include <memory>
#include <string>

#include "tigerclaw_abi.h"

namespace {

class TigerclawEngine : public fcitx::InputMethodEngine {
public:
    TigerclawEngine() {
        tigerclaw_host host = {};
        host.user = this;
        host.commit = &TigerclawEngine::commitCallback;
        host.update = &TigerclawEngine::updateCallback;
        engine_ = tigerclaw_engine_new(&host);
        if (const char *status = tigerclaw_engine_status(engine_)) {
            FCITX_INFO() << "tigerclaw: " << status;
        }
    }
    ~TigerclawEngine() override { tigerclaw_engine_free(engine_); }

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
        context_->inputPanel().setClientPreedit(preeditText);
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
            candidateList->setCursorIndex(selected);
        }
        context_->inputPanel().setCandidateList(std::move(candidateList));
        context_->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

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
