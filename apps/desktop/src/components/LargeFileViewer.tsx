import { useMemo, useState } from "react";
import type { FrozenFile } from "@diffkin/protocol";

export function needsSegments(file: FrozenFile) {
  return [file.before, file.after].some(
    (source) => source && (source.length > 200_000 || (source.match(/\n/g)?.length ?? 0) >= 50_000),
  );
}
function SourceSegments({
  source,
  label,
  truncated,
}: {
  source: string | null;
  label: string;
  truncated: boolean;
}) {
  const [page, setPage] = useState(0);
  const segments = useMemo(() => {
    const result: Array<{ from: number; to: number; line: number }> = [];
    if (!source) return result;
    let line = 1;
    for (let from = 0; from < source.length;) {
      let to = Math.min(source.length, from + 32_768);
      // Never split a UTF-16 surrogate pair, even for a pathological single line.
      if (to < source.length && /[\uD800-\uDBFF]/.test(source[to - 1])) to++;
      result.push({ from, to, line });
      for (let i = from; i < to; i++) if (source[i] === "\n") line++;
      from = to;
    }
    return result;
  }, [source]);
  const segment = segments[page];
  return (
    <section className="source-segments" aria-label={`${label}分段原文`}>
      <div className="segment-controls">
        <strong>{label}</strong>
        <button
          disabled={page === 0}
          onClick={() => setPage(page - 1)}
          aria-label={`${label}上一段`}
        >
          上一段
        </button>
        <span>
          {segments.length ? page + 1 : 0} / {segments.length} 段
        </span>
        <button
          disabled={page >= segments.length - 1}
          onClick={() => setPage(page + 1)}
          aria-label={`${label}下一段`}
        >
          下一段
        </button>
        {segment && (
          <span>
            从 L{segment.line}，字符 {segment.from + 1}–{segment.to}
          </span>
        )}
      </div>
      {truncated && (
        <p className="notice">此侧冻结原文已截断；末段不是文件结尾，无法核对完整内容。</p>
      )}
      <textarea
        readOnly
        aria-label={`${label}冻结原文`}
        spellCheck={false}
        value={
          segment
            ? source!.slice(segment.from, segment.to)
            : source === null
              ? "此侧内容不存在或不可读取"
              : "空文件"
        }
      />
    </section>
  );
}
export function LargeFileViewer({ file }: { file: FrozenFile }) {
  return (
    <div className="large-file-viewer">
      <p className="notice">
        大文件分段原文 · 两侧独立翻页，每段最多约 32K
        字符。段号不代表对应关系；此视图不进行跨段差异或结构判定。
      </p>
      <div className="segment-columns">
        <SourceSegments source={file.before} label="修改前" truncated={file.beforeTruncated} />
        <SourceSegments source={file.after} label="修改后" truncated={file.afterTruncated} />
      </div>
    </div>
  );
}
