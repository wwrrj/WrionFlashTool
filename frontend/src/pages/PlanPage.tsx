import { useState } from 'react';
import { ArrowLeft, ArrowRight, ChevronDown, Cpu, FolderOpen, HardDrive, RotateCw, Sparkles, Zap } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { pickFile } from '../lib/api';
import { baseName } from '../lib/format';
import type { FlashOptions } from '../wizard';
import type { FlashProfile } from '../types';

interface PlanPageProps {
  profile: FlashProfile;
  options: FlashOptions;
  onChange: (patch: Partial<FlashOptions>) => void;
  onBack: () => void;
  onNext: () => void;
  onOpenExpand: () => void;
}

export function PlanPage({ profile, options, onChange, onBack, onNext, onOpenExpand }: PlanPageProps) {
  const [advanced, setAdvanced] = useState(false);
  const hasId = Boolean(profile.post_flash_id || profile.dynamic_partitions.some((p) => p.name.toLowerCase() === 'mi_ext'));
  const hasRoot = profile.enable_root_menu;
  const hasExpand = Boolean(profile.expansion?.enabled);
  const hasAdvanced = hasId || hasRoot || hasExpand;
  // 官方包只有包里带了对应脚本才能选
  const official = profile.kind === 'official' ? profile.official : null;
  const canClean = !official || Boolean(official.clean_script);
  const canUpdate = !official || Boolean(official.keep_data_script);

  const missingFile = (options.idSource === 'external' && !options.idPath.trim())
    ? '请选择外部 ID 补丁文件'
    : options.rootEnabled && !options.rootPath.trim() ? '请选择 Root 镜像文件' : null;
  const hint = !options.mode ? '请先选择刷入方式' : missingFile ?? undefined;

  const browse = async (name: string, exts: string[], apply: (p: string) => void) => {
    const picked = await pickFile(name, exts);
    if (picked) apply(picked);
  };

  return (
    <PageFrame stepNo={3}
      title="选择刷入方式"
      description={official
        ? '官方线刷包：完整线刷使用 flash_all.bat，保留数据更新使用 flash_all_except_storage.bat。'
        : '不确定的话，选“完整线刷”最稳妥；只想升级系统并保留数据，选“保留数据更新”。'}
      secondary={<button className="btn" onClick={onBack}><ArrowLeft size={15} /> 上一步</button>}
      hint={hint}
      primary={<button className="btn primary large" disabled={Boolean(hint)} onClick={onNext}>下一步：确认 <ArrowRight size={17} /></button>}
    >
      <div className="choice-grid">
        <button className={`choice ${options.mode === 'clean' ? 'selected' : ''}`} disabled={!canClean} onClick={() => onChange({ mode: 'clean' })}>
          <span className="choice-icon danger"><Zap size={22} /></span>
          <strong>完整线刷</strong>
          <p>全新安装系统，适合第一次刷入或想彻底重装。</p>
          <span className="tag danger">{canClean ? '会清空手机上的所有数据' : '这个刷机包不支持'}</span>
        </button>
        <button className={`choice ${options.mode === 'update' ? 'selected' : ''}`} disabled={!canUpdate} onClick={() => onChange({ mode: 'update' })}>
          <span className="choice-icon safe"><RotateCw size={22} /></span>
          <strong>保留数据更新</strong>
          <p>只覆盖系统分区，保留照片、应用和文件。</p>
          <span className={`tag ${canUpdate ? 'safe' : 'danger'}`}>{canUpdate ? '保留用户数据' : '这个刷机包不支持'}</span>
        </button>
      </div>

      {hasAdvanced && (
        <section className="advanced">
          <button className="advanced-toggle" onClick={() => setAdvanced((v) => !v)} aria-expanded={advanced}>
            <ChevronDown size={16} className={advanced ? 'open' : ''} /> 高级选项 <small>{[hasId && 'ID 补丁', hasRoot && 'Root', hasExpand && '扩容'].filter(Boolean).join('、')} — 大多数用户不需要</small>
          </button>
          {advanced && <div className="advanced-body">
            {hasId && (
              <div className="opt">
                <div className="opt-head"><Sparkles size={17} /><div><strong>ID 补丁（mi_ext）</strong><small>默认使用刷机包自带的，一般不用改</small></div></div>
                <div className="segmented">
                  <button className={options.idSource === 'package' ? 'on' : ''} onClick={() => onChange({ idSource: 'package' })}>使用包内自带</button>
                  <button className={options.idSource === 'external' ? 'on' : ''} onClick={() => onChange({ idSource: 'external' })}>使用外部文件</button>
                </div>
                {options.idSource === 'external' && <FileRow value={options.idPath} placeholder="选择 ID 补丁 .zip 或 .img" onPick={() => void browse('ID 补丁', ['zip', 'img'], (p) => onChange({ idPath: p }))} onChange={(v) => onChange({ idPath: v })} />}
              </div>
            )}
            {hasRoot && (
              <div className="opt">
                <label className="opt-head toggle">
                  <Cpu size={17} /><div><strong>刷入 Root 镜像</strong><small>刷入已修补的 boot / init_boot</small></div>
                  <input type="checkbox" checked={options.rootEnabled} onChange={(e) => onChange({ rootEnabled: e.target.checked })} />
                </label>
                {options.rootEnabled && <>
                  <div className="segmented">
                    <button className={options.rootTarget === 'init_boot' ? 'on' : ''} onClick={() => onChange({ rootTarget: 'init_boot' })}>Magisk · init_boot</button>
                    <button className={options.rootTarget === 'boot' ? 'on' : ''} onClick={() => onChange({ rootTarget: 'boot' })}>KernelSU / APatch · boot</button>
                  </div>
                  <FileRow value={options.rootPath} placeholder="选择已修补的 .img 镜像" onPick={() => void browse('Root 镜像', ['img'], (p) => onChange({ rootPath: p }))} onChange={(v) => onChange({ rootPath: v })} />
                </>}
              </div>
            )}
            {hasExpand && (
              <div className="opt">
                <div className="opt-head"><HardDrive size={17} /><div><strong>Super 物理扩容</strong><small>重划分区表以扩大 super，会清空数据</small></div>
                  <button className="btn small" onClick={onOpenExpand}>打开</button></div>
              </div>
            )}
          </div>}
        </section>
      )}

      <label className="reboot-row">
        <input type="checkbox" checked={options.reboot} onChange={(e) => onChange({ reboot: e.target.checked })} />
        <span>刷入完成后自动重启手机</span>
      </label>
    </PageFrame>
  );
}

function FileRow({ value, placeholder, onPick, onChange }: { value: string; placeholder: string; onPick: () => void; onChange: (v: string) => void }) {
  return (
    <div className="dest-row">
      <input className="input mono" value={value} placeholder={placeholder} title={value ? baseName(value) : undefined} onChange={(e) => onChange(e.target.value.replace(/["']/g, ''))} />
      <button className="btn" onClick={onPick}><FolderOpen size={15} /> 选择</button>
    </div>
  );
}
