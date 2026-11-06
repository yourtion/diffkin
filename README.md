# Diffkin · 观变

看清代码的每一次变化。

Diffkin 是轻量、本地的代码审查客户端。**0.3.0-alpha.3** 支持只读 Git 与多语言结构 diff、多仓库冻结审查，以及需要明确确认的单仓固定提交验证。采用 **MIT** 许可证。

[公开仓库](https://github.com/yourtion/diffkin) · [Actions](https://github.com/yourtion/diffkin/actions) · [本轮验收记录](docs/acceptance/large-repositories.md) · [M3/M4 验收记录](docs/acceptance/m3-m4.md)

## 开始使用

安装 `.node-version` 指定的 Node、根 `package.json` 指定的 pnpm、Rust/rustup，以及 **Git 2.45+**。桌面平台依赖见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)。

```sh
pnpm install --frozen-lockfile
pnpm desktop dev
```

点击“浏览…”选择本地仓库目录，或直接输入路径，再选择比较模式。⌘/Ctrl O 可打开目录选择。比较两个提交时，使用“分支 ▾”选择来源与目标的本地分支、远程跟踪分支或标签，也可直接输入 commit ID / ref；引用列表支持搜索和分页，不自动拉取远程。

比较模式：

| 模式         | 基线 → 候选                             |
| ------------ | --------------------------------------- |
| 全部本地改动 | HEAD → 原始工作区文件，可纳入未跟踪文件 |
| 已暂存       | HEAD → 冻结 index                       |
| 未暂存       | 冻结 index → 原始工作区文件             |
| 两个提交     | 两个输入 ref 解析并固定后的 commit      |

默认工作区比较使用完整 index/stat 元数据筛选，按实际变化读取源码。需要逐一核对所有已跟踪文件原始字节时，勾选“完整原文核验”；这也覆盖已删除历史转换规则留下的内容，大仓库会更慢且仍受捕获时间预算限制。CLI 对应 `--verify-all`。

桌面单仓库的“两个提交”比较超过内存捕获预算时，自动使用临时磁盘冻结快照，最多 20,000 个变化路径、512 MiB 序列化内容、120 秒磁盘捕获；正文按需读取，正常退出清理，异常遗留在下次大型捕获时回收。它不是持久归档。CLI、多仓库捕获和本地改动模式仍受 2,000 路径 / 64 MiB / 20 秒预算约束，超限可填写字面路径范围后重试。实测与限制见[原生性能与扩容验收](docs/acceptance/native-performance-2026-10-29.md)。

文件捕获后保持不变；刷新创建新比较。使用 J/K 切换文件、⌘/Ctrl F 搜索、Alt+↑/↓ 跳转变化。打开含移动的文件会突出首个移动关系；连线支持起点/终点定位，选择内部修改可直接对照其双侧内容。格式折叠有明确依据，可点击展开或切回完整原始视图。已审查标记绑定快照、内容和分析规则版本。

`pnpm dev` 只预览浏览器界面；本地 Git、比较内核和验证执行由桌面应用提供。普通比较及工作区捕获不执行仓库脚本、Git filters、外部 diff、hooks 或自动拉取缺失对象。验证是独立的显式执行入口。

### 用本机 AI 解释一处变更

在桌面比较里打开一个文本文件，点击文件名旁的 **AI 解释**。默认解释第一块原始差异；如果先在“变更详情”选中了一个函数移动、内部修改等关系，就从该组变化开始。可以切换差异块，也可以明确选择整个文件。

1. 展开“查看将发送的源码”，核对路径和改前/改后行号。局部解释最多带前后各 4 行相邻代码；完整上下文上限为 48 KiB。二进制、只有预览或元数据的文件不提供解释。
2. 选择已在本机安装、登录的 **Codex CLI** 或 **Claude Code**，按需填写问题和模型，再点击“生成解释”。仅打开比较或预览不会调用模型。CLI 可能向其模型服务发送这段代码、消耗当前账户额度；Diffkin 不需要 API Key。
3. 阅读代码含义和前后行为差异，展开“核对源码”查看引用。修改动机单列为**推测**：当前上下文不含提交说明、需求或作者确认。解释不会改变已审查标记、格式折叠或验证结果。

例如 `balance > amount` 改为 `balance >= amount`，两侧源码可以说明“余额等于金额时由拒绝变为允许”；“可能是为了允许取出全部余额”则需要用需求或提交说明核实。

支持 macOS / Linux，使用本机 CLI 登录状态，在独立临时目录调用一次；Windows 当前不执行。桌面进程会搜索 `PATH`、`~/.local/bin` 和 Homebrew 常用路径，也可在启动 Diffkin 前用 `DIFFKIN_CODEX_PATH` / `DIFFKIN_CLAUDE_PATH` 指定可信 CLI 的绝对路径。需要支持下列参数的版本（本轮核对 Codex CLI 0.162.1 / Claude Code 2.1.278）：[Codex 非交互模式](https://developers.openai.com/codex/noninteractive)使用 `exec --ignore-user-config --ephemeral --sandbox read-only`，不加载个人配置的模型路由；[Claude Code CLI](https://code.claude.com/docs/en/cli-reference) 使用 `--print --safe-mode --tools ""`。本机组织策略仍适用。切换范围、关闭面板或手动取消会停止请求；模型服务已处理的请求不能撤回。

### 用选择题检查对变更的理解

在文件名旁点击 **变更测验**。默认考当前文件的全部文本改动，也可选择一个差异块。面板显示题数、增删行数和将发送的源码。点击“生成测验”，已登录的本机 Codex 或 Claude 会围绕具体输入、执行路径、边界条件和可观察的行为变化出题。

每题四选一，交卷后显示得分、每题的正确答案、解析及冻结 diff 的引用。可返回上一题修改选择，交卷后可重答原题或重新出题。重答和评分都在本地完成。新增与删除行数之和为 1–4、5–12、13–24、25–49、50–99、100 以上时，依次出 3、4、5、6、7、8 题。每个差异块最多带前后各两行相邻源码，已预览的片段合计限 48 KiB。

### 本地设置

点击“设置”调整双栏/统一视图、格式折叠和未修改区域折叠。仓库路径、最近 8 个仓库、比较模式与来源/目标也会自动保存到平台应用数据目录的 `settings.json`。下次打开只恢复设置，点击“打开比较”后才读取仓库。

旧 WebView 会话缓存会迁移；本地文件损坏或版本未知时保留原文件并显示恢复入口。只有点击“保存当前设置并替换本地文件”才会覆盖该文件。工作区及审查证据继续使用各自的持久化存储。

### 多仓库审查

1. 点击“工作区”，填写名称与本地 Git 路径；用“关联另一个仓库”添加 API、SDK、Web 等仓库，分别选择比较模式和提交。仓库标识保持稳定，修改路径即可重定位。
2. 点击“捕获工作区”。切换仓库标签查看各自变化；缺失仓库单独报错，零变化仓库保留比较身份。没有改动的文件正文不会全部归档。
3. 选择文件进行审查；展开“手工关联”，选择目标仓库/文件、填写可选备注，再保存关联。关联与审查都绑定这次冻结内容。
4. 下次从“已保存工作区”选择并“恢复工作区”，或切换“工作区历史快照”。恢复读取本机归档，不重读活动源码；重新捕获产生新快照，旧结论保留为历史。
5. 点击“导出版本清单”获得 JSON 摘要。默认不含源码、绝对路径、显示标签、关联备注或错误文本；它不替代保留源码的本机证据归档。

当前使用有界 JSON 存储，最多 16 个工作区，每个配置 8 个仓库、8 个快照；引用中的历史证据不会被普通缓存淘汰。详细限额见[当前验收记录](docs/acceptance/m3-m4.md)。

### 固定提交验证

1. 打开一个本地仓库，点击“验证”，填写候选提交/ref；需要对照时勾选“同时准备基线”并填写基线。未提交的工作区或 index 改动不会进入验证。
2. 填写明确的程序路径、JSON 字符串参数数组、相对验证副本的工作目录和超时。例如，受信任的 Python 检查可填写本机 Python 绝对路径与 `["-m", "unittest", "discover"]`；项目需要该检查本身及相应依赖。
3. 点击“创建验证计划”，核对解析后的 commit、程序/命令身份、参数、源码范围、锁文件和限制。此步骤不运行命令；表单后续改动也不改变该计划。
4. 确认计划后点击“运行候选计划”。按需运行同一计划的“运行基线计划”，比较两次结果及各自源码身份；过程中可看增量日志或“取消运行”。
5. 从验证历史查看原计划和结果。源码或程序被修改、超时、环境错误、取消及中断均有独立状态；重新运行创建新记录。

执行仅支持 **Unix 上可信本机程序**，副本不是安全沙箱：程序仍可访问主机与网络。Windows 当前不会执行。锁文件已记录，但安装依赖、动态库、完整工具链和跨仓 SDK 装配仍为 `unconfirmed`；“通过”仅代表该单仓固定提交命令成功且纳入核验的文件/程序未变，不能代表联合候选已验证。详见[执行边界](crates/validation/README.md)。

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

子模块工作区 dirty 不递归读取。超过 2 MiB 的 Git blob 只提供对象身份；工作区超大文件提供有限预览。当前验收范围和未完成事项见[M3/M4 验收记录](docs/acceptance/m3-m4.md)，旧版本证据保留在历史报告中。

多仓库联合查看、手工关联与按需 AI 变更解释已实现；自动跨仓契约推断、依赖候选联合装配、AI 风险扫描、自动 rebase 和自动更新尚未实现。macOS 原生核心流程已通过；当前仍未完成全部产品与跨平台发布门槛。

## CLI

```sh
cargo run -p diffkin-cli -- before.ts after.ts
cargo run -p diffkin-cli -- --structure before.ts after.ts
cargo run -p diffkin-cli -- --repo /path/to/repo --mode commits --base HEAD~1 --head HEAD
cargo run -p diffkin-cli -- --repo /path/to/repo --mode worktree --untracked
cargo run -p diffkin-cli -- --repo /path/to/repo --mode worktree --verify-all
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

CI 会先运行 `diffkin-analysis` 的 `ui_fixture` 和 `diffkin-desktop` 的 `workflow_fixture`，通过 `DIFFKIN_KERNEL_FIXTURE` 与 `DIFFKIN_WORKFLOW_FIXTURE` 将真实分析、持久化工作区及受控验证结果交给浏览器联测。浏览器交互测试在本机使用已安装的 Chrome，CI 使用 Playwright Chromium。它验证实际界面与模拟 IPC 的交互；原生 WebView 验收单独记录。

| 目录                 | 职责                                                    |
| -------------------- | ------------------------------------------------------- |
| `apps/desktop`       | React 界面、Tauri 桥接、有界任务和快照缓存              |
| `apps/cli`           | 文件与仓库比较，JSON 输出                               |
| `crates/domain`      | Rust DTO，生成 TypeScript 协议                          |
| `crates/application` | 只读 Git 捕获、多仓库冻结内容、审查依据与有界 JSON 存储 |
| `crates/validation`  | 固定提交装配、明确确认后的可信本机执行、日志与结果      |
| `crates/explanation` | 冻结片段、受限的本机 AI CLI 调用和解释引用校验          |
| `crates/analysis`    | 无损文本、语法关系、覆盖证明和取消预算                  |
| `fixtures/structure` | 冻结输入与明确 expected                                 |
| `docs/acceptance`    | 正确性、性能原始记录与未验收事项                        |
| `third-party`        | 第三方许可文本与可核对来源                              |
| `.github`、`scripts` | CI、版本、发版、协议与许可材料                          |

## 发版

以下版本号仅为流程示例，以实际准备发布的版本为准。

```sh
pnpm version:set 0.3.0-alpha.3
pnpm format
pnpm check
# 提交版本、源码及锁文件后
# git tag v0.3.0-alpha.3
# git push origin v0.3.0-alpha.3
```

Tag 触发版本校验、三平台 CI，以及 macOS Apple Silicon / Intel、Windows x64、Linux x64 构建。全部产物齐备后生成 `SHA256SUMS` 和 **Release 草稿**。CLI 与桌面包附带许可证和第三方材料。已公开 Release 不允许重跑覆盖。

当前安装包供 Alpha 验证，自动更新仍待配置。内部包不会发布到 npm/crates.io。macOS 发布包默认对应用和 CLI 做 ad-hoc 签名。正式分发可启用 Apple Developer ID 签名与公证，Windows 可启用 Microsoft Artifact Signing。配置步骤见 [CI 与发布说明](docs/releasing.md#签名和自动更新)。

## 设计文档

- [整体架构](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [质量与性能门槛](docs/quality.md)
- [M3/M4 当前验收记录](docs/acceptance/m3-m4.md)
- [M2 历史验收记录](docs/acceptance/m2.md)
- [核心语言能力与 alpha.2 历史验收](docs/acceptance/core-languages.md)

原始讨论：[高级代码差异工具设计](chatgpt-conversation://6ae14eed-2b28-83ec-b62e-843ae0316ce8)。
