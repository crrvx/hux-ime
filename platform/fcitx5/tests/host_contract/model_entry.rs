// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 「虎虚」首项（模型入口）的源码级守卫：文案、可点、打开目录、ABI 声明与导出。
//!
//! ①～④ 四组断言，顺序 = 原用例小节序；ABI 部分跨读 `hux_abi.h` 与 `src/abi.rs`。

use super::source::Source;
use hux_test_support::repo_path;

/// 「虎虚」首项（模型入口）守卫（源码级）：
///   ① 文案 = 引擎给的那段原文（**不自带**「模型：」，否则与首项前缀叠成「虎虚：模型：…」）；
///   ② 首项**可点**：打开「所加载模型所在目录」，且仍带 `hux` 图标、仍登记在 `statusActions_`；
///   ③ 打开目录 = `hux_engine_model_path` → `parent_path` → `create_directories` → 双 fork
///      `execlp`（先 `xdg-open`、再 `gio open`），**不得**用 `std::system`（路径会经 shell 解释）；
///   ④ ABI 入口在头文件里声明、在 `abi.rs` 里导出（CI 另有 `nm -D` ↔ 头文件的动态比对）。
#[test]
fn model_entry_opens_the_model_directory() {
    let source = Source::read();
    model_text_prefix(&source);
    model_entry_is_clickable(&source);
    model_directory_launch(&source);
    model_path_abi_contract();
}

/// ① 文案：引擎原文 + 短兜底，不带任何前缀。
fn model_text_prefix(source: &Source) {
    let flat = source.flat();
    // ① 文案：引擎原文 + 短兜底，不带任何前缀；「虎虚：」只在调用方加一次。
    let text = source.function("std::string modelText() const");
    assert!(
        text.contains(r#"return info != nullptr ? std::string(info) : std::string("不可用");"#),
        "modelText() 必须只回引擎原文（nullptr 时短兜底）：{text}"
    );
    assert!(
        !text.contains("模型："),
        "modelText() 不得自带前缀（会与首项前缀叠成「虎虚：模型：…」）：{text}"
    );
    assert_eq!(
        flat.matches("+ modelText()").count(),
        1,
        "模型短名只应由首项文案消费（前缀各加一次）"
    );
    assert!(
        flat.contains(r#"return std::string("虎虚：") + modelText();"#),
        "首项文案 = 「虎虚：」+ 引擎原文（前缀只加一次）"
    );
}

/// ② 首项可点：`HuxHostAction` + hux 图标 + 登记进状态区。
fn model_entry_is_clickable(source: &Source) {
    let flat = source.flat();
    // ② 首项可点：HuxHostAction（带 activate）+ hux 图标 + 登记进状态区。
    assert!(
        flat.contains("menuAction_ = std::make_unique<HuxHostAction>(")
            && flat.contains("openModelDirectory(inputContext);")
            && flat.contains(r#"registerStatusAction("hux-menu", *menuAction_);"#),
        "首项必须是可点动作（点击打开模型目录）且仍登记在 statusActions_ 里"
    );
    let host_action = source.function("class HuxHostAction");
    assert!(
        host_action.contains(
            "std::string icon(fcitx::InputContext * /*unused*/) const override { return icon_; }"
        ),
        "首项必须保留 hux 图标（`icon_` 由构造参数给出）：{host_action}"
    );
    assert!(flat.contains(r#""hux");"#), "首项构造必须传入 hux 图标名");
}

/// ③ 打开目录：路径 → 父目录 → 建目录 → 拉起文件管理器（双 fork + `execlp`）。
fn model_directory_launch(source: &Source) {
    let flat = source.flat();
    // ③ 打开目录：路径 → 父目录 → 建目录 → 拉起文件管理器。
    let open = source.function("void openModelDirectory(");
    for fragment in [
        "const char *path = hux_engine_model_path(engine_);",
        "std::filesystem::path(path).parent_path()",
        "std::filesystem::create_directories(directory, error)",
        "launchFileManager(directory.string())",
    ] {
        assert!(
            open.contains(fragment),
            "打开模型目录缺少 {fragment}：{open}"
        );
    }
    let launch = source.function("static bool launchFileManager(");
    for fragment in [
        "const pid_t child = fork();",
        "const pid_t grandchild = fork();",
        r#"execlp("xdg-open", "xdg-open", directory.c_str(),"#,
        r#"execlp("gio", "gio", "open", directory.c_str(),"#,
        "waitpid(child, &status, 0)",
    ] {
        assert!(
            launch.contains(fragment),
            "拉起文件管理器缺少 {fragment}：{launch}"
        );
    }
    // 注释里会**提到** `std::system`（说明为何不用），故只按「调用形态」判定（带参数括号）。
    for forbidden in [
        "std::system(",
        "system(",
        "popen(",
        "execl(",
        "execlp(\"/bin/sh\"",
    ] {
        assert!(
            !flat.contains(forbidden),
            "不得用 {forbidden} 拉进程（路径经 shell 解释 = 注入面）"
        );
    }
}

/// ④ ABI：头文件声明（含指针有效期语义）+ Rust 导出。
fn model_path_abi_contract() {
    // ④ ABI：头文件声明（含指针有效期语义）+ Rust 导出。
    let header = std::fs::read_to_string(repo_path("crates/hux-ffi/include/hux_abi.h"))
        .expect("read hux_abi.h");
    // 逐行比对（含缩进）：注释掉的声明不算数（`contains` 会匹配注释里的同一行）。
    assert!(
        header
            .lines()
            .any(|line| line.trim()
                == "const char *hux_engine_model_path(const hux_engine *engine);"),
        "hux_abi.h 缺少（未被注释的）hux_engine_model_path 声明"
    );
    assert!(
        header
            .contains("未找到模型 ⇒ 默认查找路径（其父目录即「模型该放的地方」，文件可以不存在）")
            && header.contains("指针有效期同 hux_engine_model_info"),
        "hux_engine_model_path 的语义（未找到时给「该放的位置」+ 指针有效期）必须写进头文件"
    );
    let abi =
        std::fs::read_to_string(repo_path("platform/fcitx5/src/abi.rs")).expect("read abi.rs");
    let abi_flat = abi.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        abi_flat.contains(
            "#[unsafe(no_mangle)] pub unsafe extern \"C\" fn hux_engine_model_path(engine: *const Engine) -> *const c_char"
        ),
        "abi.rs 必须导出 hux_engine_model_path（否则 nm -D 与头文件不一致）"
    );
    assert!(
        abi_flat.contains(".map_or(std::ptr::null(), |path| path.as_ptr())"),
        "空路径必须回 NULL（宿主据此直接返回，不去开目录）"
    );
}
