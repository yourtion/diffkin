import type { AppPreferences } from "@diffkin/protocol";

export function PreferencesPanel({
  preferences,
  onChange,
  status,
  failure,
  onSave,
  onClose,
  available,
}: {
  preferences: AppPreferences;
  onChange: (change: (previous: AppPreferences) => AppPreferences) => void;
  status: string;
  failure: boolean;
  onSave: () => void;
  onClose: () => void;
  available: boolean;
}) {
  return (
    <section className="preferences-panel" aria-label="本地设置">
      <div>
        <h2>本地设置</h2>
        <p>
          自动记住仓库路径、最近仓库、比较模式、来源/目标和显示偏好。重新打开应用时恢复设置，由你决定何时打开比较。
        </p>
      </div>
      <label>
        默认视图{" "}
        <select
          aria-label="默认差异视图"
          value={preferences.view.layout}
          onChange={(event) =>
            onChange((current) => ({
              ...current,
              view: { ...current.view, layout: event.target.value as "split" | "unified" },
            }))
          }
        >
          <option value="split">双栏</option>
          <option value="unified">统一</option>
        </select>
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={preferences.view.foldFormats}
          onChange={(event) =>
            onChange((current) => ({
              ...current,
              view: { ...current.view, foldFormats: event.target.checked },
            }))
          }
        />
        自动收起已证明的格式变化
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={preferences.view.collapseUnchanged}
          onChange={(event) =>
            onChange((current) => ({
              ...current,
              view: { ...current.view, collapseUnchanged: event.target.checked },
            }))
          }
        />
        折叠未修改区域
      </label>
      <button
        type="button"
        disabled={!preferences.recent.length}
        onClick={() => onChange((current) => ({ ...current, recent: [] }))}
      >
        清空最近仓库
      </button>
      <output className={failure ? "picker-error" : "preferences-status"}>{status}</output>
      {failure && available && (
        <button type="button" onClick={onSave}>
          保存当前设置并替换本地文件
        </button>
      )}
      <button type="button" onClick={onClose}>
        收起设置
      </button>
    </section>
  );
}
