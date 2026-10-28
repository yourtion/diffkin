# Diffkin · 观变

看清代码的每一次变化。

当前处于 **M0 框架阶段**：React 界面通过 Tauri 调用 Rust 文本比较，CLI 使用同一内核。移动识别、语义降噪、Git 仓库读取和 AI 尚未实现。

## 技术栈

- React 19、TypeScript、CodeMirror 6。
- Rsbuild / Rspack、Tauri 2、Rust workspace。
- Oxlint、Oxfmt、TypeScript、Rust fmt / Clippy / tests。
- GitHub Actions 三平台 CI、四目标安装包与 CLI、校验和、Release 草稿。

Vue SFC 仍是未来支持的输入语言，应用界面已改用 React。

## 开发

安装 `.node-version` 指定的 Node、根 `package.json` 指定的 pnpm，以及 Rust/rustup。Rust 工具链由 `rust-toolchain.toml` 固定。桌面平台依赖见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)。macOS 需要 Xcode Command Line Tools。

```sh
pnpm install --frozen-lockfile
pnpm desktop dev
```

桌面中点击“比较示例”，运行真实 Rust 文本比较。`pnpm dev` 仅启动浏览器预览，不模拟原生内核。

```sh
pnpm check
pnpm protocol:check
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# CLI：0=相同，1=有差异，2=错误
cargo run -p diffkin-cli -- fixtures/correctness/condition.before.ts fixtures/correctness/condition.after.ts

# 当前平台的 release 安装包
pnpm desktop build -- --locked
```

原型限制每侧 UTF-8 文本为 200,000 字节、5,000 行。不折叠结构变化，不修改输入文件。CLI 输出带 `schemaVersion` 的 JSON。

## 项目结构

| 目录                 | 当前职责                        |
| -------------------- | ------------------------------- |
| `apps/desktop`       | React / Rsbuild 界面及 Tauri 壳 |
| `apps/cli`           | UTF-8 文件比较 CLI              |
| `crates/domain`      | 协议定义与 TypeScript 生成器    |
| `crates/analysis`    | 文本比较与正确性测试            |
| `crates/application` | 桌面/CLI 共享调用入口           |
| `packages/protocol`  | 自动生成的 TypeScript 类型      |
| `scripts`            | 版本同步、协议检查、产物校验    |
| `.github`            | CI、发版、依赖更新              |

## 发版

```sh
pnpm version:set 0.1.0-alpha.1
pnpm format
pnpm check
# 提交代码、版本和锁文件后，在对应提交打 tag
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

tag 触发版本校验、完整 CI，以及 macOS Apple Silicon / Intel、Windows x64、Linux x64 构建。全部产物齐备后生成 `SHA256SUMS` 并创建 **Release 草稿**。预发布标记根据 alpha/beta/rc 版本识别；已发布 Release 禁止重跑覆盖。

详见 [CI 与发布说明](docs/releasing.md)。未配置证书时产物未签名；自动更新、Windows 签名和正式许可证尚待配置。所有 workspace 包为 private / publish=false，不发布 npm 或 crates.io 包。

## 设计文档

- [整体架构](docs/architecture.md)
- [Roadmap 与任务状态](docs/roadmap.md)
- [质量与性能计划](docs/quality.md)
- [CI 与发布说明](docs/releasing.md)

原始讨论：[高级代码差异工具设计](chatgpt-conversation://6ae14eed-2b28-83ec-b62e-843ae0316ce8)。
