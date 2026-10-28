# Diffkin 开发与发布说明

当前为 M0 脚手架。公开仓库为 [yourtion/diffkin](https://github.com/yourtion/diffkin)，CI 与发版由 [GitHub Actions](https://github.com/yourtion/diffkin/actions) 执行。安装包构建成功不代表已经完成所有平台的安装、签名与用户体验验收。

## 工具链与依赖

Node 使用 `.node-version`，pnpm 使用根 `package.json` 的 `packageManager`，Rust 使用 `rust-toolchain.toml`。更新 Rust 时同时更新 `.github/actions/setup/action.yml` 中的版本。JS/Rust 依赖分别由 `pnpm-lock.yaml`、`Cargo.lock` 锁定；CI 使用 frozen/locked 安装。

Rust 的默认成员只包含内核与 CLI，`cargo test` 不要求桌面 SDK；`cargo test --workspace` 包括 Tauri，需要平台依赖及已构建的前端资源。UI 使用 Rsbuild 的 React 插件，开发地址固定为 `127.0.0.1:1420`，端口占用直接报错，避免 Tauri 连接错误的服务。

`pnpm protocol:generate` 根据 `crates/domain` 生成 TS 类型；生成结果提交到 Git。`pnpm protocol:check` 重新生成并比较，不一致即失败；它也适用于全新 checkout，无须预先编译。

## CI

`ci.yml` 在 main push、PR 和 release workflow 调用时运行，平台为 Ubuntu 22.04、macOS 和 Windows。每个平台执行：

1. 固定工具链安装与锁文件依赖安装。
2. JS/Rust 版本一致性、Oxfmt、Oxlint、TypeScript 和发版脚本测试。
3. Rust → TypeScript 协议漂移检查与 Rsbuild 生产构建。
4. Rust fmt、Clippy 和完整 workspace 测试。

Actions 引用固定到已核验的提交 SHA，Dependabot 定期提出更新。默认 token 只有读取权限；只有 release 的最后一个 job 获得 `contents: write`。PR 不使用签名 secrets，也不使用 `pull_request_target` 执行贡献者代码。

建议在仓库 branch rules 中要求三个 `Checks (...)` job 通过。branch rules 和 secrets 是仓库配置，不会因提交 YAML 自动设置；本地 `origin` 已连接公开仓库。

## 版本与发布

根 `package.json` 是应用版本入口，Cargo workspace 保存同一值，所有 Rust 包继承；Tauri 直接读取根 package 版本。内部 JS 包不单独发版。

支持 `X.Y.Z` 和 `X.Y.Z-alpha.N / beta.N / rc.N`，不允许前导零、build metadata 或任意后缀。tag 必须严格为 `v${version}`。

```sh
pnpm version:set 0.1.0-alpha.1
pnpm format
pnpm check
pnpm protocol:check
pnpm build
cargo test --workspace --locked
```

`version:set` 同时更新 root package、Cargo workspace 和 Cargo.lock 中的本地包版本；不会重新解析第三方依赖。检查并提交这些文件，然后在该提交创建并推送 tag。

Release workflow 的顺序为：校验 tag → 复用完整 CI → 四目标构建 → 收集 9 个安装/CLI 产物 → 完整性校验与 SHA256SUMS → 创建或更新草稿。手工触发也必须选择一个版本 tag，分支触发会被拒绝。

| 目标                       | 安装包        | CLI                     |
| -------------------------- | ------------- | ----------------------- |
| `aarch64-apple-darwin`     | DMG           | tar.gz                  |
| `x86_64-apple-darwin`      | DMG           | tar.gz                  |
| `x86_64-unknown-linux-gnu` | DEB、AppImage | tar.gz                  |
| `x86_64-pc-windows-msvc`   | NSIS EXE      | tar.gz 内含 diffkin.exe |

文件名包含版本和 target，防止矩阵上传时重名。工作流中间产物保留 14 天，GitHub Release 中的产物独立保存。CLI 压缩包包含可执行文件、MIT `LICENSE` 和 `THIRD-PARTY-NOTICES.txt`；桌面安装包包含同样材料及依赖清单。

本地可以检查相同的收集逻辑：

```sh
cargo build -p diffkin-cli --release --locked --target aarch64-apple-darwin
pnpm desktop build --target aarch64-apple-darwin --bundles dmg -- --locked
node scripts/release-assets.mjs collect aarch64-apple-darwin
```

`checksums` 步骤要求四目标全部产物，不能用本机单目标冒充完整 release。发布前检查各平台安装、`diffkin --version`、文本比较与 `SHA256SUMS`，编辑发布说明并手动发布草稿。

## 第三方许可材料

安装依赖后运行：

```sh
pnpm licenses:generate
```

输出位于 `target/third-party-notices`：`README.md` 列出依赖、声明许可证与随包或固定来源补齐的许可文件；`THIRD-PARTY-NOTICES.txt` 保留这些文件的原文；`inventory.json` 包含完整清单、待补材料、应用版本和两个锁文件的 SHA-256。输出不包含开发机绝对路径，依赖未变时可重复生成。

Rust 清单覆盖锁定的依赖图，包含构建/测试和其他平台的依赖；JavaScript 清单只沿桌面应用已安装的生产依赖遍历。这是一份保守的来源清单，不等同于精确的运行时 SBOM。系统 WebView、Git、系统动态库、工具链与安装器额外下载的组件不在清单内。

项目采用 MIT。缺失的上游许可文本已按精确版本/提交或同一发布工作区补齐到 `third-party/licenses`，保留来源 URL 与 SHA-256 校验；详见 [来源说明](../third-party/README.md)。生成器若遇到新依赖没有许可文本，会保留待检查清单并失败，不自动补写版权声明。CI 和桌面构建会先生成材料；Tauri 通过 `bundle.license`、`licenseFile` 和资源映射携带 MIT 与 notices，CLI 收集脚本也要求这些文件存在。

开发机若需绕开不可用的 Cargo 镜像，可在临时目录生成 `cargo metadata --locked --format-version 1 --manifest-path /path/to/diffkin/Cargo.toml > metadata.json`，再将该文件作为脚本第二个参数（桌面构建可设置 `DIFFKIN_LICENSE_METADATA` 指向此文件）：`node scripts/license-report.mjs target/third-party-notices /path/to/metadata.json`。元数据必须对应当前锁文件，不要复用旧版本清单。

## 签名和自动更新

默认允许产出未签名的测试安装包并保留草稿。macOS 正式签名/公证接入开关为仓库变量 `MACOS_SIGNING_ENABLED=true`，所需 secrets：

| Secret                       | 用途                                |
| ---------------------------- | ----------------------------------- |
| `APPLE_CERTIFICATE`          | Base64 编码的 Developer ID 证书 p12 |
| `APPLE_CERTIFICATE_PASSWORD` | p12 密码                            |
| `APPLE_SIGNING_IDENTITY`     | 签名身份                            |
| `APPLE_ID`                   | 公证所用 Apple 账号                 |
| `APPLE_PASSWORD`             | App 专用密码                        |
| `APPLE_TEAM_ID`              | 开发者团队 ID                       |

开启开关后缺失配置直接失败，不退回未签名构建。secrets 只进入 macOS 签名步骤；配置前先确认 `dev.diffkin.desktop` 是正式使用的应用标识，首次公开发布后避免随意更换。

Windows 代码签名未实现，需要确定证书或签名服务后接入。基础框架也没有启用 Tauri updater：自动更新需要独立签名密钥、公钥、更新源和版本回退策略，不能把上传安装包视为已有自动更新。

官方流程参考：[Tauri GitHub 分发](https://v2.tauri.app/distribute/pipelines/github/)、[macOS 签名](https://v2.tauri.app/distribute/sign/macos/)。

DMG 在 CI 中跳过依赖 Finder 的图标布局脚本，避免无人值守环境的 AppleEvent 超时。本机遇到同样问题时，可用 `CI=true TAURI_BUNDLER_DMG_IGNORE_CI=false pnpm desktop bundle --target aarch64-apple-darwin --bundles dmg --ci` 重新打包已有二进制；这不绕过签名，只跳过安装窗口美化。[Tauri DMG 说明](https://v2.tauri.app/distribute/dmg/)

## 失败与重跑

单个平台失败时，GitHub 上重跑失败 jobs；完整构建成功后才会执行草稿发布 job。产物上传允许覆盖同一 workflow run 中的同名中间 artifact，草稿附件允许同名替换。已经公开发布的 Release 会被拒绝覆盖，应提升版本重新发版。

不要移动已发布 tag。没有收到完整成功结果前，检查 Actions 中各平台状态，不以存在草稿或单个平台安装包作为发版完成依据。签名失败、缺依赖、校验和缺文件均应保留失败状态。

## 开发机镜像问题

如果用户级 Cargo 镜像不可用，不要提交个人机器的源配置或改写用户全局配置。可以从仓库父目录之外执行 `cargo <command> --manifest-path /path/to/diffkin/Cargo.toml ...`，并指定临时 `CARGO_HOME` 使用官方缓存；常规 CI 在干净 runner 上使用项目固定工具链即可。
