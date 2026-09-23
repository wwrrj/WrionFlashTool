import React, { useState } from 'react';
import { X, Sparkles } from 'lucide-react';

interface IdModalProps {
  isOpen: boolean;
  onClose: () => void;
  onFlashId: (idFile: string, formatData: boolean, reboot: boolean) => void;
  isFlashing: boolean;
  defaultCandidate?: string;
}

export const IdModal: React.FC<IdModalProps> = ({
  isOpen,
  onClose,
  onFlashId,
  isFlashing,
  defaultCandidate,
}) => {
  const [idFile, setIdFile] = useState(defaultCandidate || '');
  const [formatData, setFormatData] = useState(false);
  const [reboot, setReboot] = useState(false);
  const [confirmWord, setConfirmWord] = useState('');

  if (!isOpen) return null;

  const isMatched = confirmWord.trim() === 'ID' && idFile.trim().length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 select-none">
      <div className="bg-white rounded-[12px] border border-[#e5e7eb] max-w-lg w-full p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-[#f3f4f6]">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-full bg-[#8b5cf6]/15 text-[#8b5cf6] flex items-center justify-center">
              <Sparkles className="w-4 h-4" />
            </div>
            <h3 className="font-['Cal_Sans',sans-serif] text-[18px] font-semibold tracking-[-0.3px] text-[#111111]">
              刷入 / 更新 ColorOS ID 补丁
            </h3>
          </div>
          <button
            onClick={onClose}
            className="w-8 h-8 rounded-full flex items-center justify-center text-[#6b7280] hover:bg-[#f8f9fa] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="py-4 space-y-4">
          <div className="bg-[#f5f3ff] border border-[#ede9fe] rounded-[8px] p-3 text-[12px] text-[#6d28d9] font-medium leading-relaxed">
            ROM 包内的 mi_ext 只是占位分区，必须刷入独立 ID 补丁 zip（含 mi_ext.new.dat + transfer.list）或 .img 才能正常通过系统向导。未刷入 ID 可能停在开机动画。
          </div>

          <div>
            <label className="block text-[12px] font-semibold text-[#111111] mb-1.5">
              ID 补丁文件路径 (.zip 或 .img)
            </label>
            <input
              type="text"
              value={idFile}
              onChange={(e) => setIdFile(e.target.value.replace(/["']/g, ''))}
              placeholder="请输入或粘贴 ID 补丁 zip 或 mi_ext.img 绝对路径"
              className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[13px] font-mono text-[#111111] focus:outline-none transition-colors"
            />
          </div>

          <div className="space-y-2 pt-1">
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                id="id-format-data"
                checked={formatData}
                onChange={(e) => setFormatData(e.target.checked)}
                className="w-4 h-4 rounded text-[#111111] focus:ring-0 cursor-pointer"
              />
              <label htmlFor="id-format-data" className="text-[13px] text-[#374151] cursor-pointer">
                刷入后清空用户数据（Erase userdata / metadata，首次刷入推荐）
              </label>
            </div>

            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                id="id-reboot"
                checked={reboot}
                onChange={(e) => setReboot(e.target.checked)}
                className="w-4 h-4 rounded text-[#111111] focus:ring-0 cursor-pointer"
              />
              <label htmlFor="id-reboot" className="text-[13px] text-[#374151] cursor-pointer">
                刷入成功后自动重启设备
              </label>
            </div>
          </div>

          <div>
            <label className="block text-[12px] font-medium text-[#6b7280] mb-1.5">
              输入 <span className="font-mono font-bold text-[#111111] bg-[#f5f5f5] px-1.5 py-0.5 rounded border border-[#e5e7eb]">ID</span> 确认刷入：
            </label>
            <input
              type="text"
              value={confirmWord}
              onChange={(e) => setConfirmWord(e.target.value)}
              placeholder="ID"
              className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[14px] font-mono text-[#111111] focus:outline-none transition-colors"
            />
          </div>
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t border-[#f3f4f6]">
          <button
            onClick={onClose}
            className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-[#111111] bg-white border border-[#e5e7eb] hover:bg-[#f8f9fa] transition-all cursor-pointer"
          >
            取消
          </button>
          <button
            onClick={() => onFlashId(idFile, formatData, reboot)}
            disabled={!isMatched || isFlashing}
            className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-white bg-[#111111] hover:bg-[#242424] active:bg-[#000000] transition-all cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed"
          >
            {isFlashing ? '正在刷入…' : '立即刷入 ID 补丁'}
          </button>
        </div>
      </div>
    </div>
  );
};
