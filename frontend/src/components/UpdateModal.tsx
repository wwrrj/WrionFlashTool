import { Download, Sparkles, TriangleAlert } from 'lucide-react';
import { Modal } from './Modal';
import type { UpdateInfo } from '../types';

interface UpdateModalProps {
  info: UpdateInfo;
  onDownload: () => void;
  onSkip: () => void;
  onClose: () => void;
}

/** 启动时发现新版本：展示更新内容，引导去 123 云盘下载；必须更新时不能关闭。 */
export function UpdateModal({ info, onDownload, onSkip, onClose }: UpdateModalProps) {
  return (
    <Modal
      title={`发现新版本 v${info.latest}`}
      icon={<Sparkles size={17} />}
      locked={info.required}
      onClose={onClose}
      footer={<>
        {!info.required && <button className="btn" onClick={onSkip}>跳过此版本</button>}
        {!info.required && <button className="btn" onClick={onClose}>以后再说</button>}
        <button className="btn primary" onClick={onDownload}><Download size={15} /> 前往下载</button>
      </>}
    >
      {info.notes.length > 0
        ? <ol className="notice-list">{info.notes.map((note, i) => <li key={i}>{note}</li>)}</ol>
        : <p className="muted">新版本已发布，建议更新。</p>}
      <dl className="kv">
        <div><dt>当前版本</dt><dd className="mono">v{info.current}</dd></div>
        <div><dt>最新版本</dt><dd className="mono">v{info.latest}</dd></div>
        {info.date && <div><dt>发布日期</dt><dd className="mono">{info.date}</dd></div>}
        <div><dt>下载地址</dt><dd className="mono selectable">{info.download_url}</dd></div>
      </dl>
      {info.required
        ? <div className="callout warn"><TriangleAlert size={16} /><span>当前版本存在需要修复的问题，<b>请更新后再使用</b>。下载页会在浏览器中打开，下载后替换原来的 exe 即可。</span></div>
        : <p className="muted">下载页会在浏览器中打开（123 云盘），下载后替换原来的 exe 即可。</p>}
    </Modal>
  );
}
