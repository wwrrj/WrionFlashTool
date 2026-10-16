import { useEffect } from 'react';
import type { ReactNode } from 'react';
import { X } from 'lucide-react';

interface ModalProps {
  title: string;
  icon?: ReactNode;
  tone?: 'default' | 'danger';
  width?: number;
  /** 为 true 时隐藏关闭按钮（例如操作进行中）。 */
  locked?: boolean;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}

export function Modal({ title, icon, tone = 'default', width = 520, locked, onClose, children, footer }: ModalProps) {
  useEffect(() => {
    if (locked) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [locked, onClose]);

  return (
    <div className="modal-backdrop" role="presentation">
      <div className="modal" style={{ width }} role="dialog" aria-modal="true" aria-label={title}>
        <header className="modal-head">
          <div className={`modal-icon ${tone}`}>{icon}</div>
          <h2>{title}</h2>
          {!locked && <button className="icon-btn" onClick={onClose} aria-label="关闭"><X size={16} /></button>}
        </header>
        <div className="modal-body">{children}</div>
        {footer && <footer className="modal-foot">{footer}</footer>}
      </div>
    </div>
  );
}
