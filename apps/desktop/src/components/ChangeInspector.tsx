import type { AnalysisResult, ChangeEvent } from "@diffkin/protocol";
import { isProvenFormat } from "../lib/presentation";

const facets: Record<string, string> = {
  move: "移动",
  rename: "改名",
  edit: "修改",
  format: "格式",
  insert: "新增",
  delete: "删除",
};
export function ChangeInspector({
  result,
  selected,
  onSelect,
  fold,
  onFoldChange,
}: {
  result: AnalysisResult;
  selected: ChangeEvent | null;
  onSelect: (event: ChangeEvent | null) => void;
  fold: boolean;
  onFoldChange: (fold: boolean) => void;
}) {
  const formatOnly = (event: ChangeEvent) => isProvenFormat(event, result);
  const hidden = result.events.filter(formatOnly);
  const visible = result.events.filter((event) => !fold || !formatOnly(event));
  return (
    <aside className="inspector" aria-label="结构变化">
      <div className="panel-heading">
        <h2>变更关系</h2>
        <span>{visible.length}</span>
      </div>
      <div className="analysis-info">
        {result.language} <span>规则 {result.ruleVersion}</span>
      </div>
      <label className="check">
        <input
          type="checkbox"
          checked={fold}
          onChange={(e) => {
            onFoldChange(e.target.checked);
            if (selected && formatOnly(selected)) onSelect(null);
          }}
        />
        收起已证明的格式变化
      </label>
      {fold && hidden.length > 0 && (
        <button className="format-summary" onClick={() => onFoldChange(false)}>
          {hidden.length} 组格式变化已收起 · 展开查看
        </button>
      )}
      <div className="event-list">
        {visible.map((event) => (
          <button
            className={`event ${selected?.id === event.id ? "selected" : ""}`}
            key={event.id}
            onClick={() => onSelect(selected?.id === event.id ? null : event)}
          >
            <span className="facets">
              {event.facets.map((facet) => (
                <span key={facet} className={`facet ${facet}`}>
                  {facets[facet] ?? facet}
                </span>
              ))}
            </span>
            <strong>{event.title}</strong>
            <span className="event-location">
              {event.before ? `L${event.before.startLine}–${event.before.endLine}` : "新增"} →{" "}
              {event.after ? `L${event.after.startLine}–${event.after.endLine}` : "删除"}
            </span>
          </button>
        ))}
        {visible.length === 0 && (
          <p className="muted">
            {hidden.length
              ? "格式细节可随时展开，原始内容完整保留。"
              : "没有可确认的结构关系，请查看原始差异。"}
          </p>
        )}
      </div>
      {selected && (
        <section className="evidence">
          <h3>判断依据</h3>
          {selected.evidence.map((evidence, index) => (
            <p key={index}>{evidence}</p>
          ))}
          <p>关联 {selected.rawEditIds.length} 处原始增删。移动和改名仍需审查，不代表行为不变。</p>
        </section>
      )}
      {result.fallbacks.length > 0 && (
        <section className="fallbacks">
          <h3>文本回退</h3>
          {result.fallbacks.map((fallback, index) => (
            <p key={index}>{fallback}</p>
          ))}
        </section>
      )}
      <details className="coverage">
        <summary>原始编辑覆盖 · {result.coverage.length} 项</summary>
        <p>
          每项原始编辑均保留来源；仅有完整证明的格式范围会收起。关闭格式收起可恢复完整原始差异。
        </p>
        <p>
          {result.coverage.filter((entry) => entry.presentation === "folded-format").length}{" "}
          项格式证明，
          {result.coverage.filter((entry) => entry.presentation === "text-fallback").length}{" "}
          项文本回退。
        </p>
      </details>
    </aside>
  );
}
