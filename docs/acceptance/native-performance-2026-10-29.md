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

原始记录：[`native-performance/`](results/native-performance)。第一、二阶段使用既有 2000 变化路径预算，不能从这两阶段的性能结果推导 10k 变化文件的性能；第三阶段的扩容与独立验收见下文。

## 第二阶段：按测量结果优化

第一阶段提交：`2e2d703`。格式化输入、提交和采样驱动保持一致，逐项验证后再组合：

- 原生每份源码建立一次 UTF-16/行号索引，替代每个事件重新扫描源码前缀。单独改动后结构就绪 p95 从 1099 ms 降至 130 ms。
- 前端按源码预建 CRLF 坐标映射、按分析结果建立格式证明覆盖索引，保留原有证明要求；原生 LRU 返回共享的不可变分析结果，避免深度克隆。
- 最终格式化场景的列表就绪 / 首次文本 / 结构就绪 / 缓存切换 p95 分别为 169 / 49 / 119 / 77 ms。缓存切换最大值 113 ms，不能把 p95 达标描述成每次均低于 100 ms。
- 共享缓存本身没有解决内存峰值。将 Tokio 工作者从默认数量改为 2 的对照也没有改善，因此没有引入线程池配置变更。
- 相同输入的纯 Rust 捕获循环最大 RSS 约 234 MiB，说明不能将宿主增长归因于 WebView。Git blob 改为从管道逐个解码后，同样 30 次捕获的最大 RSS 降至约 26 MiB。取消、字节预算、完整性检查仍生效；没有放大已有预算。

新增回归覆盖所有 Unicode/换行边界坐标、批量格式化中真实条件编辑可见、格式证明的缺失/重复证据、共享结果所有权，以及流式空 blob/重复 blob/多字节原文。72 项浏览器回归通过。顺序运行的本机结果仍有后台噪声；不是基线与候选交替运行的独占实验室认证。

流式读取后的完整原生进程集合 physical footprint 采样峰值为 **415.6 MiB**，低于 600 MiB 峰值门槛（同样的 30 轮捕获/切换/取消流程，辅助进程退出核对通过）。此结果不代替长期稳定内存、冷启动或其他平台验证。

## 第三阶段：有界磁盘冻结快照

第二阶段提交：`8d1bba2`。桌面单仓库的固定提交比较在普通捕获超预算后，自动改用磁盘冻结存储；两端 ref 固定为 commit ID，完整元数据清单进入内存，正文分批冻结到应用缓存目录。发布成功后，预览和分析只读取已冻结内容，既不重新读取 Git，也不读取活动工作区。

- 最多 20,000 个原始变化路径；唯一重命名的两端均计入原始路径预算。每批最多 256 路径、按 blob 大小估算的 8 MiB 原文；两侧 2 MiB 单文件上限继续生效。
- 单份快照最多 512 MiB 序列化内容，磁盘捕获阶段最多 120 秒。此前普通捕获仍可能用满 20 秒；两者不是整个点击流程总计 120 秒的承诺。磁盘配额是逻辑内容字节，不含文件系统块、目录和锁文件开销。
- 原生保留当前、上一次两个快照；新捕获进行中会暂时存在第三份。失败、取消和淘汰时删除相应目录；正常退出显式释放托管快照，因为 Tauri 的直接进程退出不会执行这些对象的析构。异常退出遗留在下一次大型捕获时按目录锁回收，不删除仍被其他实例持有的目录。
- 完整目录支持搜索，首次响应仍为 200 项；只有可见导航行进入 DOM，正文按选择读取。内容长度与 SHA-256 校验失败会明确报错，不回退到实时源码。所有批次完成且核验通过后才替换旧快照。
- CLI、持久多仓工作区捕获，以及 worktree/staged/unstaged 模式仍使用 2,000 路径 / 64 MiB / 20 秒预算，可通过字面路径范围缩小输入。临时磁盘比较不是可恢复的审查归档。

回归覆盖：跨批次唯一重命名、重复对象不误配、与内存捕获相同的文件内容及比较身份、源仓库移走后的冻结读取、篡改报错、万文件分页与末页、捕获中取消的半成品清理、磁盘配额失败、20,001+ 路径拒绝、活跃目录与异常遗留目录的区分，以及退出时释放当前与上一次快照。

10,000 个真实修改文件的固定提交在原生 WebView 中验证通过：捕获到列表可用为 **5967 ms**，搜索并打开第 10,000 个文件为 **65 ms**，实际显示修改后的条件。窄/宽窗口均无页面横向溢出。该输入每文件只有一个函数，只有一次功能计时，**不是 30 轮 p95，也不代表万文件大源码的性能**；原始记录为 `giant-functional.json`。

### 最终自动化验收

- `pnpm check`：版本、格式、lint、类型检查与 7 项脚本测试通过；前端 release 构建通过。
- Rust workspace 串行测试：149 项通过，1 项规模测试单独执行；100k / 500k 索引项的独立规模测试通过。收尾增加 1 项跨实例回收锁回归，6 项应用单元测试及 3 项磁盘集成测试再次通过。
- 完整 Playwright：72 项通过，包括真实内核结果、持久工作区/验证输出、VS Code 与 React Native 固定样本逐文件审查。
- Rust fmt、workspace/all-targets Clippy（含 benchmark feature）与生成协议一致性通过；没有协议 DTO 变更。
- 不含测试入口的最终 `Diffkin Acceptance.app` 已构建并打包；二进制摘要、检查日志见 [`final-checks.json`](results/native-performance/final-checks.json)。

追加的前台内存复测被桌面控制工具报告的锁定状态阻挡，没有纳入有效结果。资源清理改由测试构建通过真实 IPC 入口启动，由原生端独立完成两次万文件捕获、末页冻结读取、报告与正常退出；它不依赖后台 WebView 回调，也不用于可见延迟测量。最终两次捕获与读取均通过，正常退出后快照目录数为 0，原始记录为 [`giant-lifecycle.json`](results/native-performance/giant-lifecycle.json)。

此轮仍没有完成冷启动、滚动帧率、长期稳定内存和跨系统/缩放组合的发布认证；本机 30 轮交互与采样峰值仅支持前文所列场景结论。
