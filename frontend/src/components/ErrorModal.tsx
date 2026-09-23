import React from 'react';
import { AlertTriangle, ArrowLeft, Terminal, X } from 'lucide-react';

interface ErrorModalProps {
  isOpen: boolean;
  message: string;
  onClose: () => void;
  onReturn: () => void;
  onViewLogs: () => void;
}

export const ErrorModal: React.FC<ErrorModalProps> = ({
  isOpen,
  message,
  onClose,
  onReturn,
  onViewLogs,
}) => {
  if (!isOpen) return null;

  return (
    <div className="error-modal-backdrop" role="presentation">
      <div className="error-modal" role="alertdialog" aria-modal="true" aria-labelledby="flash-error-title">
        <div className="error-modal-header">
          <div className="error-modal-heading">
            <span className="error-modal-icon"><AlertTriangle size={18} /></span>
            <div>
              <span className="section-kicker">刷入未完成</span>
              <h2 id="flash-error-title">操作遇到问题</h2>
            </div>
          </div>
          <button className="error-modal-close" onClick={onClose} title="关闭" aria-label="关闭错误提示"><X size={17} /></button>
        </div>
        <div className="error-modal-body">
          <p>{message}</p>
          <span>设备可能仍停留在 Fastboot 或 Fastbootd，请不要立刻拔出 USB。你可以先查看完整日志，再决定是否重试。</span>
        </div>
        <div className="error-modal-actions">
          <button className="secondary-button" onClick={onViewLogs}><Terminal size={15} /> 查看日志</button>
          <button className="primary-button" onClick={onReturn}><ArrowLeft size={15} /> 返回选择</button>
        </div>
      </div>
    </div>
  );
};
