import { useEffect, useRef, useState } from 'react';
import { Check, Copy, FolderOpen, Trash2, X } from 'lucide-react';

interface LogDrawerProps {
  logs: string[];
  onClose: () => void;
  onClear: () => void;
  onOpenFolder: () => void;
}

const tone = (line: string) =>
  /失败|FAILED|error/i.test(line) ? 'error' : /成功|完成|OKAY/.test(line) ? 'success' : /阶段|STAGE|---|>>>/.test(line) ? 'stage' : '';

export function LogDrawer({ logs, onClose, onClear, onOpenFolder }: LogDrawerProps) {
  const endRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => { endRef.current?.scrollIntoView({ block: 'end' }); }, [logs]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(logs.join('\n'));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch { setCopied(false); }
  };

  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <aside className="drawer" onClick={(e) => e.stopPropagation()} aria-label="运行日志">
        <header className="drawer-head">
          <div><h2>运行日志</h2><small>{logs.length} 条 · 遇到问题时把这里复制给作者</small></div>
          <div className="drawer-actions">
            <button className="icon-btn" onClick={() => void copy()} title="复制全部">{copied ? <Check size={15} /> : <Copy size={15} />}</button>
            <button className="icon-btn" onClick={onOpenFolder} title="打开日志文件夹"><FolderOpen size={15} /></button>
            <button className="icon-btn" onClick={onClear} title="清空"><Trash2 size={15} /></button>
            <button className="icon-btn" onClick={onClose} title="关闭"><X size={16} /></button>
          </div>
        </header>
        <div className="log-window">
          {logs.length === 0
            ? <p className="log-empty">还没有日志。开始检测或刷入后，这里会实时显示。</p>
            : logs.map((line, i) => <div key={i} className={`log-line ${tone(line)}`}>{line}</div>)}
          <div ref={endRef} />
        </div>
      </aside>
    </div>
  );
}
