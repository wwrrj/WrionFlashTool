import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from '../lib/api';
import type { FlashStageEvent, PartitionProgressEvent } from '../types';

/** 订阅后端推送的日志、阶段和分区转换进度。 */
export function useFlashEvents() {
  const [logs, setLogs] = useState<string[]>([]);
  const [stage, setStage] = useState<FlashStageEvent | null>(null);
  const [partition, setPartition] = useState<PartitionProgressEvent | null>(null);

  const addLog = useCallback((line: string) => setLogs((prev) => [...prev, line].slice(-1000)), []);
  const resetProgress = useCallback(() => { setStage(null); setPartition(null); }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const subs = [
      listen<string>('log_line', (e) => addLog(e.payload)),
      listen<FlashStageEvent>('flash_stage', (e) => setStage(e.payload)),
      listen<PartitionProgressEvent>('partition_progress', (e) => setPartition(e.payload)),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()).catch(() => undefined));
  }, [addLog]);

  return { logs, setLogs, addLog, stage, partition, resetProgress };
}
