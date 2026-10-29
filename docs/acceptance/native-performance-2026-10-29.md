# 原生性能基线与后续优化

日期：2026-10-29。第一阶段基线基于 `8ce3250` 的产品代码，加本次仅测试构建可用的基准入口。机器为 Mac14,3 / Apple M2 / 16 GiB，macOS 27.2 (26B123)，arm64 release，Git 2.54.0。WebView UA、像素倍率与实际内容区尺寸记录在原始 JSON。

## 方法

使用真实 Tauri IPC、React 与 CodeMirror，独立标识 `dev.diffkin.benchmark`，不启用模拟桥。`native-benchmark` Cargo feature 才编译控制命令和驱动；生产构建没有测试控制命令。基准输入是临时生成的 10k 跟踪文件 / 20 个修改文件的固定提交，原始结果保留提交 ID。磁盘和系统文件缓存为暖缓存，每轮清空应用分析缓存；缓存切换前明确预热两个目标文件。

起点为程序触发实际按钮事件，终点为内容条件满足后的两次 requestAnimationFrame，保守覆盖一次绘制机会。它包含 React/编辑器布局与实际 IPC，但不是像素传感器测出的物理显示时刻，也不包括 OS 输入设备到点击事件的延迟。每个场景 30 轮；两份未缓存文件共 60 个样本。p95 使用 nearest-rank。首次文本可以是先行冻结文本，结构完成以可审查状态为准。捕获统计截止新 manifest 和导航内容就绪。此机器仍运行用户的日常应用，属于本机基线；不冒充独占性能实验室或跨机器发布认证。

必须将窗口置前后再采样；后台窗口的 rAF 暂停不能算入应用交互时延。驱动输入阶段的绘制等待使实际采样在窗口激活后开始。通过桌面工具对窗口执行 Raise；不修改系统锁屏策略。

## 基线

| 场景                                        | 列表就绪 p95 | 首次文本 p95 | 结构就绪 p95 | 缓存切换 p95 |
| ------------------------------------------- | -----------: | -----------: | -----------: | -----------: |
| 约 37 KiB，单处条件修改                     |       145 ms |        34 ms |        67 ms |        35 ms |
| 约 187 KiB，单处条件修改（完整流程）        |       166 ms |        35 ms |        67 ms |        35 ms |
| 约 187 KiB，1000 个函数格式化且混有条件修改 |       333 ms |        51 ms |  **1099 ms** |        91 ms |

格式化场景结构就绪超过 1 s 目标，需要优化；首次文本仍能先读。不能以快速文本预览代替结构增强达标。

捕获取消：反馈 p95 25 ms，后台任务结束 p95 26 ms。分析取消：反馈 p95 38 ms，后台任务结束 p95 39 ms。每轮先确认真实后台任务仍活动，再点击取消；完成以运行时 active jobs 归零为准，包含测量 IPC/轮询开销，不只是丢弃前端结果。

原生 760×540 和 1200×800 窗口均无页面横向溢出，实际 WebView 内容区分别为 760×508、1200×738（标题栏占据高度），倍率 2。尚未覆盖其他 OS 缩放档位、60 Hz 滚动帧和应用冷启动。

## 内存

独立运行内存采样，数据不混入延迟成绩。每约 1 s 采集宿主及后代进程，以及启动后新建、退出后消失的 WebKit GPU/WebContent/Networking PID。未启动其他 WebKit 应用；所有归入的新辅助进程都随基准退出。`launchctl procinfo` 要求 root，此处采用进程生命周期归属，因此保留方法限制，不宣称内核级责任归属证明。

macOS physical footprint 合计采样峰值 **612.2 MiB**，超过 600 MiB 目标；该点宿主约 278 MiB、WebContent 约 306 MiB，其余为 GPU/Networking。空闲窗口稳定样本约 86 MiB。RSS 另列，不能与 physical footprint 混用或忽略共享页重复计数。1 s 采样可能漏掉短峰值；每进程系统峰值也保存于原始记录。连续捕获、切换和取消的内存峰值需要第二阶段继续处理。

## 复现

```sh
python3 scripts/native-benchmark-fixture.py /tmp/diffkin-native-fixture 1000 format > /tmp/diffkin-native-config.json
pnpm build
# 使用仓库指定 Rust 工具链；离线缓存/镜像路径由本机配置决定。
TAURI_CONFIG='{"productName":"Diffkin Benchmark","identifier":"dev.diffkin.benchmark"}' \
  cargo build --release -p diffkin-desktop --features tauri/custom-protocol,native-benchmark
pnpm --filter @diffkin/desktop exec tauri bundle --ci --bundles app \
  --config '{"productName":"Diffkin Benchmark","identifier":"dev.diffkin.benchmark"}'
python3 scripts/native-benchmark-run.py \
  'target/release/bundle/macos/Diffkin Benchmark.app/Contents/MacOS/diffkin-desktop' \
  /tmp/diffkin-native-config.json /tmp/diffkin-native-results.json
# 将独立 Benchmark 窗口置前；完成后自动退出。
python3 scripts/native-benchmark-summary.py /tmp/diffkin-native-results.json
```

配置可加 `cancellation: true`、`layout: true`。内存独立运行加 `--memory`，并使用 `idleMs: 10000` 留出空闲样本。计时汇总超过门槛或样本少于 30 会返回非零；未完成的运行不会当作成功。

原始记录：[`native-performance/`](results/native-performance)。不要从这些结果推导 10k 个变化文件的支持承诺；本阶段仍使用既有 2000 变化路径预算。

## 第二阶段：按测量结果优化

第一阶段提交：`2e2d703`。格式化输入、提交和采样驱动保持一致，逐项验证后再组合：

- 原生每份源码建立一次 UTF-16/行号索引，替代每个事件重新扫描源码前缀。单独改动后结构就绪 p95 从 1099 ms 降至 130 ms。
- 前端按源码预建 CRLF 坐标映射、按分析结果建立格式证明覆盖索引，保留原有证明要求；原生 LRU 返回共享的不可变分析结果，避免深度克隆。
- 最终格式化场景的列表就绪 / 首次文本 / 结构就绪 / 缓存切换 p95 分别为 169 / 49 / 119 / 77 ms。缓存切换最大值 113 ms，不能把 p95 达标描述成每次均低于 100 ms。
- 共享缓存本身没有解决内存峰值。将 Tokio 工作者从默认数量改为 2 的对照也没有改善，因此没有引入线程池配置变更。
- 相同输入的纯 Rust 捕获循环最大 RSS 约 234 MiB，说明不能将宿主增长归因于 WebView。Git blob 改为从管道逐个解码后，同样 30 次捕获的最大 RSS 降至约 26 MiB。取消、字节预算、完整性检查仍生效；没有放大已有预算。

新增回归覆盖所有 Unicode/换行边界坐标、批量格式化中真实条件编辑可见、格式证明的缺失/重复证据、共享结果所有权，以及流式空 blob/重复 blob/多字节原文。72 项浏览器回归通过。顺序运行的本机结果仍有后台噪声；不是基线与候选交替运行的独占实验室认证。

流式读取后的完整原生进程集合 physical footprint 采样峰值为 **415.6 MiB**，低于 600 MiB 峰值门槛（同样的 30 轮捕获/切换/取消流程，辅助进程退出核对通过）。此结果不代替长期稳定内存、冷启动或其他平台验证。
