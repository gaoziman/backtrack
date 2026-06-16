# 更新日志

本项目所有版本的更新内容记录于此。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## v0.1.0 - 2026-06-16

首个公开版本。Backtrack 是一个 macOS 桌面应用，把本地 **Claude Code** 与 **Codex** 的历史会话按目录归类，支持全文搜索、完整对话阅读与一键终端 `resume`。纯本地、只读、不联网、不调用任何 AI 模型。

### 新增

- **双工具扫描** — 自动索引 `~/.claude/projects` 与 `~/.codex/sessions` 下的全部会话。
- **目录为主轴** — 左栏按工作目录（cwd）聚合，Claude / Codex 混排并以彩色标签区分。
- **完整对话阅读器** — user/assistant 气泡、工具调用可折叠、Markdown 与代码高亮。
- **全文搜索** — 跨所有会话的子串搜索，CJK 友好，命中即点即达。
- **一键恢复** — 复制 `claude --resume <id>` / `codex resume <id>`，或唤起 iTerm / Terminal / Warp 自动 `cd` 并执行。
- **整理而不破坏** — 隐藏、关注置顶；删除走废纸篓（可恢复），绝不修改原始 jsonl。
