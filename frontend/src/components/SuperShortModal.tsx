import { HardDrive, TriangleAlert } from 'lucide-react';
import { Modal } from './Modal';
import { formatGiB } from '../lib/format';

interface SuperShortModalProps {
  current: number;
  required: number;
  /** 当前刷机包是否提供了 Super 物理扩容。 */
  canExpand: boolean;
  onExpand: () => void;
  onCancel: () => void;
}

/** 预检发现 super 分区容量不足时弹出：引导去扩容，或取消本次刷入。 */
export function SuperShortModal({ current, required, canExpand, onExpand, onCancel }: SuperShortModalProps) {
  return (
    <Modal
      title="Super 分区容量不足"
      tone="danger"
      icon={<TriangleAlert size={17} />}
      onClose={onCancel}
      footer={canExpand
        ? <>
            <button className="btn" onClick={onCancel}>取消刷入</button>
            <button className="btn primary" onClick={onExpand}><HardDrive size={15} /> 去扩容</button>
          </>
        : <button className="btn primary" onClick={onCancel}>我知道了</button>}
    >
      <p>这个刷机包需要更大的 Super 分区，你的手机目前容量不够，<b>无法直接刷入</b>。手机还没有被写入任何内容。</p>
      <dl className="kv">
        <div><dt>手机当前</dt><dd className="mono bad">{formatGiB(current)}</dd></div>
        <div><dt>刷机包需要</dt><dd className="mono">{formatGiB(required)}</dd></div>
      </dl>
      {canExpand
        ? <div className="callout warn"><TriangleAlert size={16} /><span>可以先用“Super 物理扩容”扩大分区，<b>扩容会清空手机上的所有数据</b>，并自动备份原分区表。扩容完成后回来选择“完整线刷”。</span></div>
        : <p className="muted">这个刷机包没有提供扩容功能。请确认刷机包是否适用于你的手机，或联系作者。</p>}
    </Modal>
  );
}
