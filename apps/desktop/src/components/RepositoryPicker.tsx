import { useEffect, useLayoutEffect, useRef, useState, type Ref } from "react";
import { navigationClient } from "../lib/navigation-client";
import { message } from "../lib/client";

export function RepositoryPicker({
  value,
  onChoose,
  disabled,
  label = "选择仓库目录",
  buttonRef,
}: {
  value: string;
  onChoose: (path: string) => void;
  disabled?: boolean;
  label?: string;
  buttonRef?: Ref<HTMLButtonElement>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const current = useRef(value);
  useLayoutEffect(() => {
    current.current = value;
  }, [value]);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  async function choose() {
    if (busy || disabled) return;
    const original = value;
    setBusy(true);
    setError("");
    try {
      const path = await navigationClient.pick();
      if (mounted.current && current.current === original && path !== null) onChoose(path);
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }
  return (
    <span className="repository-picker">
      <button
        ref={buttonRef}
        type="button"
        aria-label={label}
        disabled={disabled || busy}
        onClick={() => void choose()}
      >
        {busy ? "选择中…" : "浏览…"}
      </button>
      {error && (
        <span role="alert" className="picker-error">
          {error}
        </span>
      )}
    </span>
  );
}
