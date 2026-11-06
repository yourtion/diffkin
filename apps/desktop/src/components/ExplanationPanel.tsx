import { useEffect, useMemo, useRef, useState } from "react";
import type {
  AnalysisResult,
  ChangeEvent,
  ExplanationContext,
  ExplanationExcerpt,
  ExplanationProvider,
  ExplanationResult,
  PreparedExplanation,
  PreparedQuiz,
  QuizResult,
} from "@diffkin/protocol";
import { explanationClient } from "../lib/explanation-client";
import { message } from "../lib/client";
import {
  providerNames as names,
  aiSettings,
  localEndpoint,
  rememberSettings,
  sessionKey,
  setSessionKey,
} from "../lib/local-ai";
import { QuizSheet } from "./QuizSheet";
import "./explanation.css";

type Mode = "explain" | "quiz";
type Props = {
  mode: Mode;
  comparisonId: string;
  fileId: string;
  analysis: AnalysisResult;
  event: ChangeEvent | null;
  onClose: () => void;
};

export function ExplanationPanel({ mode, comparisonId, fileId, analysis, event, onClose }: Props) {
  const chunks = useMemo(() => {
    const result: { start: number; label: string }[] = [];
    for (const [i, line] of analysis.raw.lines.entries()) {
      if (line.kind !== "equal" && (i === 0 || analysis.raw.lines[i - 1].kind === "equal")) {
        result.push({
          start: i,
          label: `差异块 ${result.length + 1} · ${line.oldLine !== null ? `改前 L${line.oldLine}` : `改后 L${line.newLine}`}`,
        });
      }
    }
    return result;
  }, [analysis]);
  const [target, setTarget] = useState(
    mode === "quiz" ? "file" : event ? "event" : chunks.length ? `raw:${chunks[0].start}` : "file",
  );
  const [preferred, setPreferred] = useState<ExplanationProvider | null>(
    () => aiSettings().provider,
  );
  const title = mode === "quiz" ? "变更测验" : "AI 变更解释";
  return (
    <aside className="review-details explanation-panel" aria-label={title}>
      <div className="explanation-heading">
        <h2>{title}</h2>
        <button onClick={onClose}>{mode === "quiz" ? "关闭测验" : "关闭解释"}</button>
      </div>
      <label>
        {mode === "quiz" ? "测验范围" : "解释范围"}
        <select value={target} onChange={(e) => setTarget(e.target.value)}>
          {mode === "explain" && event && <option value="event">所选变化 · {event.title}</option>}
          {chunks.map((chunk) => (
            <option key={chunk.start} value={`raw:${chunk.start}`}>
              {chunk.label}
            </option>
          ))}
          <option value="file">整个文件的变化</option>
        </select>
      </label>
      <ExplanationForm
        key={target}
        mode={mode}
        comparisonId={comparisonId}
        fileId={fileId}
        eventId={target === "event" ? event!.id : null}
        rawStart={target.startsWith("raw:") ? Number(target.slice(4)) : null}
        preferred={preferred}
        onPreferred={setPreferred}
      />
    </aside>
  );
}

function Excerpt({ side, excerpt }: { side: string; excerpt: ExplanationExcerpt | null }) {
  return (
    <div>
      <strong>{side}</strong>
      {excerpt ? (
        <>
          <span>
            {" "}
            · L{excerpt.startLine}–{excerpt.endLine}（重点 L{excerpt.focusStart}–{excerpt.focusEnd}
            ）
          </span>
          <pre>{excerpt.text}</pre>
        </>
      ) : (
        <p>此侧没有对应的变更片段。</p>
      )}
    </div>
  );
}

function Answer({ result, context }: { result: ExplanationResult; context: ExplanationContext }) {
  const { explanation } = result;
  return (
    <section className="explanation-answer" aria-label="生成的解释">
      <p className="explanation-byline">
        {names[result.provider]} · {context.path}
      </p>
      <h3>这段代码做什么</h3>
      <p>{explanation.meaning}</p>
      <h3>改前与改后的区别</h3>
      <p>{explanation.change}</p>
      <h3>
        为什么可能这样改 <small>推测</small>
      </h3>
      <p>{explanation.rationale}</p>
      {explanation.caveats.length > 0 && (
        <>
          <h3>还需核实</h3>
          <ul>
            {explanation.caveats.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </>
      )}
      <h3>核对源码</h3>
      {explanation.references.map((ref, i) => {
        const excerpt = context[ref.side]!;
        const lines = excerpt.text
          .split(/\r\n|\r|\n/)
          .slice(ref.startLine - excerpt.startLine, ref.endLine - excerpt.startLine + 1);
        return (
          <details key={i}>
            <summary>
              {ref.side === "before" ? "改前" : "改后"} L{ref.startLine}–{ref.endLine}
            </summary>
            <pre>{lines.map((line, j) => `${ref.startLine + j}  ${line}`).join("\n")}</pre>
          </details>
        );
      })}
    </section>
  );
}

type FormProps = {
  mode: Mode;
  comparisonId: string;
  fileId: string;
  eventId: string | null;
  rawStart: number | null;
  preferred: ExplanationProvider | null;
  onPreferred: (provider: ExplanationProvider) => void;
};
function ExplanationForm({
  mode,
  comparisonId,
  fileId,
  eventId,
  rawStart,
  preferred,
  onPreferred,
}: FormProps) {
  const [prepared, setPrepared] = useState<PreparedExplanation | PreparedQuiz | null>(null);
  const [question, setQuestion] = useState("");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState(() => aiSettings().baseUrl);
  const [apiModel, setApiModel] = useState(() => aiSettings().model);
  const [key, setKey] = useState(() => sessionKey(baseUrl));
  const [result, setResult] = useState<
    ((ExplanationResult | QuizResult) & { jobId: string }) | null
  >(null);
  const [error, setError] = useState("");
  const [running, setRunning] = useState(false);
  const activeJob = useRef<string | null>(null);
  useEffect(() => {
    let current = true;
    let preparing = true;
    const id = crypto.randomUUID();
    const preview =
      mode === "quiz"
        ? explanationClient.prepareQuiz(comparisonId, fileId, rawStart, id)
        : explanationClient.prepare(comparisonId, fileId, eventId, rawStart, id);
    preview
      .then((value) => {
        if (current) setPrepared(value);
      })
      .catch((cause: unknown) => {
        if (current) setError(message(cause));
      })
      .finally(() => {
        preparing = false;
      });
    return () => {
      current = false;
      if (preparing) void explanationClient.cancel(id).catch(() => {});
      if (activeJob.current) void explanationClient.cancel(activeJob.current).catch(() => {});
      activeJob.current = null;
    };
  }, [mode, comparisonId, fileId, eventId, rawStart]);
  const provider =
    prepared?.tools.find((t) => t.provider === preferred)?.provider ??
    prepared?.tools.find((t) => t.executable)?.provider ??
    prepared?.tools.find((t) => t.provider === "openai")?.provider ??
    "codex";
  const api = provider === "openai";
  const acp = provider === "acp" || provider === "trae";
  const tool = prepared?.tools.find((t) => t.provider === provider);
  const ready = api ? localEndpoint(baseUrl) && !!apiModel.trim() : !!tool?.executable;
  async function run() {
    if (!prepared || !ready || activeJob.current) return;
    const id = crypto.randomUUID();
    activeJob.current = id;
    setRunning(true);
    setError("");
    if (mode === "explain") setResult(null);
    try {
      rememberSettings(provider, baseUrl.trim(), apiModel.trim());
      const request = {
        preparedId: prepared.id,
        provider,
        model: api ? apiModel.trim() : acp ? "" : model.trim(),
        ...(api ? { connection: { baseUrl: baseUrl.trim(), apiKey: key || null } } : {}),
      };
      const answer =
        "evidence" in prepared
          ? await explanationClient.generateQuiz(request, id)
          : await explanationClient.run({ ...request, question: question.trim() }, id);
      if (activeJob.current !== id) return;
      if (
        answer.preparedId !== prepared.id ||
        ("provider" in answer && answer.provider !== provider)
      )
        throw new Error("结果与当前选择不一致，请重试");
      setResult({ ...answer, jobId: id });
    } catch (cause) {
      if (activeJob.current === id) setError(message(cause));
    } finally {
      if (activeJob.current === id) {
        activeJob.current = null;
        setRunning(false);
      }
    }
  }
  function cancel() {
    const id = activeJob.current;
    if (id) void explanationClient.cancel(id).catch(() => {});
    activeJob.current = null;
    setRunning(false);
    setError(mode === "quiz" ? "已取消出题。" : "已取消解释。");
  }
  if (result && "quiz" in result && prepared && "evidence" in prepared) {
    return (
      <>
        <QuizSheet
          key={result.jobId}
          quiz={result.quiz}
          evidence={prepared.evidence}
          generating={running}
          onGenerate={() => void run()}
        />
        {running && <button onClick={cancel}>取消出题</button>}
        {error && <output className="notice">{error}</output>}
      </>
    );
  }
  return (
    <>
      {prepared && (
        <>
          {"evidence" in prepared && (
            <p>
              共 {prepared.questionCount} 道单选题 · 新增 {prepared.added} 行，删除{" "}
              {prepared.removed} 行
            </p>
          )}
          <details className="explanation-preview">
            <summary>查看将发送的源码</summary>
            {"context" in prepared ? (
              <>
                <p>
                  {prepared.context.path} · {prepared.context.title}
                </p>
                <Excerpt side="改前" excerpt={prepared.context.before} />
                <Excerpt side="改后" excerpt={prepared.context.after} />
              </>
            ) : (
              prepared.evidence.map((source) => (
                <div key={source.id}>
                  <strong>
                    {source.path} · 片段 {source.id}
                  </strong>
                  <pre>{source.diff}</pre>
                </div>
              ))
            )}
          </details>
          <label>
            本机 AI 工具
            <select
              disabled={running}
              value={provider}
              onChange={(e) => onPreferred(e.target.value as ExplanationProvider)}
            >
              {prepared.tools.map((t) => (
                <option key={t.provider} value={t.provider}>
                  {names[t.provider]}
                  {t.provider === "openai" || t.executable ? "" : "（未找到）"}
                </option>
              ))}
            </select>
          </label>
          {api ? (
            <>
              <label>
                API 基础地址
                <input
                  disabled={running}
                  value={baseUrl}
                  maxLength={2048}
                  onChange={(e) => {
                    setBaseUrl(e.target.value);
                    setKey("");
                    setSessionKey("", "");
                  }}
                />
              </label>
              {!localEndpoint(baseUrl) && (
                <p className="notice">请填写本机 HTTP 地址，例如 http://127.0.0.1:11434/v1。</p>
              )}
              <label>
                模型名称
                <input
                  disabled={running}
                  value={apiModel}
                  maxLength={200}
                  placeholder="服务中已加载的模型 ID"
                  onChange={(e) => setApiModel(e.target.value)}
                />
              </label>
              <label>
                API 密钥（可选）
                <input
                  disabled={running}
                  type="password"
                  autoComplete="off"
                  maxLength={4096}
                  value={key}
                  onChange={(e) => {
                    setKey(e.target.value);
                    setSessionKey(baseUrl.trim(), e.target.value);
                  }}
                />
              </label>
            </>
          ) : tool?.executable ? (
            <p className="executable-path" title={tool.executable}>
              {tool.executable}
            </p>
          ) : (
            <p className="notice">
              未找到可用的 {names[provider]}。请在 macOS / Linux 安装并登录 CLI，或用{" "}
              {`DIFFKIN_${provider.toUpperCase()}_PATH`} 指定绝对路径后重启。
            </p>
          )}
          {mode === "explain" && (
            <label>
              想了解什么？
              <textarea
                disabled={running}
                value={question}
                maxLength={2000}
                placeholder="例如：这个条件会改变哪些输入的结果？（可选）"
                onChange={(e) => setQuestion(e.target.value)}
              />
            </label>
          )}
          {!api && !acp && (
            <details>
              <summary>模型选项</summary>
              <label>
                模型名称
                <input
                  disabled={running}
                  value={model}
                  maxLength={200}
                  placeholder="留空使用 CLI 默认模型"
                  onChange={(e) => setModel(e.target.value)}
                />
              </label>
            </details>
          )}
          <p className="muted">
            {api
              ? "生成时仅向填写的本机地址发送所选内容。密钥仅保留在本次应用内存中。"
              : acp
                ? "使用工具的默认模型。请选择可信工具，它可能把所选内容发送到模型服务。"
                : "生成时，所选内容可能由此 CLI 发送到模型服务。"}
          </p>
          <div className="explanation-actions">
            <button className="primary" disabled={running || !ready} onClick={() => void run()}>
              {mode === "quiz"
                ? running
                  ? "正在出题…"
                  : "生成测验"
                : running
                  ? "正在解释…"
                  : result
                    ? "重新生成"
                    : "生成解释"}
            </button>
            {running && (
              <button onClick={cancel}>{mode === "quiz" ? "取消出题" : "取消解释"}</button>
            )}
          </div>
        </>
      )}
      {!prepared && !error && <output>正在准备冻结源码…</output>}
      {error && <output className="notice">{error}</output>}
      {result && "explanation" in result && prepared && "context" in prepared && (
        <Answer result={result} context={prepared.context} />
      )}
    </>
  );
}
