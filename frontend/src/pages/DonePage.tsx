import { CheckCircle2, FolderOpen, RotateCw, ScrollText } from 'lucide-react';

interface DonePageProps {
  reboot: boolean;
  onAgain: () => void;
  onShowLogs: () => void;
  onOpenLogsFolder: () => void;
}

export function DonePage({ reboot, onAgain, onShowLogs, onOpenLogsFolder }: DonePageProps) {
  return (
    <main className="page center-page">
      <div className="welcome">
        <div className="welcome-mark success"><CheckCircle2 size={32} /></div>
        <h1>刷入完成</h1>
        <p className="lead">{reboot
          ? '手机正在自动重启。第一次开机需要几分钟，停在开机画面是正常的，请耐心等待，期间不要拔线。'
          : '刷入已完成。请长按电源键重启手机；第一次开机需要几分钟，请耐心等待。'}</p>
        <div className="welcome-actions">
          <button className="btn large" onClick={onShowLogs}><ScrollText size={16} /> 查看日志</button>
          <button className="btn large" onClick={onOpenLogsFolder}><FolderOpen size={16} /> 打开日志文件夹</button>
          <button className="btn primary large" onClick={onAgain}><RotateCw size={16} /> 再刷一台</button>
        </div>
      </div>
    </main>
  );
}
