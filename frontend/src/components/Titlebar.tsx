import { useEffect, useState } from 'react';
import { Minus, Square, X, Zap } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ask } from '@tauri-apps/plugin-dialog';
import { isTauri } from '../lib/api';

const win = () => (isTauri() ? getCurrentWindow() : null);

export function Titlebar({ busy }: { busy: boolean }) {
  // 版本号取自 tauri.conf.json，发版时不用再改这里
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (isTauri()) getVersion().then(setVersion).catch(() => setVersion(null));
  }, []);

  const close = async () => {
    if (busy && isTauri()) {
      const sure = await ask('正在刷入中，现在关闭可能导致手机无法开机。确定要关闭吗？', { title: '正在刷入', kind: 'warning', okLabel: '仍然关闭', cancelLabel: '继续刷入' });
      if (!sure) return;
    }
    void win()?.close();
  };

  return (
    <header className="titlebar">
      <div className="titlebar-drag" onMouseDown={(e) => { if (e.button === 0) void win()?.startDragging(); }}>
        <span className="brand-mark">W</span>
        <strong>Wrion</strong>
        <span className="brand-tool"><Zap size={10} strokeWidth={2.8} />FlashTool</span>
        {version && <small>v{version}</small>}
      </div>
      <div className="titlebar-actions">
        <button className="win-btn" onClick={() => void win()?.minimize()} aria-label="最小化"><Minus size={15} /></button>
        <button className="win-btn" onClick={() => void win()?.toggleMaximize()} aria-label="最大化"><Square size={11} /></button>
        <button className="win-btn close" onClick={() => void close()} aria-label="关闭"><X size={15} /></button>
      </div>
    </header>
  );
}
