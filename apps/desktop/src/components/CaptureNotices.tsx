// Saved snapshots use the same warning strings as the CLI. Only fold known
// informational messages so that new warnings stay visible by default.
const captureNotes = new Map([
  [
    "Working-tree bytes are captured literally; clean/smudge filters and line-ending conversion are not executed. Capture is not a filesystem-wide atomic transaction.",
    "按工作区原始字节读取文件，跳过 Git 过滤器和换行转换。捕获期间文件仍可能变化。",
  ],
  [
    "Fast capture uses Git index/stat metadata for unchanged tracked files. Changed, flagged, racy or known conversion-affected files are hashed literally. Historical conversion rules may no longer be discoverable; enable full literal verification to check all existing tracked bytes.",
    "快速捕获使用 Git 索引和文件状态识别未改动的跟踪文件。对已修改、有特殊标记、时间戳不可靠或受已知转换规则影响的文件，会读取原文计算哈希。要核对历史转换规则可能影响的文件，可开启「完整原文核验」。",
  ],
  [
    "Full literal verification: all existing tracked file bytes within the selected path scope were checked within the capture budget; absent sparse files retain index context.",
    "已逐字节核验所选范围内现存的跟踪文件。稀疏检出中缺失的文件沿用索引记录。",
  ],
]);

export function CaptureNotices({ warnings }: { warnings: string[] }) {
  const notes = warnings.flatMap((warning) => captureNotes.get(warning) ?? []);
  return (
    <>
      {warnings
        .filter((warning) => !captureNotes.has(warning))
        .map((warning, index) => (
          <p className="notice compact" key={index}>
            {warning}
          </p>
        ))}
      {notes.length > 0 && (
        <details className="capture-notes">
          <summary>快照捕获说明</summary>
          {notes.map((note) => (
            <p key={note}>{note}</p>
          ))}
        </details>
      )}
    </>
  );
}
