import { Check, Loader2, ScrollText } from 'lucide-react';

interface StepTimelineProps {
  steps: { name: string; hint?: string }[];
  /** 当前步骤序号（从 0 开始）；-1 表示尚未开始，>= steps.length 表示全部完成。 */
  current: number;
  /** 当前步骤下方的实时说明，优先于 hint。 */
  detail?: string | null;
}

/** 竖向步骤时间线：已完成打勾、当前步骤转圈。刷入和扩容共用。 */
export function StepTimeline({ steps, current, detail }: StepTimelineProps) {
  return (
    <ol className="timeline">
      {steps.map((s, i) => {
        const complete = i < current;
        const active = i === current;
        return (
          <li key={s.name} className={`${complete ? 'done' : ''} ${active ? 'active' : ''}`}>
            <span className="tl-dot">{complete ? <Check size={12} strokeWidth={3} /> : active ? <Loader2 size={12} className="spin" /> : i + 1}</span>
            <span className="tl-name">{s.name}</span>
            <small>{active ? detail || s.hint || '进行中' : complete ? '已完成' : ''}</small>
          </li>
        );
      })}
    </ol>
  );
}

/** 最近几行输出 + 查看完整日志入口。 */
export function LogTail({ logs, onShowLogs }: { logs: string[]; onShowLogs: () => void }) {
  return (
    <div className="tail">
      <div className="tail-head"><span>最近输出</span><button className="link-btn" onClick={onShowLogs}><ScrollText size={13} /> 查看完整日志</button></div>
      {logs.slice(-3).map((line, i) => <p key={i} className="mono">{line}</p>)}
      {logs.length === 0 && <p className="muted">等待输出…</p>}
    </div>
  );
}
