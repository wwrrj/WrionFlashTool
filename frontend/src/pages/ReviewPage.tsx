import { useState } from 'react';
import { ArrowLeft, Play, TriangleAlert } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { baseName, modeLabel, shortSerial } from '../lib/format';
import type { FlashOptions } from '../wizard';
import type { DeviceSummary, FlashProfile } from '../types';

interface ReviewPageProps {
  profile: FlashProfile;
  device: DeviceSummary | undefined;
  options: FlashOptions;
  onBack: () => void;
  onStart: () => void;
}

export function ReviewPage({ profile, device, options, onBack, onStart }: ReviewPageProps) {
  const [word, setWord] = useState('');
  const clean = options.mode === 'clean';
  const extras = [
    options.idSource === 'external' && `外部 ID 补丁：${baseName(options.idPath)}`,
  ].filter(Boolean) as string[];
  const canStart = !clean || word.trim() === profile.confirmation_word;

  return (
    <PageFrame stepNo={4}
      title="确认并开始"
      description="请再核对一遍。开始后不要断开 USB，也不要操作手机。"
      secondary={<button className="btn" onClick={onBack}><ArrowLeft size={15} /> 上一步</button>}
      primary={<button className={`btn large ${clean ? 'danger' : 'primary'}`} disabled={!canStart} onClick={onStart}><Play size={16} /> {clean ? '清空数据并开始刷入' : '开始更新'}</button>}
    >
      <section className="card">
        <dl className="kv big">
          <div><dt>刷机包</dt><dd>{profile.package_name || profile.display_name}</dd></div>
          <div><dt>手机</dt><dd>{device?.product || '未知机型'} <small className="mono">{modeLabel(device?.mode)} · {shortSerial(device?.serial)}</small></dd></div>
          <div><dt>刷入方式</dt><dd>{clean ? '完整线刷（清空数据）' : '保留数据更新'}</dd></div>
          {profile.kind === 'official' && profile.official && (
            <div><dt>执行脚本</dt><dd className="mono">{clean ? profile.official.clean_script : profile.official.keep_data_script}</dd></div>
          )}
          <div><dt>附加操作</dt><dd>{extras.length ? extras.join('；') : '无'}</dd></div>
          <div><dt>完成后</dt><dd>{options.reboot ? '自动重启手机' : '不自动重启'}</dd></div>
        </dl>
      </section>

      {clean ? (
        <section className="card danger-card">
          <div className="card-title"><TriangleAlert size={18} /> 这会清空手机上的所有数据</div>
          <p className="muted">照片、应用、聊天记录都会被删除且无法恢复。确认已备份后，在下方输入 <code>{profile.confirmation_word}</code> 继续。</p>
          <input className="input mono big" value={word} onChange={(e) => setWord(e.target.value)} placeholder={`输入 ${profile.confirmation_word}`} autoFocus />
        </section>
      ) : (
        <div className="callout info">此方式会保留 userdata 和应用。若更新后无法开机，请改用“完整线刷”。</div>
      )}
    </PageFrame>
  );
}
