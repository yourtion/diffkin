import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Comparison } from "@diffkin/protocol";
import { CodePane } from "./components/CodePane";

const before =
  "export function canWithdraw(balance: number, amount: number) {\n  return balance > amount;\n}\n";
const after =
  "export function canWithdraw(balance: number, amount: number) {\n  return balance >= amount;\n}\n";

function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return String(error);
}

export function App() {
  const [result, setResult] = useState<Comparison | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const desktop = isTauri();

  async function compare() {
    setPending(true);
    setError(null);
    setResult(null);
    try {
      const response = await invoke<Comparison>("compare_text", { request: { before, after } });
      if (response.schemaVersion !== 1) throw new Error("不支持的比较协议版本");
      setResult(response);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }

  return (
    <main>
      <header>
        <div className="brand">
          <span className="mark">↔</span>
          <div>
            <h1>
              Diffkin <span>观变</span>
            </h1>
            <p>看清代码的每一次变化</p>
          </div>
        </div>
        <span className="badge">文本比较原型</span>
      </header>
      <section className="toolbar" aria-label="比较操作">
        <div>
          <strong>条件边界变化</strong>
          <p>canWithdraw.ts · 示例内容</p>
        </div>
        <button onClick={() => void compare()} disabled={pending || !desktop}>
          {pending ? "比较中…" : "比较示例"}
        </button>
      </section>
      {!desktop && (
        <p className="notice">
          当前为浏览器界面预览。请运行 pnpm desktop dev，在桌面应用中调用本地比较内核。
        </p>
      )}
      <section className="editors" aria-label="源码对照">
        <article>
          <h2>修改前</h2>
          <CodePane value={before} label="修改前源码" />
        </article>
        <article>
          <h2>修改后</h2>
          <CodePane value={after} label="修改后源码" />
        </article>
      </section>
      <section className="result" aria-live="polite" aria-busy={pending}>
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        {result ? (
          <>
            <h2>
              原始差异{" "}
              <span className="counts">
                +{result.added} / −{result.removed}
              </span>
            </h2>
            <div className="diff-lines">
              {result.lines.map((line, index) => (
                <div className={`diff-line ${line.kind}`} key={index}>
                  <span className="line-number">{line.oldLine ?? ""}</span>
                  <span className="line-number">{line.newLine ?? ""}</span>
                  <span className="sign">
                    {line.kind === "insert" ? "+" : line.kind === "delete" ? "−" : " "}
                  </span>
                  <code>
                    {line.text.replace(/\r?\n$/, "")}
                    {!line.text.endsWith("\n") && <span className="no-newline"> ⏎ 无末尾换行</span>}
                  </code>
                </div>
              ))}
            </div>
          </>
        ) : (
          !error && <p className="empty">运行比较后，这里将显示 Rust 内核返回的原始差异。</p>
        )}
      </section>
      <footer>本地处理 · 当前仅支持文本比较 · 结构分析与仓库读取尚未接入</footer>
    </main>
  );
}
