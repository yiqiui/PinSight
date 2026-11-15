# PinSight 推广文案包（Promo Kit）

本文件是把 PinSight 推到站外时**直接可复制**的文案集合。改版本号时记得同步替换 `v0.1.x` 与下载链接里的版本。

仓库：`https://github.com/yiqiui/PinSight`
Releases（下载入口）：`https://github.com/yiqiui/PinSight/releases`

---

## 0. 一句话定位（tagline 备选，挑一句用）

- PinSight — a native, open-source screenshot & pin tool written in Rust. Lightweight, cross-platform, no Electron.
- 用 Rust 写的开源截图贴图工具：轻量、跨平台、不依赖浏览器内核。
- 截图 + 贴图 + 长截图 + 离线 OCR + 录屏 + 内置 MCP，一个原生 Rust 程序全搞定。
- Snipaste 式工作流的开源平替：可读、可改、可被 AI 调用。

---

## 1. GitHub Topics（去仓库 Settings → Topics 添加）

`rust` `screenshot` `screen-capture` `screenshot-tool` `pin` `pinboard` `annotation` `ocr` `long-screenshot` `screen-recording` `gif` `egui` `eframe` `windows` `macos` `linux` `cross-platform` `mcp` `cli` `productivity` `desktop-app`

> 关键词越全，GitHub 站内搜索和话题页的曝光越高。前 3 个（rust / screenshot / screen-capture）命中率最高。

---

## 2. Show HN（Hacker News）— 英文

**标题**
```
Show HN: PinSight – a Rust screenshot + pin tool with offline OCR and a built-in MCP server
```

**正文**
```
PinSight is an open-source, native screenshot tool I built in Rust (egui/eframe, no Electron/WebView).

It covers the Snipaste-style "capture → annotate → pin to screen" loop, plus things most
closed-source tools don't expose:

- Region / fullscreen capture, annotation (rect, arrow, pen, mosaic, numbered steps, text)
- Pin images to the desktop as always-on-top windows (native Win32 on Windows, egui viewport elsewhere)
- Scrolling long capture with automatic frame stitching
- Offline OCR: Windows.Media.Ocr / Apple Vision / Tesseract, one-click region OCR
- Screen recording → MP4 (ffmpeg) or GIF, plus a GIF editor
- Image upload, voice notes, CLI (scrot/maim style, headless), and a built-in MCP stdio server
  (9 tools) so Claude Desktop / Cursor / Continue can drive capture, pin, long-capture, recording

Why Rust: the exe is ~10 MB (UPX ~3.8 MB), every widget is hand-drawn vectors that stay crisp
at any DPI, and the macOS/Linux builds share the same tree (target-gated backends).

It's cross-platform (Windows/macOS/Linux, x64 + ARM64) and Apache-2.0. Early but daily-usable
on Windows; the long-capture edge cases are still being polished.

Repo + binaries: https://github.com/yiqiui/PinSight
```

---

## 3. 中文发布长文（少数派 / 知乎 / 掘金）— 可直接发

**标题（备选）**
- 用 Rust 重写了截图贴图工具：PinSight 开源了
- 一个不依赖浏览器的开源截图贴图工具，能 OCR、能长截图、还能被 AI 调用
- 告别 Snipaste 闭源焦虑：我做了个可读可改的截图贴图开源平替

**正文（约 1000 字，可直接发布）**

> 用了多年 Snipaste / PixPin 之后，我一直想要一个「截图 + 贴图」工作流的开源实现——核心可读、可改、还能被自动化调用。于是有了 PinSight。

**它是什么**

PinSight 是一个用 Rust（egui/eframe）写的原生桌面工具，**没有任何浏览器内核**（没有 Electron / Tauri / WebView）。它把高频的「截图 → 标注 → 贴到屏幕」闭环，和一堆进阶能力塞进了一个约 10 MB 的可执行文件（UPX 压缩后约 3.8 MB）。

**为什么值得一试**

1. **纯原生、轻量**。UI 全部用矢量手绘，任何 DPI / 缩放都不糊；启动快、内存低。
2. **跨平台**。Windows / macOS / Linux 一套代码树（按目标平台条件编译后端），x64 和 ARM64 都出包。Windows 上贴图是真正的原生置顶无边框窗口；macOS / Linux 降级为 egui 视口，能力一致。
3. **离线 OCR**。Windows 用系统自带的 `Windows.Media.Ocr`，macOS 用 Vision，Linux 用 Tesseract——不联网也能识别，工具栏一键圈选识别。
4. **长截图**。选区后自动滚动拼接，适合文档、聊天记录这类静态长页面。
5. **录屏 + GIF 编辑**。区域录屏导出 MP4（需 ffmpeg）或 GIF；还能对已有 GIF 做裁剪、加字、转 MP4。
6. **内置 MCP 服务**。`--mcp` 启动一个 stdio 服务，Claude Desktop / Cursor / Continue 能直接调用截图、贴图、长截图、录屏等 9 个工具——等于把桌面截图能力接进了你的 AI 工作流。
7. **可读、可改**。模块边界、线程模型、坐标空间、失败模式都写进了 `docs/ARCHITECTURE.md`；想加语言只需在 `i18n.rs` 加一列。

**怎么装**

普通用户直接去 [Releases](https://github.com/yiqiui/PinSight/releases) 下载对应系统的安装包或便携版即可，解压就能用。开发者 `git clone` 后 `cargo run`。

**适合谁**

如果你也受够了闭源截图工具的黑盒、想要一个能自己改、还能被 AI 调用的轻量贴图工具，PinSight 现在就能日常用（Windows 上最完整，长截图的粘性头部等极端情况还在打磨）。

**下一步**

- 去 GitHub 点个 Star，帮你判断值不值得继续投入；
- 遇到 bug 或想要功能，直接开 Issue；
- 有 macOS / Linux 真机的朋友，实测反馈最宝贵。

---

## 4. Reddit r/rust — 英文

**标题**
```
PinSight: an open-source Rust screenshot + pin tool (egui/eframe, no WebView) with OCR, long-capture and a built-in MCP server
```

**正文**
```
I've been using Snipaste/PixPin for years and wanted an open implementation of the
"capture → annotate → pin to screen" loop that's readable and automatable. PinSight is the result.

Stack: Rust 2024 edition, egui/eframe, xcap for capture, tiny-skia for rasterisation. No browser engine.
- ~10 MB exe (UPX ~3.8 MB), hand-drawn vector widgets (crisp at any DPI)
- Cross-platform: Windows/macOS/Linux, x64 + ARM64 from one tree (target-gated backends)
- Offline OCR (Win Media.Ocr / Vision / Tesseract), scrolling long-capture, region recording → MP4/GIF
- Built-in MCP stdio server (9 tools) for Claude/Cursor/Continue

Still early: daily-usable on Windows; long-capture edge cases (sticky headers, animations) are being polished.
Apache-2.0. Feedback and PRs very welcome, especially macOS/Linux real-device testing.

https://github.com/yiqiui/PinSight
```

---

## 5. Twitter / X 短帖

**英文**
```
Shipped PinSight v0.1.3 📸

A native, open-source screenshot + pin tool in Rust — no Electron.
Capture · annotate · pin-to-screen · long-capture · offline OCR · record · built-in MCP.
Cross-platform (Win/macOS/Linux, x64 + ARM64). ~10 MB.

👉 https://github.com/yiqiui/PinSight
#rustlang #opensource #screenshot
```

**中文**
```
做了个开源截图贴图工具 PinSight，用 Rust 写的，不依赖浏览器内核。
截图/标注/贴图/长截图/离线OCR/录屏，还内置 MCP 能被 AI 调用。
跨平台（Win/macOS/Linux，x64+ARM64），体积约 10MB。
👉 https://github.com/yiqiui/PinSight
#Rust #开源 #效率工具
```

---

## 6. Mastodon（dev 圈）

```
PinSight v0.1.3: an open-source, native (Rust/egui, no WebView) screenshot + pin tool.
Offline OCR, scrolling long-capture, region recording, and a built-in MCP server so your
AI client can drive capture/pin/record. Cross-platform, ~10 MB.

https://github.com/yiqiui/PinSight #rust #foss #screenshot
```

---

## 7. Release 描述模板（每次发版粘贴到 GitHub Release notes）

> 单段、每次整体覆盖（不要追加），从 `CHANGELOG.md` 的 `## [<version>]` 章节抽取 Added/Changed/Fixed 生成。

```markdown
## PinSight v0.1.3

**下载**：见上方 Assets（Windows x64/ARM64 安装包与便携版、macOS x64/Apple Silicon、Linux x64/ARM64）。

### 主要变化
- （从 CHANGELOG 复制 Added/Changed/Fixed 要点，3–6 条即可）

### 亮点
- 原生 Rust，无浏览器内核，约 10 MB
- 跨平台：Windows / macOS / Linux（x64 + ARM64）
- 截图 · 标注 · 贴图 · 长截图 · 离线 OCR · 录屏 · 内置 MCP

完整变更见 [CHANGELOG.md](CHANGELOG.md)。
```

---

## 8. 第一周推广行动清单（checklist）

- [ ] 仓库 Settings → Topics 填入第 1 节列表
- [ ] README 顶部加「普通用户直接下载」入口 + 构建/下载量徽章（见 README 改动）
- [ ] 发 Show HN（标题用第 2 节），并在评论里补一条「why Rust / lessons learned」
- [ ] 发 Reddit r/rust（第 4 节），下午欧美时区发曝光更好
- [ ] 发一条中文长文（少数派 / 知乎 / 掘金，第 3 节）
- [ ] 发 Twitter/X + Mastodon 短帖（第 5、6 节）
- [ ] 投稿 This Week in Rust newsletter（提交入口在官网，附一句话 + 链接）
- [ ] 在 r/opensource、r/software 也转一份（按需）
- [ ] 发版时套用第 7 节 Release 模板
- [ ] 前 3 天盯 Issue / 评论，当日响应；给早期 contributor 标 `good first issue`

> 节奏建议：同一天集中发 Show HN + Reddit + 长文，制造「当日热度」互相带动；之后每天一条短帖维持曝光。
