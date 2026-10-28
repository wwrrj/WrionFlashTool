import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowLeft, ArrowRight, Check, ChevronDown, Copy, Cpu, KeyRound, Loader2, Power, RefreshCw, RotateCcw, Smartphone, TriangleAlert, Usb, X } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { useUsbDriver } from '../hooks/useUsbDriver';
import { api } from '../lib/api';
import { errorMessage, modeLabel } from '../lib/format';
import type { BoardIdFix, BoardIdMethod, BoardIdOutcome } from '../types';

interface BoardIdPageProps {
  onShowLogs: () => void;
  onLog: (line: string) => void;
  onBusyChange: (busy: boolean) => void;
  onExit: () => void;
}

/** 这些问题需要用户在手机上操作，界面自动重试，用户操作完不用再点按钮。 */
const WAIT_CODES = ['unauthorized', 'no_device', 'in_fastboot'];
const WAIT_INTERVAL_MS = 3000;
const WAIT_LIMIT_MS = 2 * 60 * 1000;

const FIX_LABELS: Record<BoardIdFix, { label: string; icon: typeof Usb }> = {
  retry_root: { label: '用 Root 权限读取', icon: KeyRound },
  install_driver: { label: '安装 USB 驱动', icon: Usb },
  restart_adb: { label: '重启 ADB 服务', icon: RotateCcw },
  reboot_system: { label: '重启进系统', icon: Power },
};

/**
 * 读取主板 ID（高通 soc0/serial_number）。逐步检查 ADB、连接、授权和权限，
 * 卡在哪一步就告诉用户原因和解决办法，能一键解决的直接给按钮。
 */
export function BoardIdPage({ onShowLogs, onLog, onBusyChange, onExit }: BoardIdPageProps) {
  const [method, setMethod] = useState<BoardIdMethod>('shell');
  const [chosen, setChosen] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<BoardIdOutcome | null>(null);
  const [reading, setReading] = useState(false);
  const [fixing, setFixing] = useState<BoardIdFix | null>(null);
  const [fixNote, setFixNote] = useState<{ ok: boolean; text: string } | null>(null);
  const [copied, setCopied] = useState<'dec' | 'hex' | null>(null);
  /** 开始自动等待的时间；超过 WAIT_LIMIT_MS 后停止自动重试。 */
  const [waitSince, setWaitSince] = useState<number | null>(null);
  const [waitExpired, setWaitExpired] = useState(false);
  const inFlight = useRef(false);
  const driver = useUsbDriver(false);

  useEffect(() => { onBusyChange(reading || fixing !== null); }, [reading, fixing, onBusyChange]);

  const read = useCallback(async (opts: { method?: BoardIdMethod; serial?: string | null; auto?: boolean } = {}) => {
    if (inFlight.current) return;
    inFlight.current = true;
    const m = opts.method ?? method;
    const serial = opts.serial !== undefined ? opts.serial : chosen;
    if (!opts.auto) {
      setReading(true);
      setFixNote(null);
      setWaitExpired(false);
      onLog(`>>> 读取主板 ID（${m === 'root' ? 'Root 权限' : '普通读取'}）…`);
    }
    try {
      const next = await api.readBoardId(serial, m, opts.auto);
      setOutcome(next);
      const code = next.problem?.code;
      if (next.info) {
        onLog(`[主板 ID] ${next.info.serial_number}（${next.info.hex}）`);
      } else if (next.problem && (!opts.auto || !WAIT_CODES.includes(next.problem.code))) {
        onLog(`[读取失败] ${next.problem.title}`);
      }
      setWaitSince((prev) => (code && WAIT_CODES.includes(code) ? prev ?? Date.now() : null));
    } catch (e) {
      const message = errorMessage(e);
      onLog(`[读取失败] ${message}`);
      setOutcome({ checks: [], devices: [], problem: { code: 'internal', title: '读取过程出错', detail: message, steps: ['点“查看日志”复制完整记录，发给刷机包作者或交流群'], actions: [] } });
      setWaitSince(null);
    } finally {
      inFlight.current = false;
      if (!opts.auto) setReading(false);
    }
  }, [method, chosen, onLog]);

  // 等待用户在手机上点允许 / 开机：每隔几秒静默重试一次
  const problemCode = outcome?.problem?.code;
  useEffect(() => {
    if (waitSince === null || reading || fixing || !problemCode || !WAIT_CODES.includes(problemCode)) return;
    const timer = window.setTimeout(() => {
      if (Date.now() - waitSince > WAIT_LIMIT_MS) {
        setWaitSince(null);
        setWaitExpired(true);
        return;
      }
      void read({ auto: true });
    }, WAIT_INTERVAL_MS);
    return () => window.clearTimeout(timer);
  }, [waitSince, reading, fixing, problemCode, outcome, read]);

  const runFix = async (action: BoardIdFix) => {
    if (action === 'retry_root') {
      setMethod('root');
      void read({ method: 'root' });
      return;
    }
    setFixing(action);
    setFixNote(null);
    try {
      if (action === 'install_driver') {
        await driver.install();
        onLog('[主板 ID] 已执行 USB 驱动安装');
      } else {
        const message = await api.boardIdFix(action, null);
        onLog(`[主板 ID] ${message}`);
        setFixNote({ ok: true, text: message });
      }
    } catch (e) {
      const message = errorMessage(e);
      onLog(`[操作失败] ${message}`);
      setFixNote({ ok: false, text: message });
      return;
    } finally {
      setFixing(null);
    }
    // 重启进系统需要等手机开机，交给自动等待；其他操作完成后立即重读
    if (action === 'reboot_system') {
      setWaitSince(Date.now());
      setWaitExpired(false);
    } else {
      void read();
    }
  };

  const copy = async (kind: 'dec' | 'hex', value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(kind);
      window.setTimeout(() => setCopied(null), 1500);
    } catch { setCopied(null); }
  };

  const chooseDevice = (serial: string) => {
    setChosen(serial);
    void read({ serial });
  };

  const info = outcome?.info;
  const problem = outcome?.problem;
  const waiting = waitSince !== null && Boolean(problem && WAIT_CODES.includes(problem.code));
  const busy = reading || fixing !== null;
  const driverNote = driver.result ?? (driver.error ? { message: driver.error, reboot_required: false } : null);

  const primary = info
    ? <button className="btn primary large" onClick={onExit}>完成 <ArrowRight size={17} /></button>
    : <button className="btn primary large" disabled={busy} onClick={() => void read()}>
        {reading ? <><Loader2 size={16} className="spin" /> 正在读取…</> : outcome ? <><RefreshCw size={16} /> 重新读取</> : <><Cpu size={16} /> 开始读取</>}
      </button>;

  return (
    <PageFrame
      title="读取主板 ID"
      description="读取高通芯片的主板序列号（/sys/devices/soc0/serial_number），制作 ID 补丁时需要它。读取只执行查看命令，不会改动手机。"
      secondary={<button className="btn" onClick={onExit} disabled={busy}><ArrowLeft size={15} /> 返回</button>}
      hint={reading ? '读取期间请勿拔线' : undefined}
      primary={primary}
    >
      {info ? (
        <section className="card">
          <div className="board-id">
            <span className="board-id-label">主板 ID</span>
            <strong className="mono selectable">{info.serial_number}</strong>
            <div className="board-id-actions">
              <button className="btn small" onClick={() => void copy('dec', info.serial_number)}>
                {copied === 'dec' ? <Check size={13} className="ok" /> : <Copy size={13} />} 复制
              </button>
              <button className="btn small" onClick={() => void copy('hex', info.hex)}>
                {copied === 'hex' ? <Check size={13} className="ok" /> : <Copy size={13} />} 复制十六进制 <span className="mono">{info.hex}</span>
              </button>
            </div>
          </div>
          <dl className="kv">
            <div><dt>机型</dt><dd>{info.model || '未知'}{info.device && <small className="mono">{info.device}</small>}</dd></div>
            <div><dt>连接模式</dt><dd>{modeLabel(info.mode)}<small className="mono">{info.adb_serial}</small></dd></div>
            <div><dt>读取方式</dt><dd>{info.via_root ? 'Root 权限' : info.mode === 'recovery' ? 'Recovery' : '普通 ADB'}</dd></div>
            <div><dt>节点</dt><dd className="mono">{info.node}</dd></div>
          </dl>
          <div className="card-actions"><button className="link-btn" onClick={() => void read()} disabled={busy}><RefreshCw size={13} /> 重新读取</button></div>
        </section>
      ) : (
        <>
          {!outcome && !reading && (
            <section className="card">
              <div className="card-title">读取前请准备好手机</div>
              <ol className="steps compact">
                <li>手机<b>开机进入系统</b>并解锁屏幕（已在 TWRP / OrangeFox 等第三方 Recovery 中也可以）</li>
                <li>打开 <b>USB 调试</b>，用数据线连接电脑</li>
                <li>手机弹出“允许 USB 调试吗？”时点<b>允许</b></li>
              </ol>
              <details className="tips">
                <summary><ChevronDown size={14} /> 怎么打开 USB 调试？</summary>
                <ul>
                  <li>设置 → 我的设备 → 全部参数与信息 → 连续点击“OS 版本”（或“MIUI 版本”）7 次，打开开发者选项</li>
                  <li>设置 → 更多设置 → 开发者选项 → 打开“USB 调试”</li>
                  <li>插上数据线后下拉通知栏，把 USB 用途改为“传输文件”</li>
                </ul>
              </details>
            </section>
          )}

          {reading && (
            <section className="card waiting">
              <div className="phone-pulse"><Smartphone size={34} /></div>
              <strong>正在读取主板 ID</strong>
              <p className="muted">正在检查 ADB、手机连接和授权。{method === 'root' ? '请留意手机上的 Root 授权弹窗，出现时点“允许”。' : '手机弹出 USB 调试授权时请点“允许”。'}</p>
            </section>
          )}

          {outcome && !reading && (
            <section className="card">
              <div className="card-title">检查结果</div>
              <ol className="timeline">
                {outcome.checks.map((c) => (
                  <li key={c.label} className={c.ok ? 'done' : 'failed'}>
                    <span className="tl-dot">{c.ok ? <Check size={12} strokeWidth={3} /> : <X size={12} strokeWidth={3} />}</span>
                    <span className="tl-name">{c.label}</span>
                    <small className={c.ok ? undefined : 'bad'} title={c.detail ?? undefined}>{c.detail ?? (c.ok ? '通过' : '未通过')}</small>
                  </li>
                ))}
              </ol>
            </section>
          )}

          {problem && !reading && (
            <section className="card problem-card">
              <div className="problem-head">
                <TriangleAlert size={18} />
                <div>
                  <strong>{problem.title}</strong>
                  <p>{problem.detail}</p>
                </div>
              </div>
              {problem.steps.length > 0 && (
                <>
                  <div className="field-label">解决办法</div>
                  <ol className="steps compact">{problem.steps.map((s) => <li key={s}>{s}</li>)}</ol>
                </>
              )}
              {problem.actions.length > 0 && (
                <div className="inline-actions">
                  {problem.actions.map((a) => {
                    const { label, icon: Icon } = FIX_LABELS[a];
                    return (
                      <button key={a} className="btn primary" disabled={busy} onClick={() => void runFix(a)}>
                        {fixing === a ? <Loader2 size={15} className="spin" /> : <Icon size={15} />} {label}
                      </button>
                    );
                  })}
                </div>
              )}
              {waiting && (
                <div className="callout info"><Loader2 size={16} className="spin" /><span>正在等待手机……按上面的步骤操作后，工具会自动重新读取。</span></div>
              )}
              {waitExpired && problem && WAIT_CODES.includes(problem.code) && (
                <div className="callout warn"><TriangleAlert size={16} /><span>已等待 2 分钟仍未成功，自动重试已停止。处理好后请点“重新读取”。</span></div>
              )}
              {fixNote && <div className={`callout ${fixNote.ok ? 'info' : 'danger'}`}>{fixNote.ok ? <Check size={16} /> : <TriangleAlert size={16} />}<span style={{ whiteSpace: 'pre-wrap' }}>{fixNote.text}</span></div>}
              {driverNote && <div className={`callout ${driver.error ? 'danger' : 'info'}`}>{driver.error ? <TriangleAlert size={16} /> : <Check size={16} />}<span style={{ whiteSpace: 'pre-wrap' }}>{driverNote.message}{driver.result && !driver.result.reboot_required ? '，请重新插拔数据线。' : ''}</span></div>}
              <div className="problem-foot">
                {problem.raw && (
                  <details className="tips">
                    <summary><ChevronDown size={14} /> 查看原始输出</summary>
                    <pre className="error-text">{problem.raw}</pre>
                  </details>
                )}
                <button className="link-btn" onClick={onShowLogs}>查看日志</button>
              </div>
            </section>
          )}

          {outcome && outcome.devices.length > 1 && !reading && (
            <section className="card">
              <div className="card-title">连接了 {outcome.devices.length} 台手机，选择要读取的那台</div>
              <div className="backup-list">
                {outcome.devices.map((d) => (
                  <button key={d.serial} className={`backup-item ${(chosen ?? outcome.info?.adb_serial) === d.serial ? 'selected' : ''}`} disabled={busy} onClick={() => chooseDevice(d.serial)}>
                    <Smartphone size={18} />
                    <span className="backup-text"><strong>{d.serial}</strong><small>{modeLabel(d.state)}</small></span>
                  </button>
                ))}
              </div>
            </section>
          )}

          {!reading && (
            <section className="card">
              <div className="card-title">读取方式</div>
              <div className="segmented wide">
                <button className={method === 'shell' ? 'on' : ''} disabled={busy} onClick={() => setMethod('shell')}><Cpu size={13} /> 普通读取</button>
                <button className={method === 'root' ? 'on' : ''} disabled={busy} onClick={() => setMethod('root')}><KeyRound size={13} /> Root 权限读取</button>
              </div>
              <p className="hint-line">{method === 'shell'
                ? '大多数手机用普通 ADB 就能读取；在第三方 Recovery 中也选这个。'
                : '系统限制普通读取（提示“没有权限”）时使用，需要手机已 Root，读取时在手机上允许 Shell 的 Root 授权。'}</p>
            </section>
          )}
        </>
      )}
    </PageFrame>
  );
}
