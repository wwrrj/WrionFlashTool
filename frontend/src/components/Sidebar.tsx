import { useState } from 'react';
import type { ReactNode } from 'react';
import { BookOpen, Check, ChevronRight, Copy, Loader2, ScrollText } from 'lucide-react';
import { WIZARD_STEPS } from '../wizard';
import type { Step } from '../wizard';

/** 左侧固定显示的工具入口。 */
export interface SidebarTool {
  key: string;
  label: string;
  /** 可用时的说明 */
  hint: string;
  icon: ReactNode;
  /** 不可用时的原因；为空表示可用 */
  unavailable?: string;
  active: boolean;
  onOpen: () => void;
}

interface SidebarProps {
  step: Step;
  /** 每个大步骤下方显示的已选内容，与 WIZARD_STEPS 一一对应。 */
  values: (string | null)[];
  /** 刷入进行中时锁定导航。 */
  locked: boolean;
  logCount: number;
  onNavigate: (step: Step) => void;
  onShowLogs: () => void;
  onShowNotice?: () => void;
  qqGroup?: string;
  /** 正在使用独立工具；此时 step 是进入工具前所在的步骤。 */
  toolActive?: boolean;
  tools: SidebarTool[];
}

export function Sidebar({ step, values, locked, logCount, onNavigate, onShowLogs, onShowNotice, qqGroup, toolActive, tools }: SidebarProps) {
  const [copied, setCopied] = useState(false);
  // 完成页上所有步骤都显示为已完成
  const current = step === 'done' ? WIZARD_STEPS.length : WIZARD_STEPS.findIndex((s) => s.id.includes(step));
  const total = WIZARD_STEPS.length;

  const copyQQ = async () => {
    if (!qqGroup) return;
    try {
      await navigator.clipboard.writeText(qqGroup);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch { setCopied(false); }
  };

  return (
    <aside className="rail">
      <div className="rail-head">
        <span>刷机进度</span>
        <b>{Math.max(current, 0)} / {total}</b>
      </div>
      <div className="rail-meter"><span style={{ width: `${(Math.max(current, 0) / total) * 100}%` }} /></div>

      <ol className="rail-steps">
        {WIZARD_STEPS.map((s, i) => {
          const done = i < current;
          // 在工具页里时，当前步骤只是“暂离”，点击即可返回
          const paused = Boolean(toolActive) && i === current;
          const active = i === current && !toolActive;
          const clickable = !locked && step !== 'done' && (done || paused);
          const state = done ? 'done' : active ? 'active' : paused ? 'paused' : 'todo';
          return (
            <li key={s.label} className={state}>
              <button className="rail-step" disabled={!clickable} onClick={() => onNavigate(s.id[0])} aria-current={active ? 'step' : undefined}>
                <span className="rail-dot">
                  {done ? <Check size={11} strokeWidth={3.2} /> : active && locked ? <Loader2 size={12} className="spin" /> : i + 1}
                </span>
                <span className="rail-text">
                  <strong>{s.label}</strong>
                  <small title={values[i] ?? undefined}>{values[i] ?? (done ? '已完成' : active ? '进行中' : paused ? '点击返回' : '未开始')}</small>
                </span>
              </button>
            </li>
          );
        })}
      </ol>

      <div className="rail-group">
        <span className="rail-label">工具</span>
        {tools.map((tool) => (
          <button
            key={tool.key}
            className={`rail-tool ${tool.active ? 'active' : ''}`}
            onClick={tool.onOpen}
            disabled={locked || Boolean(tool.unavailable)}
            title={tool.unavailable}
          >
            <span className={`rail-tool-icon ${tool.key}`}>{tool.icon}</span>
            <span className="rail-tool-text"><strong>{tool.label}</strong><small>{tool.unavailable ?? tool.hint}</small></span>
            <ChevronRight size={15} className="rail-tool-arrow" />
          </button>
        ))}
      </div>

      <div className="rail-foot">
        <div className="rail-actions">
          {onShowNotice && <button onClick={onShowNotice}><BookOpen size={14} /> 须知</button>}
          <button onClick={onShowLogs}><ScrollText size={14} /> 日志{logCount > 0 && <em>{logCount > 999 ? '999+' : logCount}</em>}</button>
        </div>
        {qqGroup && (
          <button className="rail-qq" onClick={() => void copyQQ()} title="点击复制群号">
            <span>交流群</span>
            <b className="mono">{qqGroup}</b>
            {copied ? <Check size={12} className="ok" /> : <Copy size={12} />}
          </button>
        )}
      </div>
    </aside>
  );
}
