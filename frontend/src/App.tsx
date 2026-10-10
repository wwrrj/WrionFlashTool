import { useCallback, useEffect, useState } from 'react';
import { ErrorModal } from './components/ErrorModal';
import { LogDrawer } from './components/LogDrawer';
import { NoticeModal } from './components/NoticeModal';
import { Sidebar } from './components/Sidebar';
import { SuperShortModal } from './components/SuperShortModal';
import { Titlebar } from './components/Titlebar';
import { useDevices } from './hooks/useDevices';
import { useFlashEvents } from './hooks/useFlashEvents';
import { useRomPackage } from './hooks/useRomPackage';
import { api, isTauri } from './lib/api';
import { errorMessage, formatGiB, modeLabel } from './lib/format';
import { storage } from './lib/storage';
import { DonePage } from './pages/DonePage';
import { ExpandPage } from './pages/ExpandPage';
import { DevicePage } from './pages/DevicePage';
import { PackagePage } from './pages/PackagePage';
import { PlanPage } from './pages/PlanPage';
import { ReviewPage } from './pages/ReviewPage';
import { RunPage } from './pages/RunPage';
import { WelcomePage } from './pages/WelcomePage';
import { DEFAULT_OPTIONS } from './wizard';
import type { FlashOptions, Step } from './wizard';
import type { EnvReport, FlashProfile } from './types';
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
  const [error, setError] = useState<string | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);
  const [noticeOpen, setNoticeOpen] = useState(false);
  /** 进入扩容工具前所在的步骤，退出工具时回到这里。 */
  const [returnStep, setReturnStep] = useState<Step>('device');
  const [expandBusy, setExpandBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  /** 预检发现 super 容量不足时的报告，非空即弹出扩容提示。 */
  const [superShort, setSuperShort] = useState<EnvReport | null>(null);

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
      const logPath = await api.runFlash({
        chosenSerial: serial,
        preserveData,
        confirmWord: preserveData ? profile.update_confirmation_word || 'UPDATE' : profile.confirmation_word || 'WIPE',
        reboot: !external && !options.rootEnabled && reboot,
      });
      addLog(`[主流程完成] 日志保存于 ${logPath}`);
      if (external) {
        setExtraLabel('正在刷入 ID 补丁…');
        addLog('>>> 开始刷入 ID 补丁…');
        addLog(`[ID 补丁完成] ${await api.runIdFlash({ chosenSerial: serial, idFile: options.idPath, reboot: !options.rootEnabled && reboot })}`);
      }
      if (options.rootEnabled) {
        setExtraLabel('正在刷入 Root 镜像…');
        addLog(`>>> 开始刷入 ${options.rootTarget} Root 镜像…`);
        addLog(`[Root 完成] ${await api.runRootFlash({ chosenSerial: serial, imagePath: options.rootPath, target: options.rootTarget, reboot })}`);
      }
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
  const openExpand = () => {
    if (step !== 'expand') setReturnStep(step);
    setStep('expand');
  };
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
  const inExpand = step === 'expand';
  const busy = running || expandBusy;

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
        step={inExpand ? returnStep : step}
        toolActive={inExpand}
        onOpenExpand={profile?.expansion?.enabled ? openExpand : undefined}
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
      {step === 'run' && <RunPage preparing={preparing} extraLabel={extraLabel} stage={events.stage} partition={events.partition} logs={events.logs} onShowLogs={showLogs} />}
      {step === 'done' && <DonePage reboot={options.reboot} onAgain={again} onShowLogs={showLogs} onOpenLogsFolder={openLogsFolder} />}
      {inExpand && profile?.expansion && (
        <ExpandPage config={profile.expansion} logs={events.logs} onShowLogs={showLogs} onLog={addLog} onBusyChange={setExpandBusy} onExit={exitExpand} />
      )}
      </div>
      </div>
      )}

      {logsOpen && <LogDrawer logs={events.logs} onClose={() => setLogsOpen(false)} onClear={() => events.setLogs([])} onOpenFolder={openLogsFolder} />}
      {noticeOpen && profile && <NoticeModal profile={profile} onClose={() => setNoticeOpen(false)} />}
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
