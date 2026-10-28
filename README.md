# Diffkin · 观变

看清代码的每一次变化。

轻量、本地的代码审查客户端。当前为 **0.3.0-alpha.0 / M3–M4 功能候选**：只读 Git 与多语言结构 diff、多仓库冻结审查，以及需要明确确认的单仓固定提交验证。采用 **MIT** 许可证。

[公开仓库](https://github.com/yourtion/diffkin) · [Actions](https://github.com/yourtion/diffkin/actions) · [当前验收记录](docs/acceptance/m3-m4.md)

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

`pnpm dev` 只预览浏览器界面；本地 Git、比较内核和验证执行由桌面应用提供。普通比较及工作区捕获不执行仓库脚本、Git filters、外部 diff、hooks 或自动拉取缺失对象。验证是独立的显式执行入口。

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

多仓库联合查看与手工关联已实现；自动跨仓契约推断、依赖候选联合装配、AI、自动 rebase 和自动更新尚未实现。原生 UI 验收由用户暂缓，当前仍未完成全部产品与平台门槛。

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

CI 会先运行 `diffkin-analysis` 的 `ui_fixture` 和 `diffkin-desktop` 的 `workflow_fixture`，通过 `DIFFKIN_KERNEL_FIXTURE` 与 `DIFFKIN_WORKFLOW_FIXTURE` 将真实分析、持久化工作区及受控验证结果交给浏览器联测。浏览器交互测试在本机使用已安装的 Chrome，CI 使用 Playwright Chromium。它验证实际界面与模拟 IPC 的交互；原生 WebView 验收单独记录。

| 目录                 | 职责                                                    |
| -------------------- | ------------------------------------------------------- |
| `apps/desktop`       | React 界面、Tauri 桥接、有界任务和快照缓存              |
| `apps/cli`           | 文件与仓库比较，JSON 输出                               |
| `crates/domain`      | Rust DTO，生成 TypeScript 协议                          |
| `crates/application` | 只读 Git 捕获、多仓库冻结内容、审查依据与有界 JSON 存储 |
| `crates/validation`  | 固定提交装配、明确确认后的可信本机执行、日志与结果      |
| `crates/analysis`    | 无损文本、语法关系、覆盖证明和取消预算                  |
| `fixtures/structure` | 冻结输入与明确 expected                                 |
| `docs/acceptance`    | 正确性、性能原始记录与未验收事项                        |
| `third-party`        | 第三方许可文本与可核对来源                              |
| `.github`、`scripts` | CI、版本、发版、协议与许可材料                          |

## 发版

以下版本号仅为流程示例，以实际准备发布的版本为准。

```sh
pnpm version:set 0.3.0-alpha.1
pnpm format
pnpm check
# 提交版本、源码及锁文件后
# git tag v0.3.0-alpha.1
# git push origin v0.3.0-alpha.1
```

Tag 触发版本校验、三平台 CI，以及 macOS Apple Silicon / Intel、Windows x64、Linux x64 构建。全部产物齐备后生成 `SHA256SUMS` 和 **Release 草稿**。CLI 与桌面包附带许可证和第三方材料。已公开 Release 不允许重跑覆盖。

详见 [CI 与发布说明](docs/releasing.md)。正式签名/公证和自动更新仍待配置；当前安装包供 Alpha 验证。内部包不会发布到 npm/crates.io。

## 设计文档

- [整体架构](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [质量与性能门槛](docs/quality.md)
- [M3/M4 当前验收记录](docs/acceptance/m3-m4.md)
- [M2 历史验收记录](docs/acceptance/m2.md)
- [核心语言能力与 alpha.2 历史验收](docs/acceptance/core-languages.md)

原始讨论：[高级代码差异工具设计](chatgpt-conversation://6ae14eed-2b28-83ec-b62e-843ae0316ce8)。
