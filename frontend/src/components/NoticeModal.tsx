import { ShieldAlert, FileText } from 'lucide-react';
import { Modal } from './Modal';
import type { FlashProfile } from '../types';

export function NoticeModal({ profile, onClose }: { profile: FlashProfile; onClose: () => void }) {
  return (
    <Modal
      title="刷入须知"
      icon={<FileText size={17} />}
      width={560}
      onClose={onClose}
      footer={<button className="btn primary" onClick={onClose}>我已知晓</button>}
    >
      {profile.notes.length > 0
        ? <ol className="notice-list">{profile.notes.map((note, i) => <li key={i}>{note}</li>)}</ol>
        : <p className="muted">这个刷机包没有提供额外说明。</p>}
      <div className="callout warn">
        <ShieldAlert size={16} />
        <span>本工具<b>不会</b>执行锁定 Bootloader 的操作。</span>
      </div>
      <dl className="kv">
        <div><dt>支持机型</dt><dd className="mono">{profile.codenames.join(', ')}</dd></div>
        <div><dt>作者</dt><dd>{profile.author || '—'}</dd></div>
        {profile.qq_group && <div><dt>QQ 交流群</dt><dd className="mono">{profile.qq_group}</dd></div>}
        {profile.build_date && <div><dt>构建日期</dt><dd className="mono">{profile.build_date}</dd></div>}
      </dl>
    </Modal>
  );
}
