# 安全策略

[English](#english)

## 支持的版本

片羽还没有发布版本。发布后，只为最新正式版修复安全问题。

## 报告漏洞

请不要开公开 issue。到仓库的 [Security](https://github.com/paralcor/pinfeather/security) 页点击 “Report a vulnerability”，[私下提交](https://github.com/paralcor/pinfeather/security/advisories/new)。

报告中请写明受影响的版本或提交、复现步骤和可能的影响，有概念验证更好。规划阶段发现的设计缺陷，也可以这样报告。

## 重点范围

- 剪贴板历史的本地存储：存储位置、文件权限、删除和清理是否彻底
- 敏感内容：“不进入剪贴板历史”标记、排除应用、暂停收录是否真的生效
- 回贴与输入注入：目标窗口校验，是否可能把内容粘贴到错误的窗口
- CLI 与 MCP：只能读取用户明确开放的集合，撤销后立即失效
- 局域网同步：配对、加密传输、撤销配对

## 处理流程

片羽由两位维护者在业余时间维护，通常一周内回复。确认后会在修复版本发布时公开安全公告，并在版本说明中致谢报告者（如你同意）。

---

## English

Pinfeather has no release yet; once it does, only the latest release receives security fixes. Please do not open a public issue. Report vulnerabilities, including design flaws found during planning, privately via [Security → Report a vulnerability](https://github.com/paralcor/pinfeather/security/advisories/new), with the affected version or commit, reproduction steps and impact.

Areas of interest: local storage of clipboard history, honoring sensitive-content markers, excluded apps and paused capture, paste and input injection into the right window, CLI and MCP access limited to collections the user explicitly shares, and LAN pairing and sync.

Pinfeather is maintained by two people in their spare time; expect a reply within about a week.
