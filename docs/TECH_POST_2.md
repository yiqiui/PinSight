# 截图里的黑块从哪来：Windows WGC 与 GDI 的取舍，以及分层窗口被烤成纯黑矩形的根因

> 第二篇。第一篇讲了如何跨平台识别「光标底下的控件」（[TECH_POST.md](TECH_POST.md)）。
> 这一篇讲截图本身最容易翻车、也最隐蔽的一个坑：**截图工具自己的悬浮窗，为什么会变成一块纯黑矩形烤进画面里**。
>
> 仓库：https://github.com/yiqiui/PinSight （Apache-2.0，欢迎 Star 和反馈）

---

## 0. 那个黑块到底是什么

做截图工具的人迟早会撞上这个现象：你写了个置顶的悬浮工具条（截图时的标注栏、倒计时药丸之类），结果截出来的图里，那个工具条不是透明的、也不是正常渲染的，而是**一块纯黑、不透明的矩形**，严丝合缝地盖在原来的位置上。

它不是 bug 那么简单，它是**你选的截图后端和窗口类型不匹配**的必然结果。要解释清楚，得先说 Windows 上抓取屏幕的两条路。

---

## 1. 两条截图后端：WGC 与 GDI BitBlt

Windows 抓屏主要有两代方案：

- **WGC（Windows.Graphics.Capture，Win10 1803+）**：系统级的"屏幕捕获 API"，走的是桌面合成器（DWM）。它能拿到合成后的最终画面，而且**尊重窗口的"排除捕获"标记**。
- **GDI `BitBlt`**：老办法，从屏幕 DC 直接位块传输。它快、兼容性好，但它是"读帧缓冲"式的，**看不到也无法正确处理分层窗口（layered window）**，而且**完全无视"排除捕获"标记**。

我们在 PinSight 里以 WGC 为主后端，GDI 只作回退。而 WGC 这条路，在 Rust 侧是靠 `xcap` 这个库走的——但有个要命的细节：

```toml
# Cargo.toml —— 这个 features = ["wgc"] 是整件事的关键
xcap = { version = "0.9.8", features = ["wgc"] }
```

**`xcap` 默认并不启用 `wgc` feature。** 不带它的时候，`capture_image` / `capture_region` 会**静默退回 GDI `BitBlt`**，既不会报错、也不会警告。你以为自己在用现代 WGC，其实在用一个读帧缓冲的老后端——直到你发现黑块，才意识到后端被换了。

---

## 2. 黑块的根因：分层窗口 + GDI 无法合成

那个被烤黑的悬浮工具条，在技术上是 **layered window**（`WS_EX_LAYERED`，带每像素 alpha 或半透明合成）。分层窗口的像素是"合成出来"的，不在传统帧缓冲里。

- **WGC** 读的是 DWM 合成后的画面，分层窗口被正确合成进来，而且可以用 `WDA_EXCLUDEFROMCAPTURE` 标记让 WGC **直接丢掉**这个窗口——于是截图工具自己的 UI 根本不出现在画面里。
- **GDI `BitBlt`** 读的是底层帧缓冲。它**无法合成分层窗口**，于是那个窗口的区域就塌成一块**纯黑矩形**。更糟的是，GDI 也**无视 `WDA_EXCLUDEFROMCAPTURE`**，所以"排除捕获"在 GDI 路径下形同虚设。

代码里的注释把这件事说得直白：

```rust
// GDI 是 WGC 的等价回退：WDA_EXCLUDEFROMCAPTURE 对 BitBlt 无效，
// 而 BitBlt 无法合成分层（透明）窗口，于是它落地成一个黑矩形。
fn with_hidden_hud_pills<R>(f: impl FnOnce() -> R) -> R { /* 截前临时隐藏药丸 */ }
```

也就是说，一旦后端退回 GDI，你就面临两难：要么黑块，要么得在截图前**手动把悬浮窗隐藏掉再截**（这就是 `with_hidden_hud_pills` 做的事——截前隐藏 HUD 药丸、截完恢复，还要避开闪烁）。

修复方法简单到让人恼火：**给 `xcap` 加上 `wgc` feature**。一行依赖改动，后端从 GDI 切回 WGC，分层窗口被正确处理、`WDA_EXCLUDEFROMCAPTURE` 生效，黑块消失。这是我们 2026-10-27 实机验证过的决定性根因。

---

## 3. 第二个坑：HDR 显示器上 WGC 会"发白"

你以为切到 WGC 就万事大吉？还有 HDR。

WGC 返回的是合成缓冲。当显示器跑在 HDR（高级颜色）模式下，桌面合成的是 **scRGB 浮点**值，而 8-bit 的截图路径会直接 **clamp（截断）** 而不是做 tone-map（色调映射），结果就是整屏**发白、过曝**。

而 GDI 的屏幕 blit 读的是 DWM 已经 **tone-map 好的 SDR 合成画面**，看起来才是正确的。所以我们的策略是：**HDR 显示器上反过来优先走 GDI 路径**（同样用 `with_hidden_hud_pills` 把药丸藏掉），WGC 留给 SDR 场景。

怎么知道哪块屏幕是 HDR？走 DXGI 枚举输出，看它的色彩空间是不是 PQ / BT.2020：

```rust
// hdr.rs —— 高级颜色（HDR）检测
if desc.ColorSpace != DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 {
    continue; // 不是 HDR，跳过
}
// 记录该输出的物理矩形，截图时按区域重叠判断该走哪条后端
```

检测很便宜，但答案只在用户拨动 HDR 开关时变，所以加了个 2 秒 TTL 的缓存。

---

## 4. 给做截图 / 录屏工具的人几条经验

1. **一定要显式开 `xcap` 的 `wgc` feature。** 默认是 GDI 静默回退，黑块是最好的"你没开"的证据。
2. **自己的 UI 要么用分层窗口 + `WDA_EXCLUDEFROMCAPTURE`（配 WGC），要么在截前临时隐藏（配 GDI）。** 二选一，别指望 GDI 能正确处理透明 UI。
3. **HDR 不是"开了 WGC 就完事"。** WGC 在 HDR 下会发白，必要时应回退到读 DWM SDR 合成的 GDI 路径，或用 WGC 自己的 SDR tone-map 输出。
4. **后端选择是按显示器 / 区域动态决定的**，不是全局一个开关。SDR 用 WGC，HDR 区域用 GDI，才是稳态。
5. **回退路径也要测。** GDI 是回退，但它在 HDR 和分层窗口两个场景恰恰最关键，不能当作"备而不用"就放任。

---

## 5. 结尾

截图工具表面是个简单需求，底下全是这类"文档不写、踩了才知道"的平台细节。PinSight 把这些（WGC/GDI 后端选择、分层窗口、HDR tone-map、控件级识别）都收敛进一套跨平台代码树里，Windows 上最完整，macOS / Linux 还在打磨。

如果这篇对你有用，去仓库点个 Star，或者开个 Issue 说说你踩过的截图坑——这类实战经验比文档值钱。

- 仓库与二进制：https://github.com/yiqiui/PinSight
- 后端与黑块修复：`src/capture.rs`（含 `with_hidden_hud_pills`）
- HDR 检测：`src/hdr.rs`
- 上篇（控件级截图三后端）：[TECH_POST.md](TECH_POST.md)

---

## English abstract (for r/rust / Hacker News comments)

**Where the black rectangle in your screenshot comes from: WGC vs GDI BitBlt on Windows, and why a layered window bakes in as solid black.**

If you build a screenshot tool, you'll eventually hit this: your own always-on-top floating toolbar shows up in captures as a **solid black rectangle** instead of being transparent / excluded. The cause is the capture backend, not a rendering bug.

Windows has two screen-grab paths:
- **WGC** (`Windows.Graphics.Capture`, Win10 1803+): reads the DWM-composited frame and **honours `WDA_EXCLUDEFROMCAPTURE`**, so you can mark your own UI to be dropped from the frame.
- **GDI `BitBlt`**: reads the framebuffer. It **cannot composite a layered (per-pixel-alpha) window**, so that window lands as a black rectangle, and it **ignores `WDA_EXCLUDEFROMCAPTURE`** entirely.

We use `xcap` for capture, and the trap is that **`xcap` does not enable the `wgc` feature by default** — without `features = ["wgc"]` it *silently* falls back to GDI BitBlt, no error, no warning. Turning that feature on is the one-line fix for the black block (verified on real hardware). On the GDI path you instead have to hide your HUD before grabbing (`with_hidden_hud_pills`), because there's no other way to keep it out of the frame.

Second trap: on **HDR** displays WGC returns washed-out (scRGB clamped, not tone-mapped) frames, while a GDI blit reads DWM's already-tone-mapped SDR compositing and looks correct. So we detect HDR via DXGI (`DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020`) and prefer the GDI path on HDR regions. Backend selection is per-display / per-region, not a single global flag.

Code: `src/capture.rs`, `src/hdr.rs`. Repo: https://github.com/yiqiui/PinSight (Apache-2.0).
