import React, { useState } from 'react';
import { X, Cpu } from 'lucide-react';

interface RootModalProps {
  isOpen: boolean;
  onClose: () => void;
  onFlashRoot: (imagePath: string, target: string, reboot: boolean) => void;
  isFlashing: boolean;
}

export const RootModal: React.FC<RootModalProps> = ({
  isOpen,
  onClose,
  onFlashRoot,
  isFlashing,
}) => {
  const [imagePath, setImagePath] = useState('');
  const [target, setTarget] = useState<'init_boot' | 'boot'>('init_boot');
  const [reboot, setReboot] = useState(false);
  const [confirmWord, setConfirmWord] = useState('');

  if (!isOpen) return null;

  const isMatched = confirmWord.trim() === 'ROOT' && imagePath.trim().length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 select-none">
      <div className="bg-white rounded-[12px] border border-[#e5e7eb] max-w-lg w-full p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-[#f3f4f6]">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-full bg-[#fb923c]/15 text-[#fb923c] flex items-center justify-center">
              <Cpu className="w-4 h-4" />
            </div>
            <h3 className="font-['Cal_Sans',sans-serif] text-[18px] font-semibold tracking-[-0.3px] text-[#111111]">
              刷入已修补的 Root 镜像
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
          <div className="bg-[#fffbeb] border border-[#fef3c7] rounded-[8px] p-3 text-[12px] text-[#b45309] font-medium leading-relaxed">
            本工具不会在线下载或自动修补 Root，只刷入你已准备好且包含 ANDROID! 头的 .img 镜像。
            此操作不会清空任何用户数据。
          </div>

          <div>
            <label className="block text-[12px] font-semibold text-[#111111] mb-1.5">
              目标分区类型
            </label>
            <div className="grid grid-cols-2 gap-3">
              <button
                type="button"
                onClick={() => setTarget('init_boot')}
                className={`p-3 rounded-[8px] border text-left cursor-pointer transition-all ${
                  target === 'init_boot'
                    ? 'border-[#111111] bg-[#f8f9fa] shadow-xs'
                    : 'border-[#e5e7eb] hover:border-[#d1d5db]'
                }`}
              >
                <div className="font-bold text-[13px] text-[#111111]">Magisk</div>
                <div className="text-[11px] text-[#6b7280] mt-0.5">刷入 init_boot_ab</div>
              </button>

              <button
                type="button"
                onClick={() => setTarget('boot')}
                className={`p-3 rounded-[8px] border text-left cursor-pointer transition-all ${
                  target === 'boot'
                    ? 'border-[#111111] bg-[#f8f9fa] shadow-xs'
                    : 'border-[#e5e7eb] hover:border-[#d1d5db]'
                }`}
              >
                <div className="font-bold text-[13px] text-[#111111]">KernelSU / APatch</div>
                <div className="text-[11px] text-[#6b7280] mt-0.5">刷入 boot_ab</div>
              </button>
            </div>
          </div>

          <div>
            <label className="block text-[12px] font-semibold text-[#111111] mb-1.5">
              Root 镜像完整路径 (.img)
            </label>
            <input
              type="text"
              value={imagePath}
              onChange={(e) => setImagePath(e.target.value.replace(/["']/g, ''))}
              placeholder="请粘贴或输入修补后的 .img 镜像绝对路径"
              className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[13px] font-mono text-[#111111] focus:outline-none transition-colors"
            />
          </div>

          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="root-reboot"
              checked={reboot}
              onChange={(e) => setReboot(e.target.checked)}
              className="w-4 h-4 rounded text-[#111111] focus:ring-0 cursor-pointer"
            />
            <label htmlFor="root-reboot" className="text-[13px] text-[#374151] cursor-pointer">
              刷入成功后自动重启设备
            </label>
          </div>

          <div>
            <label className="block text-[12px] font-medium text-[#6b7280] mb-1.5">
              输入 <span className="font-mono font-bold text-[#111111] bg-[#f5f5f5] px-1.5 py-0.5 rounded border border-[#e5e7eb]">ROOT</span> 确认刷入：
            </label>
            <input
              type="text"
              value={confirmWord}
              onChange={(e) => setConfirmWord(e.target.value)}
              placeholder="ROOT"
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
            onClick={() => onFlashRoot(imagePath, target, reboot)}
            disabled={!isMatched || isFlashing}
            className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-white bg-[#111111] hover:bg-[#242424] active:bg-[#000000] transition-all cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed"
          >
            {isFlashing ? '正在刷入…' : '立即刷入'}
          </button>
        </div>
      </div>
    </div>
  );
};
