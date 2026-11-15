# 用 Rust 实现跨平台「控件级截图」：Windows / macOS / Linux 三套后端与一个 dlsym 暗雷

> 本文记录 PinSight（一个用 Rust 写的开源截图贴图工具）里「控件级截图」功能的实现。
> 所谓控件级截图，就是光标指到哪个按钮 / 输入框 / 列表行，就只截那一个控件，而不是整块屏幕。
> 听起来简单，但要让它在三个操作系统上都跑起来，每一家都有一套完全不同的 API，外加一堆文档不会告诉你的坑。
>
> 仓库：https://github.com/yiqiui/PinSight （Apache-2.0，欢迎 Star 和 macOS / Linux 真机反馈）

---

## 0. 为什么要做这件事

截图工具很多，但「截一个控件」是个少有人认真做的需求：写文档、做教程、报 bug 时，最想要的往往不是全屏，而是一个干净的按钮或一段对话气泡。

难点不在于「截图」本身（那个用 `xcap` 走 Windows Graphics Capture / 平台后端就行），而在于**「光标底下到底是什么控件」**——这是一个可访问性（Accessibility）问题，三家给出的答案完全不一样：

| 平台 | 控件识别 API | 顶层窗口识别 | 兜底 |
| --- | --- | --- | --- |
| Windows | UI Automation `ElementFromPoint` | Win32 `GetAncestor(GA_ROOT)` 命中测试 | 窗口列表取最小包含者 |
| macOS | Accessibility `AXUIElementCopyElementAtPosition` | 窗口列表 | 窗口列表取最小包含者 |
| Linux (X11) | X11 窗口树遍历（最深的映射窗口） | 窗口列表 | 窗口列表取最小包含者 |

核心设计就一句话：**每个平台贡献它能真正解析到的东西，解析不到就统一退回同一个跨平台兜底**——从 `xcap` 枚举出的窗口里，取**包含光标、面积最小**的那个。这个兜底在所有平台都能用（原生 Wayland 除外，它压根不把窗口几何暴露给客户端），也是重叠窗口下最稳的选择。

还有一个贯穿全局的约定：**所有矩形都留在平台原生全局坐标空间里**，也就是 `xcap` 报告显示器 / 窗口时用的同一套坐标。Windows 和 Linux 是物理像素，macOS 是 point——因为三家都和 `xcap` 的 rect 在同一个空间，所以整条流水线**不需要任何坐标转换**。这一点省掉了无数跨平台项目最容易翻车的坐标换算 bug。

---

## 1. Windows：UIA 是 COM，COM 会死锁

Windows 上控件识别有两层：

1. **UI Automation（UIA）**：能识别现代 UWP / WinUI / Electron 里的"逻辑控件"，这是 Win32 老接口看不到的。用 `CUIAutomation::ElementFromPoint` 拿到元素，再按 `Control` / `Container` / `Window` 三档决定要不要沿树往上走一层（容器）或到根（窗口）。
2. **Win32 兜底**：`WindowFromPoint` 拿最深的 HWND，需要容器就 `GetParent` 一步，需要整窗就 `GetAncestor(GA_ROOT)`。这层能兜住 UIA 可能漏掉的传统 Win32 控件。

第一个坑来了：**UIA 是经典 COM API，绝不能跑在 GUI 线程上**。我们的主线程是 STA（单线程单元），而同步的 UIA 调用在 STA 上会因为重入陷阱直接死锁。所以识别必须丢到一个临时的 MTA（多线程单元）worker 线程里跑——和 OCR 那条路径用的是同一个套路：

```rust
std::thread::spawn(move || {
    let _ = tx.send(detect_on_worker(cursor, level));
});
// 关键：绑定等待时间，provider 卡死也绝不会冻结整个截图动作
rx.recv_timeout(Duration::from_millis(800)).ok().flatten()
```

worker 里第一件事是给自己 `CoInitializeEx(COINIT_MULTITHREADED)` 开一个独立单元，跑完再 `CoUninitialize`。再叠一个 **800ms 超时**：任何一家 provider 抽风卡住，最多让这次识别失败、退回 Win32 / 窗口列表兜底，而不会把截图热键冻死。

---

## 2. macOS：手写了 FFI，还撞上链接器的「未定义符号」

macOS 走 Accessibility API：先 `AXUIElementCreateSystemWide()` 拿系统级元素，再 `AXUIElementCopyElementAtPosition(x, y)` 拿光标处的元素，然后读 `AXPosition` / `AXSize` 属性拿到矩形。要容器就再读一次 `AXParent`。

为了不引入新 crate，这层是**手写的 FFI**——`AXUIElement*` 系列函数在 `ApplicationServices` umbrella 里，接口小且稳定。但第二个、也是最阴的坑在这里：

`kAXValueCGPointType` / `kAXValueCGSizeType` 这两个常量，是 HIServices 子框架导出的 `CFStringRef` 全局变量。新版 SDK 里，`ApplicationServices` 的 tbd（文本化链接桩）**不再列出这两个常量**了——函数还能链，常量链不了。于是你写个 `extern static` 去引用它们，链接阶段直接炸：

```
Undefined symbols for architecture arm64:
  _kAXValueCGPointType
```

文档不会提这个。解决办法是**运行时用 `dlopen` + `dlsym` 去框架里现找**：

```rust
fn ax_constant(name: &str) -> Option<CF> {
    static HANDLE: OnceLock<usize> = OnceLock::new();
    let handle = *HANDLE.get_or_init(|| unsafe {
        dlopen(FRAMEWORK_PATH.as_ptr(), RTLD_NOW) as usize
    });
    // ...dlsym 拿到符号地址，再解引用一次得到 CFStringRef 本身
    Some(unsafe { *(address as *const CF) })
}
```

`dlopen` / `dlsym` 来自 libSystem，不用额外加链接 flag。用 `OnceLock` 缓存句柄，每次识别只做一次符号查找。还有一个前提：macOS 这套 API **需要用户在「系统设置 → 隐私与安全性 → 辅助功能」里授权**；没授权时调用返回错误，代码直接退回跨平台兜底——所以 macOS 上"控件级"是尽力而为，拿不到就至少能截到窗口。

> 顺带一提，`c_char` / `c_void` 这些基础类型要在 `cfg(macos)` 代码块里 `use core::ffi::{c_char, c_void};` 显式导入，否则 Windows 编译不到那段，但 macOS 编译会报"找不到 c_char"。这是 Rust 2024 edition 下很典型的"平台条件代码各自为政"的坑。

---

## 3. Linux：窗口树遍历，以及现代工具集的天花板

Linux（X11 / XWayland）走 `x11rb` 做窗口树遍历。从根窗口出发，逐层 `query_tree`，每一层取**最顶层的、且包含光标的子窗口**，一路下钻到最深的映射窗口——它就是光标处的"控件"；它的父是容器，根之下的顶层祖先是整窗。

```rust
// QueryTree 返回的子窗口是「底到顶」排列的，所以反向遍历先测最顶层的兄弟
let hit = children.iter().rev().find(|&&child| {
    root_rect(&conn, root, child)
        .is_some_and(|r| r.w > 1 && r.h > 1 && r.contains(cursor.0, cursor.1))
});
```

坐标要把窗口局部几何通过 `translate_coordinates` 转到根坐标系（X11 也是物理像素，和 `xcap` 同空间）。

但这里有个绕不开的天花板：**现代 GTK / Qt 的控件根本不是 X11 窗口**。它们把整个应用画在一个顶层 X11 窗口里，内部按钮、输入框只是"画出来的"，X11 看不到。所以 X11 路径能解析到的，是这些控件**最近的那个映射祖先窗口**——这是 X11 在不接 AT-SPI（D-Bus 桥）的前提下能给出的诚实上限。要真正做到逐控件，得再接一层 AT-SPI，那是另一个量级的工作。原生 Wayland 更甚：合成器不把窗口几何暴露给客户端，连窗口列表兜底都拿不到，只能退回整屏。

---

## 4. 给想做同类项目的人的几条经验

1. **别在 GUI 线程碰同步 COM / 任何会重入的可访问性 API**。开 MTA worker + 超时，是保命底线。
2. **跨平台坐标空间先统一约定**，比写一堆换算函数省心得多。我们直接复用 `xcap` 的坐标空间，零转换。
3. **链接错误不一定是代码错**。`kAXValue*Type` 这种"函数能链、常量不能链"的坑，dlsym 运行时解析比改链接脚本稳。
4. **每个平台都要有兜底**，而且兜底最好和主路径用同一份数据（窗口列表）。这样某个平台 API 不可用，体验只是"降级"而不是"崩"。
5. **能力上限要诚实写进文档**。macOS 控件级需要授权、Linux 对现代工具集只能到窗口级、Wayland 只能整屏——这些在 README 里明说，比被用户骂"截图不准"强。

---

## 5. 结尾

这套混合引擎让 PinSight 在 Windows 上能真正截到 UWP / Electron 的逻辑控件，在 macOS / Linux 上至少稳稳截到窗口级，且三平台共享同一棵代码树（按目标平台条件编译后端）。

项目还是早期（v0.1.3），长截图的粘性头部等极端情况还在打磨。**最缺的是 macOS / Linux 真机上的实测反馈和 PR**——如果你手上有这两家的机器，跑一下、开个 Issue，对项目的价值比 Star 还大。

- 仓库与二进制：https://github.com/yiqiui/PinSight
- 控件级截图实现：`src/snap.rs`（上面那些代码都出自这里，带单元测试）
- 架构与设计取舍：`docs/ARCHITECTURE.md`

---

## English abstract (for r/rust / Hacker News comments)

**Cross-platform "control-level" screenshot in Rust — three backends and one dlsym landmine.**

PinSight is an open-source Rust (egui/eframe, no WebView) screenshot tool. The "smart capture" feature snaps the UI control under the cursor (a button, an input, a list row) instead of the whole screen. Getting the control under the point is an accessibility problem, and each OS answers differently:

- **Windows**: UI Automation `ElementFromPoint` (resolves modern UWP/WinUI/Electron widgets) + a Win32 `GetAncestor`/`WindowFromPoint` fallback. The catch: UIA is synchronous COM and deadlocks on the STA GUI thread, so it runs on a throwaway MTA worker with an 800 ms timeout.
- **macOS**: hand-written FFI over `AXUIElementCopyElementAtPosition`. The catch: `kAXValueCGPointType`/`kAXValueCGSizeType` are no longer exported by the ApplicationServices tbd, so a static `extern` fails to link — resolved at runtime via `dlopen`+`dlsym`. Also requires the Accessibility permission, otherwise it degrades to the window-level fallback.
- **Linux (X11)**: walk the window tree with `x11rb`, take the deepest mapped window containing the point. Honest ceiling: modern GTK/Qt widgets aren't X11 windows, so they resolve to the nearest mapped ancestor; native Wayland exposes no window geometry at all.

The unifying trick: every rect stays in the platform's native coordinate space (the same one `xcap` reports), so the pipeline needs zero conversion. Each platform degrades to one shared fallback — the smallest window from `xcap`'s list that contains the cursor.

Code: `src/snap.rs`. Repo: https://github.com/yiqiui/PinSight (Apache-2.0). Real-device macOS/Linux testing and PRs are the most valuable contribution right now.
