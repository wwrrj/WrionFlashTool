import { useState } from 'react';
import {
  ArrowRight, BookOpen, CheckCircle2, FileArchive, Folder, FolderOpen, Loader2, PackageCheck, TriangleAlert, UploadCloud,
} from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { ProgressBar } from '../components/ProgressBar';
import { api, pickFile, pickFolder } from '../lib/api';
import { baseName, formatGiB, GIB } from '../lib/format';
import { storage } from '../lib/storage';
import { isZipPath } from '../hooks/useRomPackage';
import type { useRomPackage } from '../hooks/useRomPackage';
import type { FlashProfile } from '../types';

interface PackagePageProps {
  pkg: ReturnType<typeof useRomPackage>;
  profile: FlashProfile | null;
  profileError: string | null;
  onNext: () => void;
  onShowNotice: () => void;
}

export function PackagePage({ pkg, profile, profileError, onNext, onShowNotice }: PackagePageProps) {
  const [changing, setChanging] = useState(false);
  const { phase, info, dest, freeBytes, progress, error, dragging } = pkg;
  const [lastPackage] = useState(() => storage.lastPackage());


  const browseZip = async () => {
    const zip = await pickFile('刷机包', ['zip']);
    if (zip) void pkg.inspect(zip);
  };
  const browseDir = async () => {
    const dir = await pickFolder();
    if (dir && (await pkg.activateDir(dir))) setChanging(false);
  };
  const openRecent = async (path: string) => {
    if (await pkg.open(path)) setChanging(false);
  };
  const browseDest = async () => {
    const folder = await pickFolder();
    if (folder) void pkg.changeDest(folder);
  };

  const showCurrent = profile && !changing && phase === 'idle';
  const needed = info ? info.total_bytes + GIB / 2 : 0;
  const lowSpace = info && !info.already_extracted && freeBytes != null && freeBytes < needed;
  const percent = progress && progress.total_bytes > 0 ? (progress.done_bytes / progress.total_bytes) * 100 : 0;

  let body;
  if (phase === 'extracting' && info) {
    body = (
      <section className="card">
        <div className="card-title"><Loader2 size={18} className="spin" /> 正在解压刷机包</div>
        <p className="muted">第一次使用需要解压，约占用 {formatGiB(info.total_bytes)}。请保持软件打开；中途关闭也没关系，下次选择同一个包会接着解压。</p>
        <ProgressBar
          percent={percent}
          label={progress?.current ? `正在解压 ${baseName(progress.current)}` : '正在准备…'}
          detail={`${Math.floor(percent)}% · ${formatGiB(progress?.done_bytes)} / ${formatGiB(info.total_bytes)}`}
        />
        <div className="card-actions"><button className="btn" onClick={() => void api.cancelExtract()}>取消解压</button></div>
      </section>
    );
  } else if (phase === 'ready' && info) {
    body = (
      <section className="card">
        <PackageSummary name={info.package_name || info.display_name} device={info.display_name} codenames={info.codenames} meta={`${info.file_count} 个文件 · 解压后 ${formatGiB(info.total_bytes)}`} />
        <div className="dest-block">
          <label className="field-label">解压到</label>
          <div className="dest-row">
            <input className="input mono" value={dest} onChange={(e) => void pkg.changeDest(e.target.value.replace(/["']/g, ''))} />
            <button className="btn" onClick={() => void browseDest()}><FolderOpen size={15} /> 更改</button>
          </div>
          <small className={lowSpace ? 'bad' : 'muted'}>
            {info.already_extracted
              ? '这个刷机包已经解压过，直接使用即可。'
              : lowSpace
                ? `磁盘剩余 ${formatGiB(freeBytes)}，不够解压（约需 ${formatGiB(needed)}）。请更改到空间更大的磁盘。`
                : `需要约 ${formatGiB(needed)} 可用空间${freeBytes != null ? `，当前剩余 ${formatGiB(freeBytes)}` : ''}。中途中断可从断点继续。`}
          </small>
        </div>
        <div className="card-actions">
          <button className="btn" onClick={pkg.reset}>重新选择</button>
          <button className="btn primary" disabled={!dest.trim() || Boolean(lowSpace)} onClick={() => void pkg.extract().then((ok) => { if (ok) setChanging(false); })}>
            {info.already_extracted ? '使用此刷机包' : '解压并使用'} <ArrowRight size={16} />
          </button>
        </div>
      </section>
    );
  } else if (showCurrent) {
    body = (
      <section className="card current">
        <div className="ready-badge"><CheckCircle2 size={15} /> 刷机包已就绪</div>
        <PackageSummary name={profile.package_name || profile.display_name} device={profile.display_name} codenames={profile.codenames} meta={[profile.author, profile.build_date].filter(Boolean).join(' · ')} />
        <div className="card-actions">
          <button className="link-btn" onClick={onShowNotice}><BookOpen size={14} /> 刷入须知</button>
          <button className="btn" onClick={() => setChanging(true)}>更换刷机包</button>
        </div>
      </section>
    );
  } else {
    body = (
      <>
        <div className={`dropzone ${dragging ? 'over' : ''}`}>
          {phase === 'inspecting'
            ? <><Loader2 size={30} className="spin" /><strong>正在读取刷机包…</strong><small>大文件可能需要几秒</small></>
            : <>
                <UploadCloud size={32} />
                <strong>把刷机包拖到这里</strong>
                <small>支持 .zip 压缩包，也支持已经解压好的 ROM 文件夹。压缩包会自动解压。</small>
                <div className="dropzone-actions">
                  <button className="btn primary" onClick={() => void browseZip()}><FileArchive size={15} /> 选择 .zip 刷机包</button>
                  <button className="btn" onClick={() => void browseDir()}><Folder size={15} /> 选择已解压的文件夹</button>
                </div>
              </>}
        </div>
        {lastPackage && phase !== 'inspecting' && (
          <button className="recent" onClick={() => void openRecent(lastPackage)}>
            {isZipPath(lastPackage) ? <FileArchive size={16} /> : <Folder size={16} />}
            <span><small>上次使用{isZipPath(lastPackage) ? '' : '（已解压的文件夹）'}</small>{baseName(lastPackage)}</span>
            <ArrowRight size={15} />
          </button>
        )}
        {profile && <button className="link-btn" onClick={() => setChanging(false)}>返回当前刷机包</button>}
        {!profile && profileError && !profileError.includes('未找到') && !error && <div className="callout danger"><TriangleAlert size={16} /><span>{profileError}</span></div>}
      </>
    );
  }

  return (
    <PageFrame stepNo={1}
      title="选择刷机包"
      description="选择你下载的 ROM 刷机包（.zip），或已经解压好的 ROM 文件夹。工具会自动读取其中的刷机配置。"
      primary={showCurrent ? <button className="btn primary large" onClick={onNext}>下一步：连接手机 <ArrowRight size={17} /></button> : undefined}
    >
      {body}
      {error && <div className="callout danger"><TriangleAlert size={16} /><span style={{ whiteSpace: 'pre-wrap' }}>{error}</span></div>}
    </PageFrame>
  );
}

function PackageSummary({ name, device, codenames, meta }: { name: string; device: string; codenames: string[]; meta: string }) {
  return (
    <div className="pkg-summary">
      <div className="pkg-icon"><PackageCheck size={24} /></div>
      <div>
        <strong>{name}</strong>
        <span>适用机型：{device} <em className="mono">({codenames.join(' / ')})</em></span>
        {meta && <small>{meta}</small>}
      </div>
    </div>
  );
}
