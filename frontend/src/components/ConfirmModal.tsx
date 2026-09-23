import React, { useState, useEffect } from 'react';
import { AlertTriangle, X } from 'lucide-react';

interface ConfirmModalProps {
  isOpen: boolean;
  title: string;
  description: string;
  warningNotice?: string;
  expectedWord: string;
  onConfirm: () => void;
  onCancel: () => void;
  confirmLabel?: string;
  isDangerous?: boolean;
}

export const ConfirmModal: React.FC<ConfirmModalProps> = ({
  isOpen,
  title,
  description,
  warningNotice,
  expectedWord,
  onConfirm,
  onCancel,
  confirmLabel = '确认执行',
  isDangerous = false,
}) => {
  const [inputVal, setInputVal] = useState('');

  useEffect(() => {
    if (isOpen) {
      setInputVal('');
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const isMatched = inputVal.trim() === expectedWord;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 select-none">
      <div className="bg-white rounded-[12px] border border-[#e5e7eb] max-w-md w-full p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-[#f3f4f6]">
          <div className="flex items-center gap-2.5">
            <div className={`w-8 h-8 rounded-full flex items-center justify-center ${isDangerous ? 'bg-[#ef4444]/10 text-[#ef4444]' : 'bg-[#f59e0b]/10 text-[#f59e0b]'}`}>
              <AlertTriangle className="w-4 h-4" />
            </div>
            <h3 className="font-['Cal_Sans',sans-serif] text-[18px] font-semibold tracking-[-0.3px] text-[#111111]">
              {title}
            </h3>
          </div>
          <button
            onClick={onCancel}
            className="w-8 h-8 rounded-full flex items-center justify-center text-[#6b7280] hover:bg-[#f8f9fa] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="py-4">
          <p className="text-[14px] text-[#374151] leading-relaxed mb-3">
            {description}
          </p>

          {warningNotice && (
            <div className="bg-[#fffbeb] border border-[#fef3c7] rounded-[8px] p-3 text-[12px] text-[#b45309] font-medium leading-relaxed mb-4">
              {warningNotice}
            </div>
          )}

          <div className="mt-4">
            <label className="block text-[12px] font-medium text-[#6b7280] mb-1.5">
              请输入确认词 <span className="font-mono font-bold text-[#111111] bg-[#f5f5f5] px-1.5 py-0.5 rounded border border-[#e5e7eb]">{expectedWord}</span> 确认操作：
            </label>
            <input
              type="text"
              autoFocus
              value={inputVal}
              onChange={(e) => setInputVal(e.target.value)}
              placeholder={`输入 ${expectedWord}`}
              className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[14px] font-mono text-[#111111] focus:outline-none transition-colors"
            />
          </div>
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t border-[#f3f4f6]">
          <button
            onClick={onCancel}
            className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-[#111111] bg-white border border-[#e5e7eb] hover:bg-[#f8f9fa] active:bg-[#e5e7eb] transition-all cursor-pointer"
          >
            取消
          </button>
          <button
            onClick={() => {
              if (isMatched) onConfirm();
            }}
            disabled={!isMatched}
            className={`h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-white transition-all cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed ${
              isDangerous
                ? 'bg-[#ef4444] hover:bg-[#dc2626] active:bg-[#b91c1c]'
                : 'bg-[#111111] hover:bg-[#242424] active:bg-[#000000]'
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
};
