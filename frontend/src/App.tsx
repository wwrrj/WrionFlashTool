import { useCallback, useEffect, useState } from 'react';
import { ErrorModal } from './components/ErrorModal';
import { LogDrawer } from './components/LogDrawer';
import { NoticeModal } from './components/NoticeModal';
import { Sidebar } from './components/Sidebar';
import type { SidebarTool } from './components/Sidebar';
import { HardDrive, Sparkles } from 'lucide-react';
import { SuperShortModal } from './components/SuperShortModal';
import { Titlebar } from './components/Titlebar';
import { UpdateModal } from './components/UpdateModal';
import { useDevices } from './hooks/useDevices';
import { useFlashEvents } from './hooks/useFlashEvents';
import { useRomPackage } from './hooks/useRomPackage';
import { api, isTauri } from './lib/api';
import { errorMessage, formatGiB, modeLabel } from './lib/format';
import { storage } from './lib/storage';
import { DonePage } from './pages/DonePage';
import { ExpandPage } from './pages/ExpandPage';
import { IdPage } from './pages/IdPage';
import { DevicePage } from './pages/DevicePage';
import { PackagePage } from './pages/PackagePage';
import { PlanPage } from './pages/PlanPage';
import { ReviewPage } from './pages/ReviewPage';
import { RunPage } from './pages/RunPage';
import { WelcomePage } from './pages/WelcomePage';
import { DEFAULT_OPTIONS } from './wizard';
import type { FlashOptions, Step } from './wizard';
import type { EnvReport, FlashProfile, UpdateInfo } from './types';
import './styles/app.css';

export function App() {
  const [step, setStep] = useState<Step>(() => {
    // 浏览器预览可用 ?step=device 等直接打开某一页，方便调界面
    const previewStep = !isTauri() && new URLSearchParams(window.location.search).get('step');
    if (previewStep) return previewStep as Step;
    return storage.agreed() ? 'package' : 'welcome';
  });
  const [profile, setProfile] = useState<FlashProfile | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [options, setOptions] = useState<FlashOptions>(DEFAULT_OPTIONS);
  const [preparing, setPreparing] = useState(true);
  const [extraLabel, setExtraLabel] = useState<string | null>(null);
  /** 刷入结束时手机是否真的重启了（用于完成页提示）。 */
  const [rebooted, setRebooted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);
  const [noticeOpen, setNoticeOpen] = useState(false);
  /** 进入扩容工具前所在的步骤，退出工具时回到这里。 */
  const [returnStep, setReturnStep] = useState<Step>('device');
  /** 工具页（扩容 / 刷入 ID）正在执行，期间锁定导航。 */
  const [toolBusy, setToolBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  /** 预检发现 super 容量不足时的报告，非空即弹出扩容提示。 */
  const [superShort, setSuperShort] = useState<EnvReport | null>(null);
  /** 启动时检查到的新版本，非空即弹出更新提示。 */
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  const events = useFlashEvents();
  const { addLog } = events;

  const loadProfile = useCallback(async () => {
    try {
      setProfile(await api.getProfile());
      setProfileError(null);
    } catch (e) {
      setProfile(null);
      setProfileError(errorMessage(e));
    }
  }, []);

  useEffect(() => { void loadProfile(); }, [loadProfile]);

  // 启动时检查一次更新；网络不通时只记日志，不打扰刷机
  useEffect(() => {
    api.checkUpdate()
      .then((info) => {
        if (!info.available) return;
        addLog(`[更新] 发现新版本 v${info.latest}（当前 v${info.current}）`);
        if (info.required || storage.skippedVersion() !== info.latest) setUpdate(info);
      })
      .catch((e) => addLog(`[更新] 检查更新失败：${errorMessage(e)}`));
  }, [addLog]);

  const openDownload = async () => {
    if (!update) return;
    try {
      await api.openDownload(update.download_url);
      if (!update.required) setUpdate(null);
    } catch (e) {
      // 弹窗保持打开，用户可以复制里面的下载地址
      addLog(`[更新] 无法打开下载页：${errorMessage(e)}`);
    }
  };

  const pkg = useRomPackage(step === 'package', (root) => {
    addLog(`[刷机包] 已启用：${root}`);
    void loadProfile();
  });
  // 检查手机时设备会重启，暂停自动检测以免干扰
  const devices = useDevices(Boolean(profile), step === 'device' && !checking);

  const patchOptions = (patch: Partial<FlashOptions>) => setOptions((prev) => ({ ...prev, ...patch }));

  /**
   * 读取机型、super 容量和防回滚版本，不写入任何内容。
   * super 不足时弹出扩容提示并返回 null；防回滚不通过时抛出错误。
   */
  const precheck = async (): Promise<EnvReport | null> => {
    addLog('>>> 检查手机：机型、Super 容量、防回滚…');
    const report = await api.diagnostics(devices.selectedSerial);
    if (!report.super_ready) {
      addLog(`[检查未通过] Super 容量 ${formatGiB(report.super_bytes)}，刷机包需要 ${formatGiB(report.super_required)}`);
      setSuperShort(report);
      return null;
    }
    if (!report.anti_ok) {
      throw new Error(`防回滚检查未通过：手机的防回滚版本是 ${report.anti ?? '?'}，刷机包是 ${report.anti_package ?? '?'}。\n刷入防回滚版本更低的系统会导致手机无法开机，请换用更新的刷机包。`);
    }
    addLog(`[检查通过] ${report.product} · Super ${formatGiB(report.super_bytes)}`);
    return report;
  };

  const checkDevice = async () => {
    setChecking(true);
    setCheckError(null);
    try {
      if (await precheck()) setStep('plan');
    } catch (e) {
      const message = errorMessage(e);
      addLog(`[检查失败] ${message}`);
      setCheckError(message);
    } finally {
      setChecking(false);
    }
  };

  const start = async () => {
    if (!profile || !options.mode) return;
    const preserveData = options.mode === 'update';
    const serial = devices.selectedSerial;
    const external = options.idSource === 'external';
    const reboot = options.reboot;
    setError(null);
    setPreparing(true);
    setExtraLabel(null);
    events.resetProgress();
    setStep('run');
    try {
      // 刷入前再检查一次（手机可能已更换或重新连接），不通过就不会写入任何内容
      if (!(await precheck())) {
        setStep('plan');
        return;
      }
      setPreparing(false);
      addLog(preserveData ? '>>> 开始系统更新（保留数据）…' : '>>> 开始完整线刷（清空数据）…');
      // 只在最后一步重启：后面还有 ID 补丁或 Root 时，主流程不重启
      let outcome = await api.runFlash({
        chosenSerial: serial,
        preserveData,
        confirmWord: preserveData ? profile.update_confirmation_word || 'UPDATE' : profile.confirmation_word || 'WIPE',
        reboot: !external && !options.rootEnabled && reboot,
      });
      addLog(`[主流程完成] 日志保存于 ${outcome.log_path}`);
      if (external) {
        setExtraLabel('正在刷入 ID 补丁…');
        addLog('>>> 开始刷入 ID 补丁…');
        outcome = await api.runIdFlash({ chosenSerial: serial, idFile: options.idPath, reboot: !options.rootEnabled && reboot });
        addLog(`[ID 补丁完成] 日志保存于 ${outcome.log_path}`);
      }
      if (options.rootEnabled) {
        setExtraLabel('正在刷入 Root 镜像…');
        addLog(`>>> 开始刷入 ${options.rootTarget} Root 镜像…`);
        outcome = await api.runRootFlash({ chosenSerial: serial, imagePath: options.rootPath, target: options.rootTarget, reboot });
        addLog(`[Root 完成] 日志保存于 ${outcome.log_path}`);
      }
      setRebooted(outcome.rebooted);
      if (reboot && !outcome.rebooted) addLog('[提示] 手机没有自动重启，请长按电源键手动重启');
      setStep('done');
    } catch (e) {
      const message = errorMessage(e);
      addLog(`[失败] ${message}`);
      setError(message);
    }
  };

  const openLogsFolder = () => {
    if (!profile) return;
    void api.openFolder(profile.logs_dir).catch((e) => addLog(`[打开目录失败] ${errorMessage(e)}`));
  };

  const agree = () => { storage.setAgreed(); setStep('package'); };
  const again = () => { setOptions(DEFAULT_OPTIONS); events.resetProgress(); setStep('device'); };

  const device = devices.current;
  const showLogs = () => setLogsOpen(true);
  const running = step === 'run' && !error;
  const inTool = step === 'expand' || step === 'idtool';
  const openTool = (tool: Step) => {
    if (!inTool) setReturnStep(step);
    setStep(tool);
  };
  const openExpand = () => openTool('expand');
  const exitExpand = (expanded: boolean) => {
    if (expanded) {
      // 扩容会清空数据，之后只能完整线刷；回到连接手机重新检查容量
      patchOptions({ mode: 'clean' });
      setCheckError(null);
      setStep('device');
    } else {
      setStep(returnStep);
    }
  };
  const busy = running || toolBusy;
  const isOfficial = profile?.kind === 'official';
  const tools: SidebarTool[] = [
    {
      key: 'idtool',
      label: '刷入 ID 补丁',
      hint: '更换 mi_ext 中的 ID',
      icon: <Sparkles size={15} />,
      unavailable: !profile ? '需要先选择刷机包' : isOfficial ? '官方线刷包不需要 ID 补丁' : undefined,
      active: step === 'idtool',
      onOpen: () => openTool('idtool'),
    },
    {
      key: 'expand',
      label: 'Super 扩容 / 恢复',
      hint: '分区容量不足时使用',
      icon: <HardDrive size={15} />,
      unavailable: !profile ? '需要先选择刷机包' : !profile.expansion?.enabled ? '当前刷机包未提供扩容' : undefined,
      active: step === 'expand',
      onOpen: openExpand,
    },
  ];

  const railValues = [
    profile ? profile.package_name || profile.display_name : null,
    device ? `${device.product || '机型待识别'} · ${modeLabel(device.mode)}` : null,
    options.mode ? (options.mode === 'clean' ? '完整线刷（清空数据）' : '保留数据更新') : null,
    step === 'done' ? '已完成' : step === 'run' ? (preparing ? '正在预检' : '正在刷入') : null,
    step === 'done' ? '刷入成功' : null,
  ];

  return (
    <div className="app">
      <Titlebar busy={busy} />

      {step === 'welcome' ? <WelcomePage onAgree={agree} onShowNotice={profile ? () => setNoticeOpen(true) : undefined} /> : (
      <div className="shell">
      <Sidebar
        step={inTool ? returnStep : step}
        toolActive={inTool}
        tools={tools}
        values={railValues}
        locked={busy || checking}
        logCount={events.logs.length}
        onNavigate={setStep}
        onShowLogs={showLogs}
        onShowNotice={profile ? () => setNoticeOpen(true) : undefined}
        qqGroup={profile?.qq_group}
      />
      <div className="content">
      {step === 'package' && (
        <PackagePage key={profile?.rom_root ?? 'none'} pkg={pkg} profile={profile} profileError={profileError} onNext={() => setStep('device')} onShowNotice={() => setNoticeOpen(true)} />
      )}
      {step === 'device' && profile && (
        <DevicePage devices={devices} profile={profile} checking={checking} checkError={checkError} onBack={() => setStep('package')} onCheck={() => void checkDevice()} />
      )}
      {step === 'plan' && profile && (
        <PlanPage profile={profile} options={options} onChange={patchOptions} onBack={() => setStep('device')} onNext={() => setStep('review')} onOpenExpand={openExpand} />
      )}
      {step === 'review' && profile && <ReviewPage profile={profile} device={device} options={options} onBack={() => setStep('plan')} onStart={() => void start()} />}
      {step === 'run' && <RunPage preparing={preparing} extraLabel={extraLabel} stage={events.stage} partition={events.partition} step={events.step} logs={events.logs} onShowLogs={showLogs} />}
      {step === 'done' && <DonePage rebootWanted={options.reboot} rebooted={rebooted} onAgain={again} onShowLogs={showLogs} onOpenLogsFolder={openLogsFolder} />}
      {step === 'expand' && profile?.expansion && (
        <ExpandPage config={profile.expansion} logs={events.logs} onShowLogs={showLogs} onLog={addLog} onBusyChange={setToolBusy} onExit={exitExpand} />
      )}
      {step === 'idtool' && profile && (
        <IdPage serial={devices.selectedSerial} logs={events.logs} onShowLogs={showLogs} onLog={addLog} onBusyChange={setToolBusy} onExit={() => setStep(returnStep)} />
      )}
      </div>
      </div>
      )}

      {logsOpen && <LogDrawer logs={events.logs} onClose={() => setLogsOpen(false)} onClear={() => events.setLogs([])} onOpenFolder={openLogsFolder} />}
      {noticeOpen && profile && <NoticeModal profile={profile} onClose={() => setNoticeOpen(false)} />}
      {update && (
        <UpdateModal
          info={update}
          onDownload={() => void openDownload()}
          onSkip={() => { storage.setSkippedVersion(update.latest); setUpdate(null); }}
          onClose={() => setUpdate(null)}
        />
      )}
      {superShort && (
        <SuperShortModal
          current={superShort.super_bytes}
          required={superShort.super_required}
          canExpand={Boolean(profile?.expansion?.enabled)}
          onExpand={() => { setSuperShort(null); openExpand(); }}
          onCancel={() => { setSuperShort(null); setStep('device'); }}
        />
      )}
      {error && (
        <ErrorModal
          message={error}
          onBack={() => { setError(null); setStep('plan'); }}
          onRetry={() => void start()}
          onViewLogs={() => { setError(null); setLogsOpen(true); setStep('plan'); }}
        />
      )}
    </div>
  );
}

export default App;
