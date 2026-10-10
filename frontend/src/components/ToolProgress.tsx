import { ShieldAlert } from 'lucide-react';
import { PageFrame } from './PageFrame';
import { ProgressBar } from './ProgressBar';
import { LogTail, StepTimeline } from './StepTimeline';
import type { ExpandProgressEvent } from '../types';

interface ToolProgressProps {
  title: string;
  description: string;
  progress: ExpandProgressEvent | null;
  logs: string[];
  onShowLogs: () => void;
}

/** 工具页（扩容 / 恢复 / 刷入 ID）执行中的实时进度。 */
export function ToolProgress({ title, description, progress, logs, onShowLogs }: ToolProgressProps) {
  const steps = progress?.steps ?? [];
  const current = progress?.current ?? -1;
  const percent = steps.length ? (Math.max(current, 0) / steps.length) * 100 : 0;
  const label = steps.length && current >= 0 && current < steps.length ? steps[current] : '正在连接手机…';
  return (
    <PageFrame title={title} description={description}>
      <div className="callout warn strong"><ShieldAlert size={17} /><span><b>请勿拔出数据线，也不要操作手机。</b>手机进入 Fastboot、Recovery 或重启都是正常现象。</span></div>
      <section className="card">
        <ProgressBar percent={Math.max(percent, 3)} label={label} detail={`${Math.min(Math.max(current, 0) + 1, steps.length || 1)} / ${steps.length || '—'}`} />
        {steps.length > 0 && <StepTimeline steps={steps.map((name) => ({ name }))} current={current} detail={progress?.detail} />}
      </section>
      <LogTail logs={logs} onShowLogs={onShowLogs} />
    </PageFrame>
  );
}
