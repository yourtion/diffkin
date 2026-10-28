# Git 快照基准

原始记录见 [snapshot-benchmark.json](snapshot-benchmark.json)。本次测量使用 Mac14,3、Apple M2、16 GiB、macOS 27.2，Rust 1.97 release 构建（thin LTO / 1 codegen unit），Git 2.54.0。

| 场景              | tracked / changed | 样本 | p50       | p95       | 最大值    |
| ----------------- | ----------------- | ---- | --------- | --------- | --------- |
| 两提交快照        | 10,000 / 100      | 30   | 95.68 ms  | 100.42 ms | 102.23 ms |
| HEAD 到工作区快照 | 10,000 / 100      | 30   | 360.98 ms | 428.07 ms | 757.21 ms |

计时从 `capture` 开始，结束于冻结清单建立。每轮重新捕获，没有结果缓存；文件系统已经热缓存。测量不包含 IPC、CodeMirror 渲染或首次可见时间，也不代表冷启动。开发机同时存在其他编译活动，因此记录用于趋势判断，不能替代受控机器的发布门槛验收。

## 复现

`crates/application/examples/snapshot_bench.rs` 接受仓库路径和 `commits` / `worktree`，连续执行 30 次并输出原始样本与分位值：

```sh
cargo run --release -p diffkin-application --example snapshot_bench -- /path/to/fixture commits
cargo run --release -p diffkin-application --example snapshot_bench -- /path/to/fixture worktree
```

构造专用临时仓库：创建 `group-{n/100 向下取整}/file-{n}.ts`，`n` 从 0 到 9,999；每个文件写入 `export function value{n}(input: number) { return input > {n}; }` 和末尾换行，提交为基线。第二个提交只将前 100 个文件的 `>` 改为 `>=`。最后将这 100 个文件的比较常量从 `n` 改为 `n+1`，不提交。基线和第二个提交的 tree ID、输入公式、构建源码摘要均保存在 JSON 中，未保存开发机绝对路径。

## 捕获语义与边界

- 两提交比较只读取不可变对象；索引通过前后摘要检测变化；工作区按原始字节冻结，读取前后及清单发布前检查文件身份、时间、长度和类型。
- 不执行 clean/smudge、textconv、external diff、fsmonitor、hooks，不写 index/ref，也不拉取缺失对象。[Git 2.45 文档](https://git-scm.com/docs/git/2.45.0)记录了 `--no-lazy-fetch`，因此要求 Git 2.45+ 并显式传入该参数；不依赖老版本可能忽略的环境变量。
- `intent-to-add` 在已暂存视图中不计为空文件；未暂存视图中显示实际新增内容。通过 `ls-files --debug -z` 读取标志，遇到不认识的格式直接报错，不猜测状态。
- 限制 50,000 个列举路径、2,000 个变化路径、64 MiB 冻结内容和 20 秒捕获时间。每文件最多保留 2 MiB；较大的 Git blob 只记录对象身份和降级说明，较大的工作区文件保留截断预览。结构分析另受 200 KB / 5,000 行限制。
- 不递归捕获子模块工作区的 dirty 状态，会在清单明确提示；提交/索引中的 gitlink 变化正常显示。Windows 的可执行位继承索引，symlink 仿真产生的文件类型差异保持可见。
- 文件系统没有全局原子快照；持续变化时返回 `stale`，不把混合内容标成完成。GUI 刷新创建新快照，已有快照内容不会随源文件变化。
