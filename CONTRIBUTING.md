# 贡献指南

English contributions are welcome. Please follow the same rules below.

片羽还在规划阶段，没有代码。现在最有用的是对[交接文档](rumo_clipboard_handoff/docs/pinfeather_project_handoff.md)和[界面原型](design/prototype/README.md)的意见。

## 提 issue

- 请用 [issue 表单](https://github.com/paralcor/pinfeather/issues/new/choose)提交，并按要求填写。
- 一个 issue 只说一个问题，提交前先搜索已有 issue，包括已关闭的。
- 有发布版本后，报告问题前请先更新到最新版，确认问题仍然存在。
- 剪贴板里常有私人内容。issue 是公开的，贴出复制内容、截图或日志前，请先检查有没有密码、令牌、聊天记录等不想公开的东西。
- 安全漏洞请按 [SECURITY.md](SECURITY.md) 私下报告。

## 提 PR

- 大的改动请先开 issue 讨论，避免做了之后方向不合适。
- 一个 PR 只做一件事。几件不相关的事混在一起的 PR，会请你拆开再提交。
- 项目还没有代码，构建和测试命令确定后会补在这里。在那之前，请在 PR 里写明你实际运行了什么、手动测了什么。
- 按 PR 模板填写改了什么、为什么改、怎么验证的，以及 AI 使用说明。

## 依赖

- 每类能力只用一个库，不引入功能重叠的库。选定的库见界面原型说明的[实现约定](design/prototype/README.md#实现约定)；要换库或加新的一类，请先开 issue 讨论。
- `package.json` 里的依赖使用精确版本。
- 用 `npm install <包>@<版本>` 升级，让 npm 重新生成 `package-lock.json`，不要手改 lock 文件。
- 不要用 `overrides` 单独覆盖某个子包。同一组的包要一起升级，例如 `react` 和 `react-dom`、全部 `@tauri-apps/*`。
- Rust 依赖同理，用 `cargo update -p <包>` 升级，不要手改 `Cargo.lock`。

## 使用 AI 的说明

可以用 AI 辅助写代码、查问题或整理文字，但提 issue 和 PR 时必须如实说明：

- 写明有没有用 AI。用了的话，写出具体的工具和模型版本，例如 `chatgpt-6.1-sol`，不要只写“AI”或“ChatGPT”；再写明用在了哪里，比如发现问题、写代码、写测试、写说明。issue 表单和 PR 模板里都有对应的位置。
- AI 发现的问题，提交前请自己确认能复现，或者能在代码里指出具体位置；还没有确认的，请注明。
- AI 写的代码，提交人必须自己读过全部改动、在本地验证过，并在 PR 里写明验证方式。
- 没有如实说明，或者没有验证过的自动化 PR，会直接关闭。

AI tools are welcome, but disclose them: name the exact tool and model version (for example `chatgpt-6.1-sol`) and what it was used for. Undisclosed or unverified AI-generated PRs will be closed.

## 许可

提交贡献即表示你同意按本项目的 [MIT 许可](LICENSE)发布你的贡献。
