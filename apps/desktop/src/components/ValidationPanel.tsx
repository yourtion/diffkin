import { useEffect, useId, useRef, useState } from "react";
import type {
  ComparisonManifest,
  ValidationLog,
  ValidationPlan,
  ValidationResult,
} from "@diffkin/protocol";
import { message } from "../lib/client";
import { appendLogs, parseArguments, validationClient } from "../lib/validation-client";
import "./validation.css";

const states: Record<string, string> = {
  passed: "通过 · 单仓库固定版本",
  failed: "失败",
  "not-run": "未运行",
  indeterminate: "结果不确定",
  cancelled: "已取消",
  interrupted: "已中断",
  "env-error": "环境错误",
  running: "运行中",
};
let serial = 0;
function nextJob() {
  return `validation-${Date.now()}-${++serial}`;
}
function status(result: ValidationResult) {
  return result.sourceChanged
    ? "结果不确定 · 验证改写了源码"
    : (states[result.status] ?? result.status);
}
function date(value: number) {
  return new Date(value).toLocaleString();
}

export function ValidationPanel({
  repository,
  manifest,
}: {
  repository: string;
  manifest: ComparisonManifest | null;
}) {
  const argumentsId = useId();
  const [candidateRef, setCandidateRef] = useState("HEAD");
  const [includeBaseline, setIncludeBaseline] = useState(false);
  const [baselineRef, setBaselineRef] = useState("HEAD~1");
  const [program, setProgram] = useState("");
  const [args, setArgs] = useState("[]");
  const [workingDirectory, setWorkingDirectory] = useState(".");
  const [timeoutSeconds, setTimeoutSeconds] = useState(120);
  const [plan, setPlan] = useState<ValidationPlan | null>(null);
  const [result, setResult] = useState<ValidationResult | null>(null);
  const [history, setHistory] = useState<ValidationResult[]>([]);
  const [logs, setLogs] = useState<ValidationLog[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"prepare" | "run" | "history" | null>(null);
  const [target, setTarget] = useState<"candidate" | "baseline">("candidate");
  const [cancelRequested, setCancelRequested] = useState(false);
  const job = useRef<string | null>(null);
  const generation = useRef(0);
  const mounted = useRef(true);

  async function refreshHistory() {
    try {
      const items = await validationClient.history();
      if (mounted.current) {
        setHistory(items);
        setHistoryError(null);
      }
    } catch (e) {
      if (mounted.current) setHistoryError(message(e));
    }
  }
  useEffect(() => {
    mounted.current = true;
    let active = true;
    void validationClient.history().then(
      (items) => {
        if (active) setHistory(items);
      },
      (e: unknown) => {
        if (active) setHistoryError(message(e));
      },
    );
    return () => {
      active = false;
      mounted.current = false;
    };
  }, []);

  async function prepare() {
    setError(null);
    try {
      const parsed = parseArguments(args);
      if (!Number.isInteger(timeoutSeconds) || timeoutSeconds < 1) {
        throw new Error("超时必须是正整数秒数。");
      }
      const current = ++generation.current;
      const jobId = nextJob();
      job.current = jobId;
      setBusy("prepare");
      setCancelRequested(false);
      setPlan(null);
      setResult(null);
      setLogs([]);
      setTruncated(false);
      setLogError(null);
      try {
        const prepared = await validationClient.prepare(
          {
            repository,
            candidateRef: candidateRef.trim(),
            baselineRef: includeBaseline ? baselineRef.trim() : null,
            program: program.trim(),
            args: parsed,
            workingDirectory: workingDirectory.trim() || ".",
            timeoutSeconds,
          },
          jobId,
        );
        if (mounted.current && generation.current === current) setPlan(prepared);
      } finally {
        if (mounted.current && generation.current === current) {
          setBusy(null);
          job.current = null;
        }
      }
    } catch (e) {
      if (mounted.current) setError(message(e));
    }
  }

  async function run(runTarget: "candidate" | "baseline") {
    if (!plan?.executable || busy) return;
    const current = ++generation.current;
    const jobId = nextJob();
    job.current = jobId;
    setBusy("run");
    setTarget(runTarget);
    setResult(null);
    setLogs([]);
    setTruncated(false);
    setError(null);
    setLogError(null);
    setCancelRequested(false);
    let finished = false;
    let cursor = 0;
    let visibleLogs: ValidationLog[] = [];
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const page = await validationClient.logs(jobId, cursor);
        if (finished || !mounted.current || generation.current !== current) return;
        cursor = page.nextSequence;
        const bounded = appendLogs(visibleLogs, page.entries);
        visibleLogs = bounded.entries;
        if (bounded.truncated || page.truncated) setTruncated(true);
        setLogs(bounded.entries);
        setLogError(null);
      } catch (e) {
        if (!finished && mounted.current && generation.current === current) {
          setLogError(`增量日志暂不可用：${message(e)}`);
        }
      }
      if (!finished && mounted.current && generation.current === current) {
        timer = setTimeout(() => void poll(), 500);
      }
    };
    timer = setTimeout(() => void poll(), 500);
    try {
      const completed = await validationClient.run(plan.id, runTarget, jobId);
      if (mounted.current && generation.current === current) {
        setResult(completed);
        const bounded = appendLogs([], completed.logTail);
        setLogs(bounded.entries);
        setTruncated(completed.logTruncated || bounded.truncated);
        setLogError(null);
        void refreshHistory();
      }
    } catch (e) {
      if (mounted.current && generation.current === current) setError(message(e));
    } finally {
      finished = true;
      clearTimeout(timer);
      if (mounted.current && generation.current === current) {
        setBusy(null);
        job.current = null;
      }
    }
  }

  async function cancel() {
    if (!job.current) return;
    setCancelRequested(true);
    try {
      await validationClient.cancel(job.current);
    } catch (e) {
      if (mounted.current) {
        setCancelRequested(false);
        setError(`取消请求失败：${message(e)}`);
      }
    }
  }

  async function showHistory(item: ValidationResult) {
    if (busy) return;
    const current = ++generation.current;
    setBusy("history");
    setError(null);
    try {
      const [savedPlan, savedResult] = await Promise.all([
        validationClient.plan(item.planId),
        validationClient.result(item.id),
      ]);
      if (mounted.current && generation.current === current) {
        setPlan(savedPlan);
        setResult(savedResult);
        setTarget(savedResult.target);
        const bounded = appendLogs([], savedResult.logTail);
        setLogs(bounded.entries);
        setTruncated(savedResult.logTruncated || bounded.truncated);
        setLogError(null);
      }
    } catch (e) {
      if (mounted.current && generation.current === current) setError(message(e));
    } finally {
      if (mounted.current && generation.current === current) setBusy(null);
    }
  }

  return (
    <section className="validation-panel" aria-label="固定版本验证">
      <header>
        <div>
          <h2>固定版本验证</h2>
          <p>创建可核对的执行计划，再选择运行候选或基线。修改表单不会改写已创建的计划。</p>
        </div>
      </header>
      <form
        className="validation-form"
        onSubmit={(event) => {
          event.preventDefault();
          void prepare();
        }}
      >
        <p>
          当前仓库：<code>{repository || "尚未选择仓库"}</code>
        </p>
        {manifest && (
          <p>
            当前比较：
            <code>
              {manifest.baseLabel} → {manifest.headLabel}
            </code>
            ；本地未提交改动不包含在提交验证中。
          </p>
        )}
        <div className="validation-fields">
          <label>
            候选提交或引用
            <input
              value={candidateRef}
              onChange={(e) => setCandidateRef(e.target.value)}
              required
            />
          </label>
          <label>
            程序路径或名称
            <input
              value={program}
              onChange={(e) => setProgram(e.target.value)}
              placeholder="例如 /usr/bin/python3"
              required
            />
          </label>
          <label>
            工作目录（相对验证副本）
            <input value={workingDirectory} onChange={(e) => setWorkingDirectory(e.target.value)} />
          </label>
          <label>
            超时（秒）
            <input
              type="number"
              min={1}
              step={1}
              value={timeoutSeconds}
              onChange={(e) => setTimeoutSeconds(Number(e.target.value))}
              required
            />
          </label>
        </div>
        <div className="validation-arguments">
          <label htmlFor={argumentsId}>参数（JSON 字符串数组）</label>
          <textarea
            id={argumentsId}
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            spellCheck={false}
          />
        </div>
        <label className="validation-check">
          <input
            type="checkbox"
            checked={includeBaseline}
            onChange={(e) => setIncludeBaseline(e.target.checked)}
          />
          同时准备基线
        </label>
        {includeBaseline && (
          <label>
            基线提交或引用
            <input value={baselineRef} onChange={(e) => setBaselineRef(e.target.value)} required />
          </label>
        )}
        <div className="validation-actions">
          <button type="submit" disabled={!!busy || !repository}>
            {busy === "prepare" ? "正在固定版本…" : "创建验证计划"}
          </button>
          {busy === "prepare" && (
            <button type="button" disabled={cancelRequested} onClick={() => void cancel()}>
              {cancelRequested ? "等待准备取消…" : "取消准备"}
            </button>
          )}
        </div>
      </form>
      {error && (
        <p role="alert" className="validation-error">
          {error}
        </p>
      )}
      {plan && (
        <section className="validation-plan" aria-label="已创建的验证计划">
          <h3>已创建的计划 · {plan.id}</h3>
          <dl>
            <dt>绑定仓库</dt>
            <dd>
              <code>{plan.repository}</code>
            </dd>
            <dt>候选提交</dt>
            <dd>
              <code>{plan.candidate.commit}</code>
            </dd>
            {plan.baseline && (
              <>
                <dt>基线提交</dt>
                <dd>
                  <code>{plan.baseline.commit}</code>
                </dd>
              </>
            )}
            <dt>运行程序</dt>
            <dd>
              <code>{plan.program}</code>
            </dd>
            <dt>参数数组</dt>
            <dd>
              <code>{JSON.stringify(plan.args)}</code>
            </dd>
            <dt>执行范围</dt>
            <dd>
              单仓库完整提交副本；工作目录 <code>{plan.workingDirectory}</code>
            </dd>
            <dt>超时</dt>
            <dd>{plan.timeoutSeconds} 秒</dd>
            <dt>依赖状态</dt>
            <dd>{plan.dependencyStatus}</dd>
          </dl>
          <details>
            <summary>查看版本、工具与依赖身份</summary>
            <dl>
              <dt>命令身份</dt>
              <dd>
                <code>{plan.commandId}</code>
              </dd>
              <dt>程序身份</dt>
              <dd>
                <code>{plan.programId}</code>
              </dd>
              <dt>候选内容</dt>
              <dd>
                <code>{plan.candidate.contentId}</code> · {plan.candidate.fileCount} 文件
              </dd>
              <dt>候选树</dt>
              <dd>
                <code>{plan.candidate.tree}</code>
              </dd>
              <dt>候选锁文件</dt>
              <dd>
                {plan.candidate.locks.length
                  ? plan.candidate.locks.map((lock) => (
                      <p key={lock.path}>
                        <code>
                          {lock.path}: {lock.digest}
                        </code>
                      </p>
                    ))
                  : "无锁文件身份"}
              </dd>
              {plan.baseline && (
                <>
                  <dt>基线内容</dt>
                  <dd>
                    <code>{plan.baseline.contentId}</code>
                  </dd>
                  <dt>基线锁文件</dt>
                  <dd>
                    {plan.baseline.locks.length
                      ? plan.baseline.locks.map((lock) => (
                          <p key={lock.path}>
                            <code>
                              {lock.path}: {lock.digest}
                            </code>
                          </p>
                        ))
                      : "无锁文件身份"}
                  </dd>
                </>
              )}
            </dl>
          </details>
          {plan.blockers.map((text, index) => (
            <p role="alert" className="validation-error" key={index}>
              {text}
            </p>
          ))}
          {plan.limitations.map((text, index) => (
            <p className="muted" key={index}>
              {text}
            </p>
          ))}
          <p className="validation-warning">
            点击运行会执行上方程序与参数。进程使用计划声明的环境，仍可访问本机网络及用户可读文件；验证副本不构成安全沙箱。
          </p>
          <div className="validation-actions">
            <button disabled={!plan.executable || !!busy} onClick={() => void run("candidate")}>
              运行候选计划
            </button>
            {plan.baseline && (
              <button disabled={!plan.executable || !!busy} onClick={() => void run("baseline")}>
                运行基线计划
              </button>
            )}
          </div>
        </section>
      )}
      {(busy === "run" || result || logs.length > 0) && (
        <section className="validation-run" aria-label="验证执行结果">
          <div className="validation-run-heading">
            <h3>{target === "candidate" ? "候选" : "基线"}执行</h3>
            <output className="validation-status">
              {busy === "run"
                ? cancelRequested
                  ? "正在取消，等待进程结束"
                  : "运行中"
                : result
                  ? status(result)
                  : "执行结果未知"}
            </output>
          </div>
          {busy === "run" && (
            <button disabled={cancelRequested} onClick={() => void cancel()}>
              取消运行
            </button>
          )}
          {result && (
            <>
              <p>{result.message}</p>
              <dl>
                <dt>固定源码身份</dt>
                <dd>
                  <code>{result.sourceId}</code>
                </dd>
                <dt>程序身份</dt>
                <dd>
                  <code>{result.programId}</code>
                </dd>
                <dt>命令身份</dt>
                <dd>
                  <code>{result.commandId}</code>
                </dd>
                <dt>依赖状态</dt>
                <dd>{result.dependencyStatus}</dd>
                <dt>退出码</dt>
                <dd>{result.exitCode ?? "无"}</dd>
                <dt>执行时间</dt>
                <dd>
                  {date(result.startedAtMs)} → {date(result.finishedAtMs)}
                </dd>
              </dl>
              {result.sourceChanged && (
                <p role="alert" className="validation-warning">
                  验证过程改写了源码，不能认定原始版本通过。{result.changedPaths.join("、")}
                </p>
              )}
            </>
          )}
          <p>结果仅绑定上方计划的单仓库版本；跨仓库依赖未经联合验证确认。</p>
          {logError && <p className="validation-error">{logError}</p>}
          {truncated && <p>日志已截断，仅显示有界尾部。</p>}
          <pre className="validation-logs" aria-label="验证日志">
            {logs.length
              ? logs.map((entry) => `[${entry.stream}] ${entry.text}`).join("")
              : "暂无日志"}
          </pre>
        </section>
      )}
      <section className="validation-history" aria-label="验证历史">
        <div className="validation-run-heading">
          <h3>验证历史 · 所有仓库</h3>
          <button onClick={() => void refreshHistory()}>刷新历史</button>
        </div>
        <p>历史记录绑定执行时的版本与工具身份，不会转移为当前比较的通过状态。</p>
        {historyError && (
          <p role="alert" className="validation-error">
            {historyError}
          </p>
        )}
        <div className="validation-history-list">
          {history.map((item) => (
            <button
              key={item.id}
              disabled={!!busy}
              aria-pressed={result?.id === item.id}
              onClick={() => void showHistory(item)}
            >
              <strong>
                {item.target === "candidate" ? "候选" : "基线"} · {status(item)}
              </strong>
              <span>
                {date(item.startedAtMs)} · 计划 {item.planId}
              </span>
              <code>源码 {item.sourceId}</code>
            </button>
          ))}
        </div>
        {history.length === 0 && !historyError && <p className="muted">暂无持久化验证记录。</p>}
      </section>
    </section>
  );
}
