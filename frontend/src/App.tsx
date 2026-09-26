import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ArrowLeft, ArrowRight, BookOpen, Check, CheckCircle2,
  Cpu, HardDrive, Info, Loader2, Minus, MonitorCog,
  RefreshCw, RotateCw, ShieldCheck, Smartphone, Sparkles, Terminal,
  TriangleAlert, Usb, X, Zap,
} from 'lucide-react';
import { MockupConsole } from './components/MockupConsole';
import { ConfirmModal } from './components/ConfirmModal';
import { NoticeModal } from './components/NoticeModal';
import { ExpandModal } from './components/ExpandModal';
import { ErrorModal } from './components/ErrorModal';
import { IdChoiceModal } from './components/IdChoiceModal';
import type { DeviceSummary, EnvReport, ExpandPlan, FlashProfile, FlashStageEvent, PartitionProgressEvent } from './types';
import './tokens.css';
import './App.css';

type WizardStep = 'welcome' | 'select' | 'prepare' | 'flash' | 'complete' | 'logs';
type FlashMode = 'clean' | 'update';

const isTauriRuntime = () => typeof window !== 'undefined' && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
const errorMessage = (error: unknown) => error instanceof Error ? error.message : String(error);
const formatGiB = (bytes?: number) => typeof bytes === 'number' && bytes > 0 ? `${(bytes / 1073741824).toFixed(1)} GiB` : '—';
const shortSerial = (serial?: string) => !serial ? '未选择设备' : serial.length > 22 ? `${serial.slice(0, 10)}…${serial.slice(-8)}` : serial;

const STAGES = ['安全预检', '固件擦写', '启动链', '动态分区', '数据处理', '完成重启'];

export function App() {
  const runtimeReady = isTauriRuntime();
  const [profile, setProfile] = useState<FlashProfile | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [devices, setDevices] = useState<DeviceSummary[]>([]);
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [selectedModel, setSelectedModel] = useState('');
  const [report, setReport] = useState<EnvReport | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [stage, setStage] = useState<FlashStageEvent | null>(null);
  const [partitionProgress, setPartitionProgress] = useState<PartitionProgressEvent | null>(null);
  const [step, setStep] = useState<WizardStep>(() => {
    if (typeof window === 'undefined') return 'welcome';
    return localStorage.getItem('harusora.usage-agreed') === 'yes' ? 'select' : 'welcome';
  });
  const [mode, setMode] = useState<FlashMode>('clean');
  const [idEnabled, setIdEnabled] = useState(false);
  const [idPath, setIdPath] = useState('');
  const [idChoiceOpen, setIdChoiceOpen] = useState(false);
  const [idChoiceConfirmed, setIdChoiceConfirmed] = useState(false);
  const [idChoice, setIdChoice] = useState<'package' | 'external'>('package');
  const [rootEnabled, setRootEnabled] = useState(false);
  const [rootPath, setRootPath] = useState('');
  const [rootTarget, setRootTarget] = useState<'init_boot' | 'boot'>('init_boot');
  const [rebootAfter, setRebootAfter] = useState(true);
  const [isScanning, setIsScanning] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [prepError, setPrepError] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [cleanConfirmOpen, setCleanConfirmOpen] = useState(false);
  const [noticeOpen, setNoticeOpen] = useState(false);
  const [expandOpen, setExpandOpen] = useState(false);

  const currentDevice = useMemo(() => devices.find((item) => item.serial === selectedSerial) ?? devices[0], [devices, selectedSerial]);
  const addLog = (message: string) => setLogs((previous) => [...previous, message].slice(-500));
  const currentModeLabel = currentDevice?.mode === 'fastbootd'
    ? 'Fastbootd'
    : currentDevice?.mode === 'fastboot'
      ? 'Bootloader'
      : currentDevice?.mode === 'device'
        ? 'Android 已开机'
        : currentDevice?.mode === 'recovery'
          ? 'Recovery'
          : currentDevice?.mode === 'sideload'
            ? 'Sideload'
            : currentDevice?.mode || '未连接';
  const completedStageCount = stage?.stage === STAGES.length
    ? STAGES.length
    : Math.max(0, Math.min(STAGES.length, (stage?.stage ?? 1) - 1));
  const activeStageName = stage && stage.stage >= 1 && stage.stage <= STAGES.length
    ? STAGES[stage.stage - 1]
    : '等待开始';
  const allowId = !runtimeReady || Boolean(profile?.post_flash_id);
  const allowRoot = !runtimeReady || Boolean(profile?.enable_root_menu);
  const requiresIdChoice = runtimeReady && Boolean(
    profile?.post_flash_id || profile?.dynamic_partitions.some((part) => part.name.toLowerCase() === 'mi_ext'),
  );

  const windowHandle = () => runtimeReady ? getCurrentWindow() : null;
  const minimizeWindow = () => { void windowHandle()?.minimize(); };
  const maximizeWindow = () => { void windowHandle()?.toggleMaximize(); };
  const closeWindow = () => { void windowHandle()?.close(); };
  const dragWindow = () => { void windowHandle()?.startDragging(); };

  const loadProfile = async () => {
    if (!runtimeReady) return;
    try {
      const next = await invoke<FlashProfile>('get_profile', {});
      setProfile(next);
      setProfileError(null);
    } catch (error) {
      const message = errorMessage(error);
      setProfileError(message.includes('未找到 ROM 包配置') || message.includes('未找到刷机配置')
        ? '未找到刷机包配置，请把工具放在 ROM 根目录，或使用 --rom-root 指定目录。'
        : message);
      addLog(`[配置加载失败] ${message}`);
    }
  };

  const scanDevices = async () => {
    if (!runtimeReady) return;
    setIsScanning(true);
    try {
      const next = await invoke<DeviceSummary[]>('scan_devices', {});
      setDevices(next);
      setSelectedSerial((previous) => previous && next.some((item) => item.serial === previous) ? previous : next[0]?.serial ?? null);
    } catch (error) {
      addLog(`[设备扫描失败] ${errorMessage(error)}`);
    } finally {
      setIsScanning(false);
    }
  };

  useEffect(() => {
    const preventContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener('contextmenu', preventContextMenu);
    if (runtimeReady) {
      void loadProfile();
      void scanDevices();
    }
    const subscriptions = runtimeReady ? [
      listen<string>('log_line', (event) => addLog(event.payload)),
      listen<FlashStageEvent>('flash_stage', (event) => setStage(event.payload)),
      listen<PartitionProgressEvent>('partition_progress', (event) => setPartitionProgress(event.payload)),
    ] : [];
    return () => {
      document.removeEventListener('contextmenu', preventContextMenu);
      subscriptions.forEach((subscription) => void subscription.then((unlisten) => unlisten()).catch(() => undefined));
    };
  }, [runtimeReady]);

  const agreeAndContinue = () => {
    localStorage.setItem('harusora.usage-agreed', 'yes');
    setStep('select');
  };

  const runPreparation = async () => {
    setPrepError(null);
    setOperationError(null);
    setStep('prepare');
    setIsBusy(true);
    addLog('>>> 开始设备预检，准备进入合适的刷写模式…');
    if (!runtimeReady) {
      await new Promise((resolve) => window.setTimeout(resolve, 500));
      setReport({ serial: 'preview-device', product: '预览机型', super_bytes: 0, super_ready: true, is_userspace: false, anti: undefined });
      setStep('flash');
      setIsBusy(false);
      return;
    }
    try {
      const nextReport = await invoke<EnvReport>('run_diagnostics_cmd', { chosenSerial: selectedSerial });
      setReport(nextReport);
      addLog(`[预检通过] ${nextReport.product} · ${currentModeLabel} · super ${formatGiB(nextReport.super_bytes)}`);
      setStep('flash');
    } catch (error) {
      const message = errorMessage(error);
      setPrepError(message);
      addLog(`[预检失败] ${message}`);
    } finally {
      setIsBusy(false);
    }
  };

  const runFlash = async () => {
    if (idEnabled && !idPath.trim()) {
      setOperationError('已选择 ID 更新，但没有填写补丁文件路径');
      setStep('select');
      return;
    }
    if (rootEnabled && !rootPath.trim()) {
      setOperationError('已选择 Root，但没有填写镜像文件路径');
      setStep('select');
      return;
    }
    setIsBusy(true);
    setOperationError(null);
    addLog(mode === 'clean' ? '>>> 开始完整线刷（清空数据）…' : '>>> 开始系统更新（保留数据）…');
    if (!runtimeReady) {
      await new Promise((resolve) => window.setTimeout(resolve, 900));
      if (idEnabled) addLog(`[预览] 将刷入 ID 补丁：${idPath || '未填写路径'}`);
      if (rootEnabled) addLog(`[预览] 将刷入 Root 镜像：${rootPath || '未填写路径'}`);
      addLog('[预览] 流程完成。打包后的应用会在这里执行真实 fastboot/adb 操作。');
      setStep('complete');
      setIsBusy(false);
      return;
    }
    try {
      const logPath = await invoke<string>('run_flash_cmd', {
        chosenSerial: selectedSerial,
        preserveData: mode === 'update',
        confirmWord: mode === 'update' ? profile?.update_confirmation_word || 'UPDATE' : profile?.confirmation_word || 'WIPE',
        reboot: !idEnabled && !rootEnabled && rebootAfter,
      });
      addLog(`[主流程完成] 日志保存于 ${logPath}`);
      if (idEnabled) {
        addLog('>>> 开始刷入 ID 补丁…');
        const idLog = await invoke<string>('run_id_flash_cmd', { chosenSerial: selectedSerial, idFile: idPath, formatData: false, confirmWord: 'ID', reboot: !rootEnabled && rebootAfter });
        addLog(`[ID 补丁完成] 日志保存于 ${idLog}`);
      }
      if (rootEnabled) {
        addLog(`>>> 开始刷入 ${rootTarget} Root 镜像…`);
        const rootLog = await invoke<string>('run_root_flash_cmd', { chosenSerial: selectedSerial, imagePath: rootPath, target: rootTarget, confirmWord: 'ROOT', reboot: rebootAfter });
        addLog(`[Root 完成] 日志保存于 ${rootLog}`);
      }
      setStep('complete');
    } catch (error) {
      const message = errorMessage(error);
      setOperationError(message);
      addLog(`[刷入失败] ${message}`);
    } finally {
      setIsBusy(false);
    }
  };

  const continueSelection = () => {
    if (runtimeReady && !profile) {
      setPrepError('未找到刷机包配置，请把工具放在 ROM 根目录，或使用 --rom-root 指定目录。');
      return;
    }
    if (!selectedSerial && runtimeReady) {
      setPrepError('请先连接设备，并点击右上角刷新设备列表');
      return;
    }
    if (requiresIdChoice && !idChoiceConfirmed) {
      setIdChoice(idEnabled ? 'external' : 'package');
      setIdChoiceOpen(true);
      return;
    }
    if (mode === 'clean') setCleanConfirmOpen(true);
    else void runPreparation();
  };

  const confirmIdChoice = () => {
    if (idChoice === 'external' && !idPath.trim()) return;
    setIdEnabled(idChoice === 'external');
    setIdChoiceConfirmed(true);
    setIdChoiceOpen(false);
    if (mode === 'clean') setCleanConfirmOpen(true);
    else void runPreparation();
  };

  const handleCleanConfirmed = () => {
    setCleanConfirmOpen(false);
    void runPreparation();
  };

  const resetToSelection = () => {
    setStep('select');
    setReport(null);
    setStage(null);
    setPartitionProgress(null);
    setOperationError(null);
    setPrepError(null);
    setIdChoiceConfirmed(false);
    setIdChoiceOpen(false);
  };

  const handleOpenFolder = () => {
    if (!runtimeReady || !profile) return addLog('[预览] 打包应用中才可以打开 logs 文件夹');
    const target = profile.logs_dir;
    void invoke('open_folder_cmd', { path: target }).catch((error) => addLog(`[打开目录失败] ${errorMessage(error)}`));
  };

  const getExpandPlan = async (): Promise<ExpandPlan> => {
    if (!runtimeReady) throw new Error('浏览器预览无法读取 Recovery 分区表');
    return invoke<ExpandPlan>('get_expand_plan_cmd', {});
  };
  const applyExpand = async (confirmWord: string) => {
    if (!runtimeReady) throw new Error('浏览器预览无法执行物理扩容');
    setIsBusy(true);
    try { const result = await invoke<string>('apply_expand_cmd', { confirmWord }); addLog(`[扩容成功] ${result}`); setExpandOpen(false); return result; }
    finally { setIsBusy(false); }
  };
  const listBackups = async () => runtimeReady ? invoke<string[]>('list_backups_cmd', {}) : [];
  const restoreBackup = async (backupDir: string, confirmWord: string) => {
    if (!runtimeReady) throw new Error('浏览器预览无法恢复 GPT 分区表');
    setIsBusy(true);
    try { const result = await invoke<string>('restore_backup_cmd', { backupDir, confirmWord }); addLog(`[分区表还原成功] ${result}`); setExpandOpen(false); return result; }
    finally { setIsBusy(false); }
  };

  const titlebar = (
    <header className="window-titlebar">
      <div className="titlebar-drag" onMouseDown={(event) => { if (event.button === 0) void dragWindow(); }}>
        <div className="brand-mark">H</div><div><strong>Harusora FlashTool</strong><span>Android 刷机工具</span></div><small>v2.0</small>
      </div>
      <div className="window-controls">
        <button onClick={minimizeWindow} title="最小化"><Minus size={15} /></button>
        <button onClick={maximizeWindow} title="最大化"><MonitorCog size={14} /></button>
        <button className="close" onClick={closeWindow} title="关闭"><X size={15} /></button>
      </div>
    </header>
  );

  if (step === 'welcome') return <div className="app-shell">{titlebar}<main className="wizard-page welcome-page"><div className="welcome-mark"><Usb size={29} /></div><div className="eyebrow"><span /> Harusora / 首次使用</div><h1>开始之前，先确认你了解刷机风险</h1><p className="welcome-lead">Harusora FlashTool 会根据刷机包配置执行 fastboot 和 adb 操作。完整线刷会清空手机数据，Root、ID 和分区操作也可能影响系统启动。</p><div className="agreement-box"><div className="agreement-icon"><ShieldCheck size={20} /></div><div><strong>使用前必读</strong><p>请确认 Bootloader 已解锁、重要资料已备份，并确认当前刷机包支持你的机型。</p><button className="link-button" onClick={() => setNoticeOpen(true)}><BookOpen size={14} /> 查看完整刷入须知</button></div></div><label className="agreement-check"><input type="checkbox" id="agree" /><span>我已阅读并同意以上说明，知道刷机可能造成数据丢失</span></label><button className="primary-button welcome-continue" onClick={() => { const input = document.getElementById('agree') as HTMLInputElement | null; if (input?.checked) agreeAndContinue(); }}><span>进入设备设置</span><ArrowRight size={17} /></button><small className="remember-note">这个选择会保存在本机，下次启动不再重复询问</small></main><NoticeModal isOpen={noticeOpen} onClose={() => setNoticeOpen(false)} profile={profile} /></div>;

  return (
    <div className="app-shell">
      {titlebar}
      {step === 'select' && <main className="wizard-page select-page">
        <WizardHeader step={2} title="选择设备与刷入方式" description="先确认机型和连接模式，再选择这次要执行的操作。" onBack={() => setStep('welcome')} />
        <div className="select-layout">
        <section className="device-panel"><div className="card-heading"><div><span className="section-kicker">设备连接</span><h2>{currentDevice ? '已找到设备' : '等待连接设备'}</h2></div><button className="icon-button" onClick={() => void scanDevices()} disabled={isScanning} title="刷新设备">{isScanning ? <Loader2 size={17} className="spin" /> : <RefreshCw size={17} />}</button></div>{currentDevice ? <div className="connected-device"><div className="device-symbol"><Smartphone size={23} /></div><div><strong>{shortSerial(currentDevice.serial)}</strong><span>{currentModeLabel} · {currentDevice.product || '机型待检测'}</span></div><CheckCircle2 size={18} className="green-icon" /></div> : <div className="empty-device"><Smartphone size={28} /><strong>未检测到设备</strong><p>手机开机、Recovery 或 Bootloader 状态都可以识别，连接后点击刷新。</p></div>}<div className="device-tip"><Info size={15} /> {currentDevice?.mode === 'device' || currentDevice?.mode === 'recovery' || currentDevice?.mode === 'sideload' ? '设备当前已连接，准备阶段会自动重启到 bootloader。' : '工具会在准备阶段自动切换到需要的模式。'}</div></section>
          <section className="package-panel"><div className="section-kicker">刷机包配置</div><h2>{profile?.display_name || '等待刷机包配置'}</h2>{profileError && <p className="package-error">{profileError}</p>}<label className="model-select-row"><span>目标机型</span><select value={selectedModel || profile?.codenames[0] || ''} onChange={(event) => setSelectedModel(event.target.value)} disabled={!profile?.codenames.length}>{profile?.codenames.map((codename) => <option value={codename} key={codename}>{codename}</option>)}</select></label><div className="package-fields"><div><span>支持机型</span><b>{profile?.codenames.join(' / ') || '配置加载后显示'}</b></div><div><span>Super 最低要求</span><b>{formatGiB(profile?.minimum_super_bytes)}</b></div><div><span>作者 / 构建日期</span><b>{profile ? `${profile.author || '—'} · ${profile.build_date || '—'}` : '—'}</b></div></div></section>
        </div>
        <section className="mode-section"><div className="section-kicker">刷入模式</div><h2>这次要执行哪种操作？</h2><div className="mode-grid"><button className={`mode-card ${mode === 'clean' ? 'selected' : ''}`} onClick={() => setMode('clean')}><span className="mode-icon danger"><Zap size={19} /></span><span><strong>完整线刷</strong><small>首次安装或彻底重装，会清空 userdata</small></span><span className="radio-dot" /></button><button className={`mode-card ${mode === 'update' ? 'selected' : ''}`} onClick={() => setMode('update')}><span className="mode-icon safe"><RotateCw size={19} /></span><span><strong>系统更新</strong><small>覆盖系统分区，保留 userdata 和应用</small></span><span className="radio-dot" /></button></div></section>
        <section className="optional-section"><div className="section-kicker">可选步骤</div><h2>需要同时处理吗？</h2><div className="optional-grid"><OptionToggle icon={<Sparkles size={18} />} title="外部 ID 补丁" description="开始刷入时选择包内 mi_ext 或外部 ID 包" enabled={idEnabled} available={allowId} onToggle={() => setIdEnabled((value) => !value)} color="violet" />{idEnabled && <input className="path-input" value={idPath} onChange={(event) => setIdPath(event.target.value.replace(/["']/g, ''))} placeholder="外部 ID 补丁 .zip 或 .img 的完整路径" />}<OptionToggle icon={<Cpu size={18} />} title="Root 镜像" description="刷入已修补的 boot / init_boot" enabled={rootEnabled} available={allowRoot} onToggle={() => setRootEnabled((value) => !value)} color="orange" />{rootEnabled && <div className="root-fields"><input className="path-input" value={rootPath} onChange={(event) => setRootPath(event.target.value.replace(/["']/g, ''))} placeholder="已修补 .img 镜像的完整路径" /><select value={rootTarget} onChange={(event) => setRootTarget(event.target.value as 'init_boot' | 'boot')}><option value="init_boot">Magisk · init_boot</option><option value="boot">KernelSU / APatch · boot</option></select></div>}</div></section>
        <div className="selection-footer"><div className="selection-options"><button className="secondary-button" onClick={() => setNoticeOpen(true)}><BookOpen size={15} /> 刷入须知</button><label className="reboot-toggle"><input type="checkbox" checked={rebootAfter} onChange={(event) => setRebootAfter(event.target.checked)} /><span>完成后自动重启</span></label></div><div>{prepError && <span className="inline-error"><TriangleAlert size={14} /> {prepError}</span>}<button className="primary-button" onClick={continueSelection} disabled={isBusy}>开始准备 <ArrowRight size={16} /></button></div></div>
        {profile?.expansion?.enabled && <div className="expansion-entry"><div><strong><HardDrive size={17} /> Super 物理扩容</strong><span>刷入包内 Recovery 后，按步骤重划 GPT 并扩展 super</span></div><button className="expansion-button" onClick={() => setExpandOpen(true)}>打开扩容工具 <ArrowRight size={15} /></button></div>}
      </main>}

      {step === 'prepare' && <main className="wizard-page progress-page"><WizardHeader step={3} title="正在准备设备" description="工具会先完成只读检测，并自动进入适合当前任务的模式。" /><div className="progress-card"><div className="progress-spinner"><Loader2 size={31} /></div><h2>{prepError ? '准备失败' : '正在检查设备环境'}</h2><p>{prepError || '正在读取设备代号、Super 分区容量和防回滚信息…'}</p><div className="prepare-list"><ProgressRow label="连接设备" done={Boolean(currentDevice)} /><ProgressRow label="检查刷机包与机型" done={Boolean(report)} active={!report && !prepError} /><ProgressRow label="切换到合适的刷写模式" done={false} active={Boolean(report)} /></div>{prepError ? <div className="progress-actions"><button className="secondary-button" onClick={resetToSelection}><ArrowLeft size={15} /> 返回修改</button><button className="primary-button" onClick={() => void runPreparation()}>重试检测 <RefreshCw size={15} /></button></div> : <button className="secondary-button" onClick={resetToSelection} disabled={isBusy}>取消并返回</button>}</div></main>}

      {step === 'flash' && <main className="wizard-page flash-page"><WizardHeader step={4} title={mode === 'clean' ? '正在完整线刷' : '正在更新系统'} description={mode === 'clean' ? '手机数据会被清空，请不要断开 USB 连接。' : '正在保留用户数据更新系统，请不要断开 USB 连接。'} /><div className="flash-board"><section className="flash-card"><div className="flash-status"><div className="progress-spinner small"><Loader2 size={24} /></div><div><strong>{isBusy ? '刷写进行中…' : '准备开始刷写'}</strong><span>{currentDevice ? `${shortSerial(currentDevice.serial)} · ${currentModeLabel}` : '浏览器预览模式'}</span></div></div><div className="flash-overview"><div><strong>{completedStageCount} / {STAGES.length}</strong><span>步骤已完成</span></div><div><span>当前阶段</span><b>{stage?.title || activeStageName}</b></div></div><div className="stage-list">{STAGES.map((name, index) => { const number = index + 1; const complete = stage ? stage.stage > number || stage.stage === 6 : false; const active = stage?.stage === number; return <div className={`stage-row ${complete ? 'complete' : ''} ${active ? 'active' : ''}`} key={name}><span className="stage-number">{complete ? <Check size={13} /> : number}</span><span>{name}</span><small>{complete ? '已完成' : active ? stage?.title || '进行中' : '待处理'}</small></div>; })}</div>{partitionProgress && <div className="flash-progress"><div><span>正在处理 {partitionProgress.name}</span><b>{partitionProgress.percent}%</b></div><div className="progress-track"><span style={{ width: `${partitionProgress.percent}%` }} /></div></div>}</section><section className="latest-output-panel"><div className="latest-output-heading"><div><Terminal size={15} /><strong>最近输出</strong></div><span>{logs.length} 条</span></div><div className="latest-output-window">{logs.length === 0 ? <p className="latest-output-empty">刷写开始后，最近的 fastboot / adb 输出会显示在这里。</p> : logs.slice(-8).map((line, index) => <p key={`${line}-${index}`}>{line}</p>)}</div></section></div>{!isBusy && !operationError && <button className="primary-button start-flash" onClick={() => void runFlash()}>开始刷入 <ArrowRight size={16} /></button>}</main>}

      {step === 'complete' && <main className="wizard-page complete-page"><div className="complete-icon"><CheckCircle2 size={40} /></div><div className="eyebrow"><span /> Harusora / 已完成</div><h1>{operationError ? '刷入没有完成' : '刷入流程已完成'}</h1><p className="welcome-lead">{operationError || '设备已经完成当前选择的刷入任务。首次启动可能需要几分钟，请保持耐心。'}</p><div className="complete-summary"><div><span>设备</span><strong>{shortSerial(currentDevice?.serial)}</strong></div><div><span>模式</span><strong>{mode === 'clean' ? '完整线刷' : '系统更新'}</strong></div><div><span>附加步骤</span><strong>{[idEnabled && 'ID 补丁', rootEnabled && 'Root'].filter(Boolean).join('、') || '无'}</strong></div></div><div className="complete-actions"><button className="secondary-button" onClick={() => setStep('logs')}><Terminal size={15} /> 查看运行日志</button><button className="primary-button" onClick={resetToSelection}>返回选择页面 <ArrowRight size={15} /></button></div></main>}

      {step === 'logs' && <main className="wizard-page logs-page"><WizardHeader step={5} title="运行日志" description="这里可以滚动查看完整的 fastboot / adb 输出。" onBack={() => setStep('complete')} /><div className="logs-card"><MockupConsole logs={logs} onClearLogs={() => setLogs([])} onOpenLogsFolder={handleOpenFolder} currentStage={stage} partitionProgress={partitionProgress} isRunning={isBusy} /></div></main>}

      <ConfirmModal isOpen={cleanConfirmOpen} title="确认完整线刷（清空数据）" description="此操作会写入固件、启动链和动态分区，并格式化 userdata 与 metadata。手机中的照片、应用和文件都会被清空。" warningNotice="请确认 Bootloader 已解锁，并已备份重要资料。" expectedWord={profile?.confirmation_word || 'WIPE'} onConfirm={handleCleanConfirmed} onCancel={() => setCleanConfirmOpen(false)} confirmLabel="确认并开始准备" isDangerous />
      <NoticeModal isOpen={noticeOpen} onClose={() => setNoticeOpen(false)} profile={profile} />
      <ExpandModal isOpen={expandOpen} onClose={() => setExpandOpen(false)} expansionConfig={profile?.expansion} onGetPlan={getExpandPlan} onApplyExpand={applyExpand} onListBackups={listBackups} onRestoreBackup={restoreBackup} isProcessing={isBusy} />
      <IdChoiceModal isOpen={idChoiceOpen} path={idPath} onPathChange={setIdPath} choice={idChoice} onChoiceChange={setIdChoice} onConfirm={confirmIdChoice} onCancel={() => setIdChoiceOpen(false)} />
      <ErrorModal isOpen={Boolean(operationError)} message={operationError || ''} onClose={() => setOperationError(null)} onReturn={resetToSelection} onViewLogs={() => { setOperationError(null); setStep('logs'); }} />
    </div>
  );
}

function WizardHeader({ step, title, description, onBack }: { step: number; title: string; description: string; onBack?: () => void }) {
  return <div className="wizard-header"><div>{onBack && <button className="back-button" onClick={onBack}><ArrowLeft size={15} /> 返回</button>}<div className="eyebrow"><span /> 步骤 {step} / 5</div><h1>{title}</h1><p>{description}</p></div><div className="stepper"><span className={step >= 1 ? 'done' : ''}>1</span><i /><span className={step >= 2 ? 'done' : ''}>2</span><i /><span className={step >= 3 ? 'done' : ''}>3</span><i /><span className={step >= 4 ? 'done' : ''}>4</span><i /><span className={step >= 5 ? 'done' : ''}>5</span></div></div>;
}

function ProgressRow({ label, done, active }: { label: string; done: boolean; active?: boolean }) {
  return <div className={`prepare-row ${done ? 'done' : ''} ${active ? 'active' : ''}`}><span>{done ? <CheckCircle2 size={17} /> : active ? <Loader2 size={17} className="spin" /> : <span className="empty-circle" />}</span><strong>{label}</strong><small>{done ? '完成' : active ? '进行中' : '等待'}</small></div>;
}

function OptionToggle({ icon, title, description, enabled, available, onToggle, color }: { icon: ReactNode; title: string; description: string; enabled: boolean; available: boolean; onToggle: () => void; color: 'violet' | 'orange' }) {
  return <button className={`option-toggle ${enabled ? 'enabled' : ''} ${color}`} disabled={!available} onClick={onToggle}><span className="option-icon">{icon}</span><span><strong>{title}</strong><small>{available ? description : '当前刷机包未启用'}</small></span><span className="switch"><span /></span></button>;
}

export default App;
