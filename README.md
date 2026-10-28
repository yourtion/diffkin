# Diffkin · 观变

看清代码的每一次变化。

轻量、本地、只读的代码比较客户端。当前版本 **0.1.0-alpha.2 / 核心 diff 候选**：真实 Git 快照、双栏与统一 diff、保守格式降噪、移动连线、局部改名分组，以及 Python / Java / Go / JS/TS / Rust / Vue script 分析。采用 **MIT** 许可证。

[公开仓库](https://github.com/yourtion/diffkin) · [Actions](https://github.com/yourtion/diffkin/actions) · [验收记录](docs/acceptance/m2.md)

## 开始使用

安装 `.node-version` 指定的 Node、根 `package.json` 指定的 pnpm、Rust/rustup，以及 **Git 2.45+**。桌面平台依赖见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)。

```sh
pnpm install --frozen-lockfile
pnpm desktop dev
```

输入本地仓库路径，选择比较模式：

| 模式         | 基线 → 候选                             |
| ------------ | --------------------------------------- |
| 全部本地改动 | HEAD → 原始工作区文件，可纳入未跟踪文件 |
| 已暂存       | HEAD → 冻结 index                       |
| 未暂存       | 冻结 index → 原始工作区文件             |
| 两个提交     | 两个输入 ref 解析并固定后的 commit      |

文件捕获后保持不变；刷新创建新比较。使用 J/K 切换文件、⌘/Ctrl F 搜索、Alt+↑/↓ 跳转变化。打开含移动的文件会突出首个移动关系；连线支持起点/终点定位，选择内部修改可直接对照其双侧内容。格式折叠有明确依据，可点击展开或切回完整原始视图。已审查标记绑定快照、内容和分析规则版本。

`pnpm dev` 只预览浏览器界面；本地 Git 和比较内核由桌面应用提供。不会执行仓库脚本、Git filters、外部 diff、hooks 或自动拉取缺失对象。

## 当前能力与边界

| 能力       | Alpha 范围                                                                                                                  |
| ---------- | --------------------------------------------------------------------------------------------------------------------------- |
| 文本       | UTF-8，保留 CR / LF / CRLF、Unicode 和末尾换行事实                                                                          |
| 格式       | JS/TS、Python、Java、Go、Rust 的保守语法/token 核对与有限标量括号归一；支持独立纯格式子区域，真实编辑保持可见               |
| 移动       | 同文件、同声明容器内唯一命名函数/方法的相对重排；JS/TS 支持简单变量绑定的箭头/函数表达式；继续显示内部修改                  |
| 局部改名   | JS/TS 简单参数和一级 let/const；Python 简单参数/赋值、Java 参数/local、Go 参数/var、Rust 参数/let；声明及全部引用对应才归组 |
| Vue        | SFC script 分块并复用 JS/TS 规则；模板、样式、块属性和跨块绑定保留文本                                                      |
| 展示       | 双栏/统一 diff、父子变更组、选中移动关系对照；JS/TS、Python、Java、Go、Rust、Vue 按文件类型高亮                             |
| Git 元数据 | 唯一内容改路径、模式、symlink、gitlink、LFS、冲突、binary/不可解码内容明确展示                                              |
| 大文件     | 细化上限每侧 200,000 字节 / 5,000 行；超过上限保留原文或明确预览/元数据状态                                                 |

格式、移动、改名与高亮的支持层级不同。解析错误、语义敏感区域、名称碰撞、引用增删和未知作用域明确回退；改名和移动始终可见，不推断行为等价。跨文件符号、复杂作用域改名和 Vue 模板绑定尚不属于确认型关系。各语言的实际范围与测试证据见[核心语言能力矩阵](docs/acceptance/core-languages.md)。

子模块工作区 dirty 不递归读取。超过 2 MiB 的 Git blob 只提供对象身份；工作区超大文件提供有限预览。原生验收由用户暂缓，其他未完成的产品验收见[M2 验收记录](docs/acceptance/m2.md)。

AI、多仓库联合审查、自动 rebase 和自动更新不在此版本中。

## CLI

```sh
cargo run -p diffkin-cli -- before.ts after.ts
cargo run -p diffkin-cli -- --structure before.ts after.ts
cargo run -p diffkin-cli -- --repo /path/to/repo --mode commits --base HEAD~1 --head HEAD
cargo run -p diffkin-cli -- --repo /path/to/repo --mode worktree --untracked
```

输出 JSON。退出码：`0` 无变化，`1` 有变化，`2` 错误。CLI 与桌面共用快照、文本和结构内核。

## 开发与检查

React 19 + TypeScript + CodeMirror 6；Rsbuild/Rspack；Tauri 2 + Rust workspace；Oxlint/Oxfmt。

```sh
pnpm check
pnpm protocol:check
pnpm build
pnpm test:ui
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI 会先运行 `cargo run -p diffkin-analysis --example ui_fixture`，并通过 `DIFFKIN_KERNEL_FIXTURE` 将真实分析结果交给浏览器联测。浏览器交互测试在本机使用已安装的 Chrome，CI 使用 Playwright Chromium。它验证实际界面与模拟 IPC 的交互；原生 WebView 验收单独记录。

| 目录                 | 职责                                       |
| -------------------- | ------------------------------------------ |
| `apps/desktop`       | React 界面、Tauri 桥接、有界任务和快照缓存 |
| `apps/cli`           | 文件与仓库比较，JSON 输出                  |
| `crates/domain`      | Rust DTO，生成 TypeScript 协议             |
| `crates/application` | 只读 Git 捕获、冻结内容与共享用例          |
| `crates/analysis`    | 无损文本、语法关系、覆盖证明和取消预算     |
| `fixtures/structure` | 冻结输入与明确 expected                    |
| `docs/acceptance`    | 正确性、性能原始记录与未验收事项           |
| `third-party`        | 第三方许可文本与可核对来源                 |
| `.github`、`scripts` | CI、版本、发版、协议与许可材料             |

## 发版

```sh
pnpm version:set 0.1.0-alpha.3
pnpm format
pnpm check
# 提交版本、源码及锁文件后
# git tag v0.1.0-alpha.3
# git push origin v0.1.0-alpha.3
```

Tag 触发版本校验、三平台 CI，以及 macOS Apple Silicon / Intel、Windows x64、Linux x64 构建。全部产物齐备后生成 `SHA256SUMS` 和 **Release 草稿**。CLI 与桌面包附带许可证和第三方材料。已公开 Release 不允许重跑覆盖。

详见 [CI 与发布说明](docs/releasing.md)。正式签名/公证和自动更新仍待配置；当前安装包供 Alpha 验证。内部包不会发布到 npm/crates.io。

## 设计文档

- [整体架构](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [质量与性能门槛](docs/quality.md)
- [M2 验收记录](docs/acceptance/m2.md)

原始讨论：[高级代码差异工具设计](chatgpt-conversation://6ae14eed-2b28-83ec-b62e-843ae0316ce8)。
