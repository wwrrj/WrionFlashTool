import React, { useEffect, useRef, useState } from 'react';
import { ArrowDown, CheckCircle2, Clipboard, FolderOpen, Terminal, Trash2 } from 'lucide-react';
import type { FlashStageEvent, PartitionProgressEvent } from '../types';

interface MockupConsoleProps {
  logs: string[];
  onClearLogs: () => void;
  onOpenLogsFolder: () => void;
  currentStage: FlashStageEvent | null;
  partitionProgress: PartitionProgressEvent | null;
  isRunning: boolean;
}

const STAGES = ['安全预检', '固件擦写', '启动链', '动态分区', '数据处理', '完成重启'];

export const MockupConsole: React.FC<MockupConsoleProps> = ({
  logs,
  onClearLogs,
  onOpenLogsFolder,
  currentStage,
  partitionProgress,
  isRunning,
}) => {
  const terminalEndRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (autoScroll) terminalEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [autoScroll, logs]);

  const handleCopyLogs = async () => {
    try {
      await navigator.clipboard.writeText(logs.join('\n'));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  const activeStage = currentStage?.stage ?? 0;

  return (
    <div className="console-shell">
      <div className="console-head">
        <div className="console-heading-row">
          <div className="console-title">
            <span className={`monitor-light ${isRunning ? 'running' : ''}`} />
            <h3>{isRunning ? '任务正在执行' : '设备与刷机监控台'}</h3>
          </div>
          <span className="console-stage-label">
            {currentStage ? `STAGE ${currentStage.stage}/6` : 'IDLE READY'}
          </span>
        </div>

        <div className="stage-grid">
          {STAGES.map((stage, index) => {
            const stageNumber = index + 1;
            const complete = activeStage > stageNumber || activeStage === 6;
            const current = activeStage === stageNumber && activeStage !== 6;
            return (
              <div key={stage} className={`stage-item ${complete ? 'complete' : ''} ${current ? 'current' : ''}`}>
                <div className="stage-circle">
                  {complete ? <CheckCircle2 size={14} /> : String(stageNumber).padStart(2, '0')}
                </div>
                <div className="stage-name">{stage}</div>
              </div>
            );
          })}
        </div>
        <div className="stage-track" />

        {partitionProgress && partitionProgress.percent > 0 && partitionProgress.percent < 100 && (
          <div className="partition-progress">
            <div><strong>{partitionProgress.name} · 正在转换</strong><span>{partitionProgress.percent}%</span></div>
            <div className="progress-track"><div className="progress-fill" style={{ width: `${partitionProgress.percent}%` }} /></div>
          </div>
        )}
      </div>

      <div className="console-log-section">
        <div className="console-log-heading">
          <h4><Terminal size={14} /> 操作记录 <small>{logs.length} 条</small></h4>
          <div className="console-log-actions">
            <button onClick={() => setAutoScroll((value) => !value)} title={autoScroll ? '关闭自动滚动' : '开启自动滚动'} aria-label="切换自动滚动"><ArrowDown size={14} /></button>
            <button onClick={() => void handleCopyLogs()} title="复制日志" aria-label="复制日志"><Clipboard size={14} /></button>
            <button onClick={onOpenLogsFolder} title="打开日志目录" aria-label="打开日志目录"><FolderOpen size={14} /></button>
            <button onClick={onClearLogs} title="清空日志" aria-label="清空日志"><Trash2 size={14} /></button>
          </div>
        </div>
        {copied && <span className="sr-only" role="status">日志已复制</span>}
        <div className="log-window">
          {logs.length === 0 ? (
            <div className="log-empty">执行检测或刷机后，实时操作记录会显示在这里。</div>
          ) : (
            logs.map((line, index) => {
              const tone = line.includes('失败') || line.includes('FAILED') || line.includes('error') ? 'error' : line.includes('成功') || line.includes('OK') ? 'success' : line.includes('阶段') || line.includes('STAGE') || line.includes('---') ? 'stage' : '';
              return <div key={`${index}-${line}`} className={`log-line ${tone}`}>{line}</div>;
            })
          )}
          <div ref={terminalEndRef} />
        </div>
      </div>
    </div>
  );
};
