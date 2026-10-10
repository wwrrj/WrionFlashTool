import { useEffect, useState } from 'react';
import {
  ArrowLeft, ArrowRight, CheckCircle2, FileArchive, Folder, FolderOpen, HardDrive, RotateCcw, ShieldAlert, TriangleAlert,
} from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { ProgressBar } from '../components/ProgressBar';
import { LogTail, StepTimeline } from '../components/StepTimeline';
import { useExpandProgress } from '../hooks/useExpandProgress';
import { api, pickFile, pickFolder } from '../lib/api';
import { baseName, errorMessage, GIB } from '../lib/format';
import type { BackupInfo, ExpandPlan, ExpansionConfig } from '../types';

interface ExpandPageProps {
  config: ExpansionConfig;
  logs: string[];
  onShowLogs: () => void;
  onLog: (line: string) => void;
  /** 扩容 / 恢复执行中，用于锁定导航和关闭窗口确认。 */
  onBusyChange: (busy: boolean) => void;
  /** 返回刷机流程。expanded 为 true 表示刚完成扩容，之后必须完整线刷。 */
  onExit: (expanded: boolean) => void;
}

type Tab = 'expand' | 'restore';
/**
 * 扩容：start → reading（读取方案）→ confirm → running → done
 * 恢复：start（选择备份）→ confirm → running → done
 */
type Stage = 'start' | 'reading' | 'confirm' | 'running' | 'done';

const gib = (bytes?: number | null) => (bytes ? `${(bytes / GIB).toFixed(2)} GiB` : '—');

export function ExpandPage({ config, logs, onShowLogs, onLog, onBusyChange, onExit }: ExpandPageProps) {
  const [tab, setTab] = useState<Tab>('expand');
  const [stage, setStage] = useState<Stage>('start');
  const [error, setError] = useState<string | null>(null);
  const [word, setWord] = useState('');
  const [plan, setPlan] = useState<ExpandPlan | null>(null);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [selected, setSelected] = useState<BackupInfo | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [result, setResult] = useState('');
  const { progress, reset: resetProgress } = useExpandProgress();

  const busy = stage === 'reading' || stage === 'running';
  useEffect(() => { onBusyChange(busy); }, [busy, onBusyChange]);

  const expandWord = config.confirm_word || 'EXPAND';
  const restoreWord = config.restore_confirm_word || 'RESTORE';

  const switchTab = (next: Tab) => {
    setTab(next);
    setStage('start');
    setWord('');
    setError(null);
  };

  useEffect(() => {
    if (tab !== 'restore') return;
    api.listBackups()
      .then((list) => {
        setBackups(list);
        setSelected((prev) => prev ?? list.find((b) => b.supported) ?? null);
      })
      .catch(() => setBackups([]));
  }, [tab]);

  /** 执行一个长任务：期间显示实时进度，失败时回到 fallback 阶段并显示错误。 */
  const runTask = async (running: Stage, fallback: Stage, task: () => Promise<void>) => {
    setError(null);
    resetProgress();
    setStage(running);
    try {
      await task();
    } catch (e) {
      const message = errorMessage(e);
      onLog(`[失败] ${message}`);
      setError(message);
      setStage(fallback);
    }
  };

  const readPlan = () => runTask('reading', 'start', async () => {
    setPlan(await api.expandPlan());
    setWord('');
    setStage('confirm');
  });

  const applyExpand = () => runTask('running', 'confirm', async () => {
    const backupPath = await api.applyExpand(word);
    onLog(`[扩容成功] GPT 备份：${backupPath}`);
    setResult(backupPath);
    setStage('done');
  });

  const restore = () => runTask('running', 'confirm', async () => {
    if (!selected) return;
    const message = await api.restoreBackup(selected.path, word);
    onLog(`[分区表还原成功] ${message}`);
    setResult(message);
    setStage('done');
  });

  /** 用户手动选择的备份：检查后加入列表并选中。 */
  const addBackup = async (path: string | null) => {
    if (!path) return;
    setInspecting(true);
    setError(null);
    try {
      const info = await api.inspectBackup(path);
      setBackups((prev) => [info, ...prev.filter((b) => b.path !== info.path)]);
      setSelected(info);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInspecting(false);
    }
  };

  const openBackupFolder = () => void api.openFolder(config.backup_dir).catch((e) => setError(errorMessage(e)));

  const tabs = (
    <div className="segmented wide">
      <button className={tab === 'expand' ? 'on' : ''} onClick={() => switchTab('expand')} disabled={busy}><HardDrive size={14} /> 扩容 Super</button>
      <button className={tab === 'restore' ? 'on' : ''} onClick={() => switchTab('restore')} disabled={busy}><RotateCcw size={14} /> 恢复官方分区表</button>
    </div>
  );
  const errorBox = error && <pre className="error-text">{error}</pre>;
  const back = <button className="btn" onClick={() => onExit(false)}><ArrowLeft size={15} /> 返回刷机</button>;

  // ---------- 执行中：实时进度 ----------
  if (busy) {
    const steps = progress?.steps ?? [];
    const current = progress?.current ?? -1;
    const percent = steps.length ? (Math.max(current, 0) / steps.length) * 100 : 0;
    const label = steps.length && current >= 0 && current < steps.length ? steps[current] : '正在连接手机…';
    const title = stage === 'reading' ? '正在读取扩容方案' : tab === 'expand' ? '正在扩容 Super' : '正在恢复官方分区表';
    return (
      <PageFrame title={title} description={stage === 'reading' ? '只读取分区表，确认方案前不会修改任何分区。' : '整个过程需要几分钟，期间手机会多次重启。'}>
        <div className="callout warn strong"><ShieldAlert size={17} /><span><b>请勿拔出数据线，也不要操作手机。</b>手机进入 Fastboot、Recovery 或重启都是正常现象。</span></div>
        <section className="card">
          <ProgressBar percent={Math.max(percent, 3)} label={label} detail={`${Math.min(Math.max(current, 0) + 1, steps.length || 1)} / ${steps.length || '—'}`} />
          {steps.length > 0 && <StepTimeline steps={steps.map((name) => ({ name }))} current={current} detail={progress?.detail} />}
        </section>
        <LogTail logs={logs} onShowLogs={onShowLogs} />
      </PageFrame>
    );
  }

  // ---------- 完成 ----------
  if (stage === 'done') {
    return tab === 'expand' ? (
      <PageFrame
        title="扩容完成"
        description="Super 分区已经扩大，手机数据已清空。"
        secondary={<button className="btn" onClick={openBackupFolder}><FolderOpen size={15} /> 打开备份文件夹</button>}
        primary={<button className="btn primary large" onClick={() => onExit(true)}>继续刷机 <ArrowRight size={17} /></button>}
      >
        <section className="card result-card">
          <CheckCircle2 size={30} className="ok" />
          <div>
            <strong>扩容前的分区表已备份为压缩包</strong>
            <p className="muted"><b>请把它复制到电脑的其他位置保存好</b>，以后恢复官方分区表时需要用到。</p>
            <code className="path-chip" title={result}>{baseName(result)}</code>
          </div>
        </section>
        <div className="callout info">点击“继续刷机”后会回到连接手机，重新检查容量。刷入方式已自动设为“完整线刷”。</div>
      </PageFrame>
    ) : (
      <PageFrame
        title="官方分区表已恢复"
        description={result}
        primary={<button className="btn primary large" onClick={() => onExit(false)}>完成</button>}
      >
        <div className="callout warn strong"><TriangleAlert size={17} /><span><b>请立即刷入官方完整线刷包</b>，不要直接开机。</span></div>
      </PageFrame>
    );
  }

  // ---------- 扩容 ----------
  if (tab === 'expand') {
    if (stage === 'confirm' && plan) {
      return (
        <PageFrame
          title="确认扩容方案"
          description="请核对方案。确认后会先备份分区表，再重写分区并清空数据。"
          secondary={<button className="btn" onClick={() => setStage('start')}><ArrowLeft size={15} /> 上一步</button>}
          primary={<button className="btn danger large" disabled={word.trim() !== expandWord} onClick={() => void applyExpand()}>备份并开始扩容</button>}
        >
          {tabs}
          <section className="card">
            <dl className="kv big">
              <div><dt>目标磁盘</dt><dd className="mono">{plan.disk}</dd></div>
              <div><dt>当前 Super</dt><dd className="mono">{gib(plan.current_super_bytes)}</dd></div>
              <div><dt>扩容后 Super</dt><dd className="mono ok">{gib(plan.target_bytes)}</dd></div>
              <div><dt>扩容后 Userdata</dt><dd className="mono">{gib(plan.new_userdata_bytes)}</dd></div>
            </dl>
          </section>
          <section className="card danger-card">
            <div className="card-title"><TriangleAlert size={18} /> 扩容会清空手机上的所有数据</div>
            <p className="muted">确认已备份重要资料后，在下方输入 <code>{expandWord}</code> 继续。</p>
            <input className="input mono big" value={word} onChange={(e) => setWord(e.target.value)} placeholder={`输入 ${expandWord}`} />
          </section>
          {errorBox}
        </PageFrame>
      );
    }
    return (
      <PageFrame
        title="Super 物理扩容"
        description="当刷机包需要的 Super 分区比手机现有的大时，用它扩大分区。"
        secondary={back}
        primary={<button className="btn primary large" onClick={() => void readPlan()}>读取扩容方案 <ArrowRight size={17} /></button>}
      >
        {tabs}
        <div className="callout danger"><TriangleAlert size={16} /><span>扩容会重新划分手机的 GPT 分区表，<b>手机里的全部数据都会被清空</b>。开始前会自动备份分区表并压缩保存。</span></div>
        <section className="card">
          <div className="card-title">流程</div>
          <ol className="steps compact">
            <li>连接手机（开机、Recovery、Fastboot 均可）</li>
            <li>工具刷入包内 Recovery，读取分区表并计算扩容方案（只读取，不修改）</li>
            <li>你确认方案并输入确认词后，才会备份并修改分区</li>
          </ol>
        </section>
        {errorBox}
      </PageFrame>
    );
  }

  // ---------- 恢复 ----------
  if (stage === 'confirm' && selected) {
    return (
      <PageFrame
        title="确认恢复"
        description="恢复会把分区表改回扩容前的样子，并清空手机数据。"
        secondary={<button className="btn" onClick={() => setStage('start')}><ArrowLeft size={15} /> 上一步</button>}
        primary={<button className="btn danger large" disabled={word.trim() !== restoreWord} onClick={() => void restore()}>恢复官方分区表</button>}
      >
        {tabs}
        <section className="card">
          <dl className="kv big">
            <div><dt>备份</dt><dd className="mono">{selected.file_name}</dd></div>
            <div><dt>机型</dt><dd className="mono">{selected.device || '未记录'}</dd></div>
            <div><dt>备份时间</dt><dd>{selected.created || '—'}</dd></div>
            <div><dt>恢复后 Super</dt><dd className="mono">{gib(selected.super_old_bytes)}</dd></div>
          </dl>
        </section>
        <section className="card danger-card">
          <div className="card-title"><TriangleAlert size={18} /> 恢复后必须立即刷入官方完整线刷包</div>
          <p className="muted">恢复会清空手机数据，且不要直接开机。确认后在下方输入 <code>{restoreWord}</code>。</p>
          <input className="input mono big" value={word} onChange={(e) => setWord(e.target.value)} placeholder={`输入 ${restoreWord}`} />
        </section>
        {errorBox}
      </PageFrame>
    );
  }
  return (
    <PageFrame
      title="恢复官方分区表"
      description="选择扩容时生成的备份压缩包。工具会检查它是否完整、是否属于当前机型，符合要求才能继续。"
      secondary={back}
      hint={selected && !selected.supported ? '这个备份不能用于当前刷机包' : !selected ? '请先选择备份' : undefined}
      primary={<button className="btn primary large" disabled={!selected?.supported || inspecting} onClick={() => { setWord(''); setStage('confirm'); }}>下一步 <ArrowRight size={17} /></button>}
    >
      {tabs}
      <div className="backup-list">
        {backups.length === 0 && <div className="empty-block"><p>刷机包的备份文件夹里没有找到备份。请用下方按钮选择你保存的备份压缩包。</p></div>}
        {backups.map((b) => (
          <button key={b.path} className={`backup-item ${selected?.path === b.path ? 'selected' : ''} ${b.supported ? '' : 'bad'}`} onClick={() => setSelected(b)}>
            {b.kind === 'zip' ? <FileArchive size={18} /> : <Folder size={18} />}
            <span className="backup-text">
              <strong title={b.path}>{b.file_name}</strong>
              <small>{[b.device, b.created, b.super_old_bytes && `扩容前 Super ${gib(b.super_old_bytes)}`].filter(Boolean).join(' · ') || '无法读取备份信息'}</small>
            </span>
            {b.supported ? <span className="badge ok"><CheckCircle2 size={13} /> 可用</span> : <span className="badge bad"><TriangleAlert size={13} /> 不可用</span>}
          </button>
        ))}
      </div>
      {selected && !selected.supported && <div className="callout danger"><TriangleAlert size={16} /><span>{selected.problems.join('；')}</span></div>}
      {selected?.supported && selected.warnings.length > 0 && <div className="callout warn"><TriangleAlert size={16} /><span>{selected.warnings.join('；')}</span></div>}
      <div className="inline-actions">
        <button className="btn" onClick={() => void pickFile('GPT 备份', ['zip']).then(addBackup)} disabled={inspecting}><FileArchive size={15} /> 选择备份压缩包</button>
        <button className="link-btn" onClick={() => void pickFolder().then(addBackup)} disabled={inspecting}><Folder size={14} /> 旧版备份文件夹</button>
      </div>
      {errorBox}
    </PageFrame>
  );
}
