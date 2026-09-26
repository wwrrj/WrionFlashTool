import React from 'react';
import { Check, Package, Sparkles, X } from 'lucide-react';

interface IdChoiceModalProps {
  isOpen: boolean;
  path: string;
  onPathChange: (value: string) => void;
  choice: 'package' | 'external';
  onChoiceChange: (value: 'package' | 'external') => void;
  onConfirm: () => void;
  onCancel: () => void;
}

export const IdChoiceModal: React.FC<IdChoiceModalProps> = ({
  isOpen,
  path,
  onPathChange,
  choice,
  onChoiceChange,
  onConfirm,
  onCancel,
}) => {
  if (!isOpen) return null;
  const canConfirm = choice === 'package' || path.trim().length > 0;

  return (
    <div className="id-choice-backdrop">
      <div className="id-choice-modal" role="dialog" aria-modal="true" aria-labelledby="id-choice-title">
        <header className="id-choice-header">
          <div className="id-choice-heading"><Sparkles size={18} /><div><span className="section-kicker">mi_ext</span><h2 id="id-choice-title">选择本次 ID 来源</h2></div></div>
          <button className="id-choice-close" onClick={onCancel} title="取消"><X size={17} /></button>
        </header>
        <p className="id-choice-note">每次刷入都会先刷入 ROM 包内的 mi_ext。你也可以选择一个外部 ID 包，在主流程完成后覆盖它。</p>
        <div className="id-choice-options">
          <button className={`id-choice-option ${choice === 'package' ? 'selected' : ''}`} onClick={() => onChoiceChange('package')}>
            <span className="id-choice-icon"><Package size={18} /></span>
            <span><strong>使用包内 mi_ext</strong><small>默认选项，直接使用当前 ROM 的 mi_ext.new.dat</small></span>
            <span className="id-choice-check">{choice === 'package' && <Check size={15} />}</span>
          </button>
          <button className={`id-choice-option ${choice === 'external' ? 'selected' : ''}`} onClick={() => onChoiceChange('external')}>
            <span className="id-choice-icon external"><Sparkles size={18} /></span>
            <span><strong>刷入外部 ID 包</strong><small>支持含 mi_ext.new.dat 的 zip，或已转换的 .img</small></span>
            <span className="id-choice-check">{choice === 'external' && <Check size={15} />}</span>
          </button>
        </div>
        {choice === 'external' && <input className="id-choice-path" value={path} onChange={(event) => onPathChange(event.target.value.replace(/["']/g, ''))} placeholder="外部 ID zip 或 mi_ext.img 的完整路径" autoFocus />}
        <footer className="id-choice-actions"><button className="secondary-button" onClick={onCancel}>取消</button><button className="primary-button" onClick={onConfirm} disabled={!canConfirm}>确认并继续 <Check size={15} /></button></footer>
      </div>
    </div>
  );
};
