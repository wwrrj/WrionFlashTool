import { useEffect, useState } from 'react';
import { ArrowLeft, ArrowRight, CheckCircle2, FileArchive, FolderOpen, Sparkles, TriangleAlert } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { ToolProgress } from '../components/ToolProgress';
import { useExpandProgress } from '../hooks/useExpandProgress';
import { api, pickFile } from '../lib/api';
import { baseName, errorMessage } from '../lib/format';

interface IdPageProps {
  /** 当前选中的手机序列号；为空时自动选择唯一连接的手机。 */
  serial: string | null;
  logs: string[];
  onShowLogs: () => void;
  onLog: (line: string) => void;
  onBusyChange: (busy: boolean) => void;
  onExit: () => void;
}

type Stage = 'start' | 'running' | 'done';

/** 独立的“刷入 ID 补丁”工具：把外部 ID 包（.zip 或 mi_ext.img）刷入 mi_ext 分区。 */
export function IdPage({ serial, logs, onShowLogs, onLog, onBusyChange, onExit }: IdPageProps) {
  const [stage, setStage] = useState<Stage>('start');
  const [idPath, setIdPath] = useState('');
  const [candidates, setCandidates] = useState<string[]>([]);
  const [formatData, setFormatData] = useState(false);
  const [reboot, setReboot] = useState(true);
  const [rebooted, setRebooted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { progress, reset } = useExpandProgress();

  useEffect(() => { onBusyChange(stage === 'running'); }, [stage, onBusyChange]);
  useEffect(() => {
    api.idCandidates().then(setCandidates).catch(() => setCandidates([]));
  }, []);

  const browse = async () => {
    const picked = await pickFile('ID 补丁', ['zip', 'img']);
    if (picked) setIdPath(picked);
  };

  const start = async () => {
    setError(null);
    reset();
    setStage('running');
    onLog(`>>> 开始刷入 ID 补丁：${idPath}`);
    try {
      const outcome = await api.runIdFlash({ chosenSerial: serial, idFile: idPath, formatData, reboot });
      onLog(`[ID 补丁完成] 日志保存于 ${outcome.log_path}`);
      setRebooted(outcome.rebooted);
      setStage('done');
    } catch (e) {
      const message = errorMessage(e);
      onLog(`[失败] ${message}`);
      setError(message);
      setStage('start');
    }
  };

  if (stage === 'running') {
    return <ToolProgress title="正在刷入 ID 补丁" description="工具会自动切换到 Fastbootd 并写入 mi_ext 分区，通常不到一分钟。" progress={progress} logs={logs} onShowLogs={onShowLogs} />;
  }

  if (stage === 'done') {
    return (
      <PageFrame
        title="ID 补丁已刷入"
        description={baseName(idPath)}
        primary={<button className="btn primary large" onClick={onExit}>完成 <ArrowRight size={17} /></button>}
      >
        <section className="card result-card">
          <CheckCircle2 size={30} className="ok" />
          <div>
            <strong>mi_ext 已更新</strong>
            <p className="muted">{rebooted
              ? '手机正在重启。第一次开机可能稍慢，请耐心等待。'
              : reboot ? '手机没有自动重启，请长按电源键手动重启。' : '你关闭了自动重启，准备好后长按电源键重启手机。'}</p>
          </div>
        </section>
      </PageFrame>
    );
  }

  const hint = !idPath.trim() ? '请先选择 ID 补丁文件' : undefined;
  return (
    <PageFrame
      title="刷入 ID 补丁"
      description="把外部 ID 补丁（.zip 或 mi_ext.img）刷入手机的 mi_ext 分区。只在需要更换 ID 时使用，正常刷机不需要。"
      secondary={<button className="btn" onClick={onExit}><ArrowLeft size={15} /> 返回刷机</button>}
      hint={hint}
      primary={<button className={`btn large ${formatData ? 'danger' : 'primary'}`} disabled={Boolean(hint)} onClick={() => void start()}>
        <Sparkles size={16} /> {formatData ? '刷入并清除数据' : '开始刷入'}
      </button>}
    >
      <section className="card">
        <div className="card-title">ID 补丁文件</div>
        {candidates.length > 0 && (
          <div className="backup-list">
            {candidates.map((path) => (
              <button key={path} className={`backup-item ${idPath === path ? 'selected' : ''}`} onClick={() => setIdPath(path)}>
                <FileArchive size={18} />
                <span className="backup-text"><strong title={path}>{baseName(path)}</strong><small>在刷机包旁边找到的 ID 补丁</small></span>
              </button>
            ))}
          </div>
        )}
        <div className="dest-row">
          <input className="input mono" value={idPath} placeholder="选择 ID 补丁 .zip 或 mi_ext.img" onChange={(e) => setIdPath(e.target.value.replace(/["']/g, ''))} />
          <button className="btn" onClick={() => void browse()}><FolderOpen size={15} /> 选择</button>
        </div>
      </section>

      <section className="card">
        <label className="opt-head toggle">
          <TriangleAlert size={17} />
          <div><strong>刷入后清除用户数据</strong><small>首次刷入或更换 ID 后无法开机时使用，会清空手机上的所有数据</small></div>
          <input type="checkbox" checked={formatData} onChange={(e) => setFormatData(e.target.checked)} />
        </label>
        <label className="reboot-row">
          <input type="checkbox" checked={reboot} onChange={(e) => setReboot(e.target.checked)} />
          <span>完成后自动重启手机</span>
        </label>
      </section>

      {formatData && <div className="callout danger"><TriangleAlert size={16} /><span>已选择清除数据：照片、应用、聊天记录都会被删除且无法恢复。</span></div>}
      {error && <pre className="error-text">{error}</pre>}
    </PageFrame>
  );
}
