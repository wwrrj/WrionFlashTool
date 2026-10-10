import { ArrowLeft, RotateCw, ScrollText, TriangleAlert } from 'lucide-react';
import { Modal } from './Modal';

interface ErrorModalProps {
  message: string;
  onBack: () => void;
  onRetry?: () => void;
  onViewLogs: () => void;
}

export function ErrorModal({ message, onBack, onRetry, onViewLogs }: ErrorModalProps) {
  // 刷入失败后必须选择下一步，不能直接关掉弹窗停留在进度页
  return (
    <Modal
      title="操作没有完成"
      tone="danger"
      icon={<TriangleAlert size={17} />}
      locked
      onClose={onBack}
      footer={<>
        <button className="btn" onClick={onViewLogs}><ScrollText size={15} /> 查看日志</button>
        <button className="btn" onClick={onBack}><ArrowLeft size={15} /> 返回修改</button>
        {onRetry && <button className="btn primary" onClick={onRetry}><RotateCw size={15} /> 重试</button>}
      </>}
    >
      <pre className="error-text">{message}</pre>
      <p className="muted">手机可能还停留在 Fastboot 界面，请先不要拔线。可以先查看日志，再决定是否重试；无法解决时把日志发给作者。</p>
    </Modal>
  );
}
