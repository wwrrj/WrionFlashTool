import { useEffect, useState } from 'react';
import { ArrowLeft, ArrowRight, CheckCircle2, FileArchive, FolderOpen, KeyRound, ShieldAlert, TriangleAlert } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { ToolProgress } from '../components/ToolProgress';
import { useExpandProgress } from '../hooks/useExpandProgress';
import { api, pickFile } from '../lib/api';
import { baseName, errorMessage } from '../lib/format';
import type { FlashProfile, RootCandidates, RootMethod, RootToolOutcome } from '../types';

interface RootPageProps {
  /** 当前刷机包；为空时用户需要自己选择原始镜像。 */
  profile: FlashProfile | null;
  /** 当前选中的手机序列号；为空时自动选择唯一连接的手机。 */
  serial: string | null;
  logs: string[];
  onShowLogs: () => void;
  onLog: (line: string) => void;
  onBusyChange: (busy: boolean) => void;
  onExit: () => void;
}

type Stage = 'start' | 'running' | 'done';

/** 修补方式决定分区：KernelSU（LKM）修补 init_boot，APatch 修补 boot 里的内核。 */
const METHODS: Record<RootMethod, { label: string; target: 'init_boot' | 'boot'; file: string }> = {
  kernelsu: { label: 'KernelSU', target: 'init_boot', file: 'init_boot.img' },
  apatch: { label: 'APatch', target: 'boot', file: 'boot.img' },
};

const dirName = (path: string) => path.replace(/[\\/][^\\/]*$/, '');

/**
 * 独立的 Root 工具：在电脑上修补启动镜像后直接刷入。
 * 选了刷机包时默认用包里的镜像（和内核），用户可以改用外部镜像；没选刷机包时必须选择镜像。
 */
export function RootPage({ profile, serial, logs, onShowLogs, onLog, onBusyChange, onExit }: RootPageProps) {
  const [method, setMethod] = useState<RootMethod>('kernelsu');
  const [useExternal, setUseExternal] = useState(!profile);
  const [candidates, setCandidates] = useState<RootCandidates | null>(null);
  const [imagePath, setImagePath] = useState('');
  const [kernelPath, setKernelPath] = useState('');
  const [kmi, setKmi] = useState('');
  const [superKey, setSuperKey] = useState('');
  const [reboot, setReboot] = useState(true);
  const [stage, setStage] = useState<Stage>('start');
  const [outcome, setOutcome] = useState<RootToolOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { progress, reset } = useExpandProgress();

  useEffect(() => { onBusyChange(stage === 'running'); }, [stage, onBusyChange]);
  useEffect(() => {
    if (!profile) return;
    api.rootCandidates().then(setCandidates).catch(() => setCandidates(null));
  }, [profile]);

  const { label, target, file } = METHODS[method];
  const usePackage = Boolean(profile) && !useExternal;
  const packageImage = (method === 'apatch' ? candidates?.boot_images : candidates?.init_boot_images)?.[0];
  // APatch 默认用刷机包里的内核（如果包里单独放了 Image / kernel）
  const packageKernel = method === 'apatch' ? candidates?.kernel_images[0] : undefined;

  const imageHint = usePackage
    ? (!packageImage ? `刷机包里没有 ${file}，请改用外部镜像` : undefined)
    : (!imagePath.trim() ? `请选择原始 ${file}` : undefined);
  const hint = imageHint ?? (method === 'apatch' && superKey.trim().length < 8 ? 'SuperKey 至少需要 8 位' : undefined);

  const browse = async (name: string, exts: string[], apply: (p: string) => void) => {
    const picked = await pickFile(name, exts);
    if (picked) apply(picked);
  };

  const start = async () => {
    setError(null);
    reset();
    setStage('running');
    onLog(`>>> ${label} 修补并刷入 ${target}（${usePackage ? '刷机包内镜像' : imagePath}）`);
    try {
      const result = await api.runRootTool({
        chosenSerial: serial,
        usePackage,
        method,
        imagePath: usePackage ? '' : imagePath,
        kernelPath: method === 'apatch' ? (usePackage ? packageKernel ?? '' : kernelPath) : '',
        kmi: method === 'kernelsu' ? kmi : '',
        superKey: method === 'apatch' ? superKey : '',
        reboot,
      });
      onLog(`[Root 完成] 修补后的镜像：${result.output_path}`);
      setOutcome(result);
      setStage('done');
    } catch (e) {
      const message = errorMessage(e);
      onLog(`[失败] ${message}`);
      setError(message);
      setStage('start');
    }
  };

  if (stage === 'running') {
    return <ToolProgress title={`正在用 ${label} 修补并刷入`} description={`先在电脑上修补 ${file}，再写入手机的 ${target} 分区。`} progress={progress} logs={logs} onShowLogs={onShowLogs} />;
  }

  if (stage === 'done' && outcome) {
    const isApatch = outcome.method === 'apatch';
    return (
      <PageFrame
        title="Root 已刷入"
        description={`${METHODS[outcome.method].label} · ${outcome.target}`}
        primary={<button className="btn primary large" onClick={onExit}>完成 <ArrowRight size={17} /></button>}
      >
        <section className="card result-card">
          <CheckCircle2 size={30} className="ok" />
          <div>
            <strong>{outcome.target} 已更新</strong>
            <p className="muted">{outcome.rebooted
              ? '手机正在重启。'
              : reboot ? '手机没有自动重启，请长按电源键手动重启。' : '你关闭了自动重启，准备好后长按电源键重启手机。'}
              {isApatch ? '开机后安装 APatch 管理器，用刚才设置的 SuperKey 登录。' : '开机后安装 KernelSU 管理器即可使用。'}</p>
            <button className="btn small path-chip mono" title={outcome.output_path} onClick={() => void api.openFolder(dirName(outcome.output_path))}>
              <FolderOpen size={13} /> {baseName(outcome.output_path)}
            </button>
          </div>
        </section>
        {isApatch && <div className="callout warn"><KeyRound size={16} /><span>请记住 SuperKey，登录 APatch 管理器时需要它，工具不会保存。</span></div>}
        <p className="hint-line">开机异常时，把原始的 {outcome.target === 'boot' ? 'boot.img' : 'init_boot.img'}（{baseName(outcome.source_path)}）刷回 {outcome.target} 即可恢复。</p>
      </PageFrame>
    );
  }

  return (
    <PageFrame
      title="Root 修补刷入"
      description="在电脑上用 KernelSU 或 APatch 修补启动镜像，然后直接刷入手机。需要已解锁 Bootloader。"
      secondary={<button className="btn" onClick={onExit}><ArrowLeft size={15} /> 返回</button>}
      hint={hint}
      primary={<button className="btn primary large" disabled={Boolean(hint)} onClick={() => void start()}>
        <KeyRound size={16} /> 修补并刷入 {target}
      </button>}
    >
      <section className="card">
        <div className="card-title">修补方式</div>
        <div className="segmented wide">
          {(Object.keys(METHODS) as RootMethod[]).map((m) => (
            <button key={m} className={method === m ? 'on' : ''} onClick={() => setMethod(m)}>{METHODS[m].label} · {METHODS[m].target}</button>
          ))}
        </div>
        <p className="hint-line">{method === 'kernelsu'
          ? 'KernelSU（LKM 模式）修补 init_boot 里的 ramdisk，适用于 Android 13 及以后出厂的机型。'
          : 'APatch 修补 boot 里的内核，需要设置 SuperKey。'}</p>
      </section>

      <section className="card">
        <div className="card-title">原始镜像</div>
        {usePackage ? (
          packageImage ? (
            <div className="backup-item selected">
              <FileArchive size={18} />
              <span className="backup-text"><strong title={packageImage}>{baseName(packageImage)}</strong><small>来自当前刷机包{packageKernel ? ` · 内核使用包内 ${baseName(packageKernel)}` : ''}</small></span>
            </div>
          ) : (
            <div className="callout warn"><TriangleAlert size={16} /><span>当前刷机包里没有 {file}。请改用外部镜像。</span></div>
          )
        ) : (
          <>
            {!profile && <p className="hint-line">没有选择刷机包：请选择手机当前系统对应的原始 {file}（版本不一致可能无法开机）。</p>}
            <div className="dest-row">
              <input className="input mono" value={imagePath} placeholder={`选择原始 ${file}`} onChange={(e) => setImagePath(e.target.value.replace(/["']/g, ''))} />
              <button className="btn" onClick={() => void browse(`原始 ${file}`, ['img'], setImagePath)}><FolderOpen size={15} /> 选择</button>
            </div>
            {method === 'apatch' && (
              <div className="dest-row">
                <input className="input mono" value={kernelPath} placeholder="可选：替换为自定义内核（Image / kernel）" onChange={(e) => setKernelPath(e.target.value.replace(/["']/g, ''))} />
                <button className="btn" onClick={() => void browse('自定义内核', ['*'], setKernelPath)}><FolderOpen size={15} /> 选择</button>
              </div>
            )}
          </>
        )}
        {profile && (
          <div className="card-actions">
            <button className="link-btn" onClick={() => setUseExternal((v) => !v)}>
              {useExternal ? '改回使用刷机包里的镜像' : '改用外部镜像'}
            </button>
          </div>
        )}
      </section>

      <section className="card">
        {method === 'kernelsu' ? (
          <>
            <label className="field-label" htmlFor="root-kmi">KMI（可选）</label>
            <input id="root-kmi" className="input mono" value={kmi} placeholder="例如 android14-6.1" onChange={(e) => setKmi(e.target.value.trim())} />
            <p className="hint-line">{usePackage
              ? '留空时从刷机包的 boot.img 自动识别，识别不到再读取已开机手机的内核版本。'
              : '留空时读取已开机手机的内核版本（需要打开 USB 调试）。手机在 Fastboot 模式时请手动填写。'}</p>
          </>
        ) : (
          <>
            <label className="field-label" htmlFor="root-skey">SuperKey</label>
            <input id="root-skey" className="input mono" type="password" value={superKey} maxLength={63} placeholder="8–63 位，登录 APatch 管理器时使用" onChange={(e) => setSuperKey(e.target.value)} />
          </>
        )}
        <label className="reboot-row">
          <input type="checkbox" checked={reboot} onChange={(e) => setReboot(e.target.checked)} />
          <span>完成后自动重启手机</span>
        </label>
      </section>

      <div className="callout info"><ShieldAlert size={16} /><span>会写入手机的 {target} 分区。修补后的镜像保存在电脑上，开机异常时刷回原始 {file} 即可恢复。</span></div>
      {error && <pre className="error-text">{error}</pre>}
    </PageFrame>
  );
}
