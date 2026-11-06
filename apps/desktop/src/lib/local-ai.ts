import type { ExplanationProvider } from "@diffkin/protocol";

export const providerNames: Record<ExplanationProvider, string> = {
  codex: "Codex",
  claude: "Claude Code",
  openai: "本机 OpenAI 兼容 API",
  pi: "Pi",
  trae: "Trae CLI",
  acp: "自定义 ACP 工具",
};

const storageKey = "diffkin.ai.v1";
const defaults = { baseUrl: "http://127.0.0.1:11434/v1", model: "" };
let credential = { baseUrl: "", key: "" };

export function localEndpoint(value: string) {
  try {
    const url = new URL(value);
    return (
      url.protocol === "http:" &&
      !url.username &&
      !url.password &&
      !url.search &&
      !url.hash &&
      (url.hostname === "localhost" ||
        url.hostname === "[::1]" ||
        /^127\.\d+\.\d+\.\d+$/.test(url.hostname))
    );
  } catch {
    return false;
  }
}

export function aiSettings(): {
  provider: ExplanationProvider | null;
  baseUrl: string;
  model: string;
} {
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? "{}");
    return {
      provider: Object.hasOwn(providerNames, saved.provider) ? saved.provider : null,
      baseUrl:
        typeof saved.baseUrl === "string" && localEndpoint(saved.baseUrl)
          ? saved.baseUrl
          : defaults.baseUrl,
      model: typeof saved.model === "string" ? saved.model : "",
    };
  } catch {
    return { provider: null, ...defaults };
  }
}

export function rememberSettings(provider: ExplanationProvider, baseUrl: string, model: string) {
  const settings = aiSettings();
  try {
    localStorage.setItem(
      storageKey,
      JSON.stringify({
        ...settings,
        provider,
        ...(provider === "openai" && localEndpoint(baseUrl) ? { baseUrl, model } : {}),
      }),
    );
  } catch {
    /* The current request also works when storage is unavailable. */
  }
}

export function sessionKey(baseUrl: string) {
  return credential.baseUrl === baseUrl ? credential.key : "";
}
export function setSessionKey(baseUrl: string, key: string) {
  credential = { baseUrl, key };
}
