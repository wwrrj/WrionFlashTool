import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { api, isTauri, onPreview } from '../lib/api';
import { errorMessage } from '../lib/format';
import { storage } from '../lib/storage';
import type { ExtractProgressEvent, PackageInfo } from '../types';

export type PackagePhase = 'idle' | 'inspecting' | 'ready' | 'extracting';

export const isZipPath = (path: string) => path.toLowerCase().endsWith('.zip');

/** 选择刷机包的状态机：zip 走 读取包信息 → 解压；已解压的目录直接启用。 */
export function useRomPackage(enabled: boolean, onActivated: (root: string) => void) {
  const [phase, setPhase] = useState<PackagePhase>('idle');
  const [info, setInfo] = useState<PackageInfo | null>(null);
  const [dest, setDest] = useState('');
  const [freeBytes, setFreeBytes] = useState<number | null>(null);
  const [progress, setProgress] = useState<ExtractProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const onActivatedRef = useRef(onActivated);
  useEffect(() => { onActivatedRef.current = onActivated; }, [onActivated]);

  const inspect = useCallback(async (zipPath: string) => {
    setError(null);
    setInfo(null);
    setProgress(null);
    setPhase('inspecting');
    try {
      const next = await api.inspectPackage(zipPath);
      setInfo(next);
      setDest(next.default_dest);
      setFreeBytes(next.free_bytes ?? null);
      setPhase('ready');
    } catch (e) {
      setError(errorMessage(e));
      setPhase('idle');
    }
  }, []);

  const changeDest = useCallback(async (next: string) => {
    setDest(next);
    setFreeBytes(await api.destFreeSpace(next).catch(() => null));
  }, []);

  /** 返回是否成功启用了刷机包。 */
  const extract = useCallback(async (): Promise<boolean> => {
    if (!info) return false;
    setError(null);
    setProgress(null);
    setPhase('extracting');
    try {
      const root = await api.extractPackage(info.zip_path, dest);
      storage.setLastPackage(info.zip_path);
      setPhase('idle');
      setInfo(null);
      onActivatedRef.current(root);
      return true;
    } catch (e) {
      setError(errorMessage(e));
      setPhase('ready');
      return false;
    }
  }, [info, dest]);

  /** 使用已解压的 ROM 目录；返回是否成功。 */
  const activateDir = useCallback(async (dir: string): Promise<boolean> => {
    setError(null);
    setInfo(null);
    setPhase('inspecting');
    try {
      const root = await api.useRomDir(dir);
      storage.setLastPackage(root);
      setPhase('idle');
      onActivatedRef.current(root);
      return true;
    } catch (e) {
      setError(errorMessage(e));
      setPhase('idle');
      return false;
    }
  }, []);

  /** 按路径类型自动选择：.zip 读取包信息，其他当作已解压目录。 */
  const open = useCallback((path: string) => (isZipPath(path) ? inspect(path).then(() => false) : activateDir(path)), [inspect, activateDir]);

  const reset = useCallback(() => { setPhase('idle'); setInfo(null); setError(null); }, []);

  useEffect(() => {
    if (!enabled) return;
    if (!isTauri()) return onPreview<ExtractProgressEvent>('extract_progress', setProgress);
    const subs = [
      listen<ExtractProgressEvent>('extract_progress', (e) => setProgress(e.payload)),
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === 'enter' || p.type === 'over') setDragging(true);
        else if (p.type === 'leave') setDragging(false);
        else if (p.type === 'drop') {
          setDragging(false);
          // 拖入的可以是 .zip，也可以是已解压的 ROM 文件夹
          const path = p.paths.find(isZipPath) ?? p.paths[0];
          if (path) void open(path);
        }
      }),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()).catch(() => undefined));
  }, [enabled, open]);

  return { phase, info, dest, freeBytes, progress, error, dragging, inspect, activateDir, open, changeDest, extract, reset, setError };
}
