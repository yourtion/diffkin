# Changelog

## Unreleased

## 0.3.0-alpha.3

- macOS 的 Apple Silicon 和 Intel 发布包默认给应用和 CLI 做 ad-hoc 签名。CI 从 DMG 和 CLI 压缩包中验签，并在 Windows 上静默安装 NSIS 包。
- 大型提交比较支持将最多 20,000 个变化路径冻结到临时磁盘。可按目录缩小比较范围，切换文件或刷新时保留阅读位置。
- macOS 和 Linux 可通过已登录的本机 Codex CLI 或 Claude Code 解释选中的代码变化，或生成变更测验。调用前可预览将发送的源码片段。

## 0.1.0-alpha.1

- 实现冻结 Git 比较：commits、staged、unstaged、worktree，分页、异常文件状态及无副作用读取。
- 增加 JS/TS/TSX 结构关系、保守格式证明、函数移动、简单局部改名、Vue script 分块和完整原始覆盖。
- 完成文件导航、双栏/统一 diff、搜索、可逆格式折叠、移动局部对照和绑定规则版本的审查标记。
- 增加任务预算、运行/排队取消、迟到响应隔离、118 个结构案例与真实 Git/浏览器交互验收。
- 采用 MIT，安装包与 CLI 附带第三方许可材料，修复 Windows CLI 归档路径兼容。

## 0.1.0-alpha.0

- 建立 React、TypeScript、Rsbuild/Rspack、CodeMirror 和 Tauri 2 桌面框架。
- 建立 Rust domain / analysis / application 与 CLI，共享有输入限制的文本比较。
- 增加 Rust → TypeScript 类型生成、文本完整性和 CLI 集成测试。
- 接入 Oxlint、Oxfmt、三平台 CI 与四目标 Release 草稿流程。
- 增加版本同步、tag 校验、安装包与 CLI 收集、SHA256SUMS 和可选 macOS 签名配置。
