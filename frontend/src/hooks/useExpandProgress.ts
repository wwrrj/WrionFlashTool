import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { isTauri, PREVIEW_EXPAND_EVENT } from '../lib/api';
import type { ExpandProgressEvent } from '../types';

/** 订阅扩容 / 恢复的实时步骤进度（后端 `expand_progress` 事件）。 */
export function useExpandProgress() {
  const [progress, setProgress] = useState<ExpandProgressEvent | null>(null);
  const reset = useCallback(() => setProgress(null), []);

  useEffect(() => {
    if (!isTauri()) {
      // 浏览器预览：api 里的演示实现会派发同名的窗口事件
      const onPreview = (e: Event) => setProgress((e as CustomEvent<ExpandProgressEvent>).detail);
      window.addEventListener(PREVIEW_EXPAND_EVENT, onPreview);
      return () => window.removeEventListener(PREVIEW_EXPAND_EVENT, onPreview);
    }
    const sub = listen<ExpandProgressEvent>('expand_progress', (e) => setProgress(e.payload));
    return () => { void sub.then((off) => off()).catch(() => undefined); };
  }, []);

  return { progress, reset };
}
