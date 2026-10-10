import { ArrowLeft, ArrowRight, CheckCircle2, ChevronDown, Loader2, RefreshCw, Smartphone, TriangleAlert } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { modeLabel, shortSerial } from '../lib/format';
import type { useDevices } from '../hooks/useDevices';
import type { FlashProfile } from '../types';

interface DevicePageProps {
  devices: ReturnType<typeof useDevices>;
  profile: FlashProfile;
  /** 正在读取手机信息（机型、Super 容量、防回滚）。 */
  checking: boolean;
  checkError: string | null;
  onBack: () => void;
  /** 检查手机，通过后进入下一步。 */
  onCheck: () => void;
}

export function DevicePage({ devices, profile, checking, checkError, onBack, onCheck }: DevicePageProps) {
  const { devices: list, current, scanning, scanError } = devices;
  const matches = (product?: string | null) => !product || profile.codenames.includes(product);
  const mismatch = current ? !matches(current.product) : false;
  const ready = Boolean(current) && !mismatch;

  const hint = checking
    ? '检查期间请勿拔线'
    : mismatch ? '手机机型与刷机包不匹配' : !current ? '等待连接手机' : undefined;

  return (
    <PageFrame stepNo={2}
      title="连接手机"
      description="用数据线连接手机和电脑，手机开机或处于 Fastboot 模式都可以，工具会自动识别并切换。"
      secondary={<button className="btn" onClick={onBack} disabled={checking}><ArrowLeft size={15} /> 上一步</button>}
      hint={hint}
      primary={<button className="btn primary large" disabled={!ready || checking} onClick={onCheck}>
        {checking ? <><Loader2 size={16} className="spin" /> 正在检查手机…</> : <>检查手机并继续 <ArrowRight size={17} /></>}
      </button>}
    >
      {checking ? (
        <section className="card waiting">
          <div className="phone-pulse"><Smartphone size={34} /></div>
          <strong>正在检查手机</strong>
          <p className="muted">正在读取机型、Super 分区容量和防回滚版本。手机处于开机状态时会自动重启到 Fastboot，这是正常的。</p>
          <p className="muted">检查只读取信息，不会写入任何内容。</p>
        </section>
      ) : list.length === 0 ? (
        <section className="card waiting">
          <div className="phone-pulse"><Smartphone size={34} /></div>
          <strong>{scanning ? '正在查找手机…' : '还没有找到手机'}</strong>
          <p className="muted">连接后会自动识别，不需要手动刷新。</p>
          <ol className="steps">
            <li>用<b>原装或质量好的数据线</b>连接手机和电脑（建议直接插机箱后面的 USB 口）</li>
            <li>手机保持亮屏开机，或进入 Fastboot 模式</li>
            <li>首次连接时，手机上若弹出 USB 调试授权，请点“允许”</li>
          </ol>
          <details className="tips">
            <summary><ChevronDown size={14} /> 一直找不到手机？</summary>
            <ul>
              <li>小米手机进入 Fastboot：关机后，同时按住 <b>音量下 + 电源键</b>，看到 FASTBOOT 画面后松手</li>
              <li>换一根数据线或换一个 USB 口；不要使用 USB 扩展坞</li>
              <li>电脑需要安装 Android 手机的 USB 驱动</li>
            </ul>
          </details>
          {scanError && <small className="bad">{scanError}</small>}
        </section>
      ) : (
        <div className="device-list">
          {list.map((d) => {
            const selected = d.serial === current?.serial;
            const ok = matches(d.product);
            return (
              <button key={d.serial} className={`device-card ${selected ? 'selected' : ''} ${ok ? '' : 'bad'}`} onClick={() => devices.setSelectedSerial(d.serial)}>
                <span className="device-icon"><Smartphone size={20} /></span>
                <span className="device-text">
                  <strong>{d.product || '机型待识别'}</strong>
                  <small>{modeLabel(d.mode)} · 序列号 {shortSerial(d.serial)}</small>
                </span>
                {ok
                  ? <span className="badge ok"><CheckCircle2 size={13} /> 与刷机包匹配</span>
                  : <span className="badge bad"><TriangleAlert size={13} /> 机型不匹配</span>}
              </button>
            );
          })}
          <button className="link-btn rescan" onClick={() => void devices.scan()} disabled={scanning}>
            {scanning ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />} 重新查找
          </button>
          {mismatch && <div className="callout danger"><TriangleAlert size={16} /><span>这台手机是 <b className="mono">{current?.product}</b>，而当前刷机包适用于 <b className="mono">{profile.codenames.join(' / ')}</b>。刷入不匹配的 ROM 可能导致手机无法开机，请更换刷机包或手机。</span></div>}
          {!mismatch && current?.mode === 'device' && <p className="hint-line">手机目前是开机状态，检查时工具会自动把它重启到 Fastboot 模式。</p>}
        </div>
      )}
      {checkError && !checking && <div className="callout danger"><TriangleAlert size={16} /><span style={{ whiteSpace: 'pre-wrap' }}>{checkError}</span></div>}
    </PageFrame>
  );
}
