<div align="center">

# Backtrack

**本地 AI 会话浏览器** · 把散落的 Claude Code 与 Codex 历史会话，按目录归类、全文搜索、一键 `resume`。

[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-stable-000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=000)](https://react.dev)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](#-许可证)

</div>

> 会话散落在 `~/.claude/projects` 与 `~/.codex/sessions`，终端 `/resume` 列表难以辨认，想找回某次对话只能靠 grep。Backtrack 把它们扫成一个可浏览、可搜索、可恢复的本地索引。**纯本地、只读、不联网、不调用任何 AI 模型。**

![image-20260616211515585](https://gaoziman.oss-cn-hangzhou.aliyuncs.com/uPic/image-20260616211515585.png)

## ⬇️ 下载安装

前往 **[Releases](../../releases/latest)** 下载最新的 `.dmg`（Universal，兼容 Intel 与 Apple Silicon），拖入「应用程序」即可。

> 应用未经 Apple 签名，首次打开若提示「已损坏」或「无法验证开发者」，在终端执行后重新打开即可（开源未签名应用的正常现象）：
>
> ```bash
> xattr -cr /Applications/backtrack.app
> ```

需要自行编译见 [快速开始](#-快速开始)。

## ✨ 功能

- **双工具扫描** — 自动索引 `~/.claude/projects/**/*.jsonl` 与 `~/.codex/sessions/**/*.jsonl`。
- **目录为主轴** — 左栏按工作目录（cwd）聚合，右侧 Claude / Codex 混排，彩色标签区分。
- **完整对话阅读器** — user/assistant 气泡、工具调用可折叠、Markdown + 代码高亮。
- **全文搜索** — 跨所有会话子串搜索，**CJK 友好**，命中即点即达。
- **一键恢复** — 复制 `claude --resume <id>` / `codex resume <id>`，或唤起 iTerm / Terminal / Warp 自动 `cd` 并执行。
- **整理而不破坏** — 隐藏、关注置顶；删除走**废纸篓**（可恢复），**绝不修改原始 jsonl**。

## 🧱 技术栈

| 层 | 选型 |
|----|------|
| 框架 | Tauri 2 |
| 后端 | Rust · serde · walkdir · rusqlite(bundled) · rayon · trash · chrono |
| 前端 | React 19 · TypeScript · Vite 7 · zustand · Tailwind 4 |
| 渲染 | react-markdown · remark-gfm · rehype-highlight · highlight.js |
| 存储 | 磁盘 SQLite（`~/Library/Application Support/de.aigy.backtrack/index.db`） |

## 🏗️ 架构

```
扫描(scanner) → 并行解析(parsers/claude·codex) → SQLite 索引(store) → Tauri 命令(commands) → React 三栏 UI
```

| Rust 模块 | 职责 |
|-----------|------|
| `models.rs` | 共享类型（Tool / SessionMeta / Message / Project） |
| `scanner.rs` | 遍历会话根目录 |
| `parsers/{claude,codex,mod}.rs` | 解析两种 jsonl、标题提取、噪声过滤 |
| `indexer.rs` | 扫描 → rayon 并行解析 → 入库 |
| `store.rs` | SQLite 缓存 + 子串搜索 |
| `terminal.rs` | osascript 唤起终端 |
| `commands.rs` | Tauri IPC 命令 + AppState |

## 🚀 快速开始

**环境要求**：macOS · [Rust 工具链](https://rustup.rs) · Node 18+ · [pnpm](https://pnpm.io)

```bash
pnpm install
pnpm tauri dev      # 开发模式（热重载）
pnpm tauri build    # 打包 .app / .dmg
```

> 国内网络：未开代理时，cargo 走 `.cargo/config.toml` 内置的 rsproxy.cn 镜像直连。

**测试（Rust 核心）**

```bash
cd src-tauri
cargo test                                            # 全部单元测试
cargo test real_data_smoke -- --ignored --nocapture   # 针对真实磁盘数据的冒烟测试
```

## 📂 数据格式

| 工具 | 路径 | resume |
|------|------|--------|
| Claude | `~/.claude/projects/<编码cwd>/<uuid>.jsonl` | `claude --resume <uuid>` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`（首行 `session_meta` 含 cwd） | `codex resume <uuid>` |

## 🗺️ 路线图

当前 **v0.1（P0）** 功能完成。后续计划：

- [x] 文件变更监听，自动刷新（v0.3.0）
- [ ] 单会话导出 Markdown / HTML
- [ ] 搜索过滤器（按工具 / 目录 / 时间）
- [ ] Codex `forked_from` fork 关系可视化

**v1 不做**：云同步 · 实时监听 · 导出 · 接入其他工具 · 调用任何 AI 模型。

完整版本更新历史见 [CHANGELOG](CHANGELOG.md)。

## 🤝 贡献

欢迎 Issue 与 PR。改动请保持与现有代码风格一致；涉及 Rust 核心逻辑的请附带 `cargo test` 通过的测试。

## 🔒 隐私

所有数据仅在本机读取与索引，不上传、不联网、不调用任何远端服务。

## 📄 许可证

本项目基于 [MIT](LICENSE) 许可证开源 · Copyright (c) 2026 gaoziman
