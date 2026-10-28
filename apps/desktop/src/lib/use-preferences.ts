import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { AppPreferences } from "@diffkin/protocol";
import {
  acknowledgePreferences,
  cachePreferences,
  loadPendingPreferences,
  loadPreferences,
  markPendingPreferences,
  parsePreferences,
} from "./preferences";
import { message } from "./client";

export function usePreferences(available: boolean) {
  const [preferences, setPreferences] = useState(loadPreferences);
  const [writable, setWritable] = useState(false);
  const [status, setStatus] = useState(available ? "正在读取本地设置…" : "偏好保存在此浏览器中");
  const [failure, setFailure] = useState(false);
  const changed = useRef(false);
  const sequence = useRef(0);
  const writes = useRef(Promise.resolve());
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    let active = true;
    if (available)
      void invoke<unknown>("preferences_load")
        .then((value) => {
          if (!active) return;
          const stored = value === null ? null : parsePreferences(value);
          if (value !== null && !stored) throw new Error("本地设置格式或版本不受支持");
          if (!changed.current) {
            const pending = loadPendingPreferences();
            if (pending ?? stored) setPreferences((pending ?? stored)!);
          }
          setWritable(true);
          setFailure(false);
          setStatus("本地设置已载入");
        })
        .catch((cause) => {
          if (!active) return;
          setFailure(true);
          setStatus(`本地设置未载入，已保留原文件：${message(cause)}`);
        });
    return () => {
      active = false;
      mounted.current = false;
    };
  }, [available]);
  useEffect(() => {
    cachePreferences(preferences);
    if (available && changed.current) markPendingPreferences(preferences);
  }, [preferences, available]);
  useEffect(() => {
    if (!available || !writable) return;
    const ticket = ++sequence.current;
    const timer = setTimeout(() => {
      setStatus("正在保存本地设置…");
      writes.current = writes.current
        .catch(() => {})
        .then(async () => {
          if (ticket !== sequence.current || !mounted.current) return;
          await invoke("preferences_save", { preferences });
          acknowledgePreferences(preferences);
          if (ticket === sequence.current && mounted.current) {
            setFailure(false);
            setStatus("已自动保存到本机");
          }
        })
        .catch((cause) => {
          if (ticket === sequence.current && mounted.current) {
            setFailure(true);
            setWritable(false);
            setStatus(`本地设置未保存：${message(cause)}`);
          }
        });
    }, 300);
    return () => {
      clearTimeout(timer);
    };
  }, [preferences, available, writable]);
  function update(change: (previous: AppPreferences) => AppPreferences) {
    changed.current = true;
    if (available && !failure) setStatus("等待保存本地设置…");
    setPreferences(change);
  }
  function saveExplicitly() {
    setWritable(true);
    update((current) => ({ ...current }));
  }
  return { preferences, update, status, failure, saveExplicitly };
}
