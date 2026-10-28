import type { Extension } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { java } from "@codemirror/lang-java";
import { go } from "@codemirror/lang-go";
import { rust } from "@codemirror/lang-rust";
import { vue } from "@codemirror/lang-vue";

/** Highlighting only: structural claims always come from the Rust analysis result. */
export function languageExtensions(path: string, focusedVueScript: boolean): Extension[] {
  const normalized = path.toLowerCase();
  if (normalized.endsWith(".vue")) {
    return [focusedVueScript ? javascript({ typescript: true }) : vue()];
  }
  if (/\.(?:[cm]?[jt]s|[jt]sx)$/.test(normalized)) {
    return [
      javascript({
        typescript: /\.(?:[cm]?ts|tsx)$/.test(normalized),
        jsx: /\.[jt]sx$/.test(normalized),
      }),
    ];
  }
  if (/\.py[iw]?$/.test(normalized)) return [python()];
  if (normalized.endsWith(".java")) return [java()];
  if (normalized.endsWith(".go")) return [go()];
  if (normalized.endsWith(".rs")) return [rust()];
  return [];
}
