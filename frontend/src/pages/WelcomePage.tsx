import { useState } from 'react';
import { ArrowRight, BookOpen, ShieldCheck } from 'lucide-react';

const CHECKS = [
  '我的手机已经解锁 Bootloader',
  '手机里的重要资料（照片、聊天记录等）已经备份',
  '我下载的刷机包就是为我这台手机准备的',
];

export function WelcomePage({ onAgree, onShowNotice }: { onAgree: () => void; onShowNotice?: () => void }) {
  const [checked, setChecked] = useState<boolean[]>(CHECKS.map(() => false));
  const all = checked.every(Boolean);

  return (
    <main className="page center-page">
      <div className="welcome">
        <div className="welcome-mark"><ShieldCheck size={26} /></div>
        <h1>开始之前，请先确认</h1>
        <p className="lead">刷机会改写手机系统，操作不当可能导致数据丢失或无法开机。请逐项确认：</p>
        <div className="check-list">
          {CHECKS.map((text, i) => (
            <label key={text} className={`check-row ${checked[i] ? 'on' : ''}`}>
              <input type="checkbox" checked={checked[i]} onChange={() => setChecked((prev) => prev.map((v, j) => (j === i ? !v : v)))} />
              <span>{text}</span>
            </label>
          ))}
        </div>
        <div className="welcome-actions">
          <button className="btn primary large" disabled={!all} onClick={onAgree}>我已确认，开始 <ArrowRight size={17} /></button>
          {onShowNotice && <button className="link-btn" onClick={onShowNotice}><BookOpen size={14} /> 查看完整刷入须知</button>}
        </div>
        <small className="muted">你的选择只保存在本机，下次启动不再询问。</small>
      </div>
    </main>
  );
}
