import React from 'react';
import { X, FileText, CheckCircle2, ShieldAlert } from 'lucide-react';
import type { FlashProfile } from '../types';

interface NoticeModalProps {
  isOpen: boolean;
  onClose: () => void;
  profile: FlashProfile | null;
}

export const NoticeModal: React.FC<NoticeModalProps> = ({ isOpen, onClose, profile }) => {
  if (!isOpen || !profile) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 select-none">
      <div className="bg-white rounded-[12px] border border-[#e5e7eb] max-w-lg w-full p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-[#f3f4f6]">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-full bg-[#111111] text-white flex items-center justify-center">
              <FileText className="w-4 h-4" />
            </div>
            <div>
              <h3 className="font-['Cal_Sans',sans-serif] text-[18px] font-semibold tracking-[-0.3px] text-[#111111]">
                ROM 刷入须知
              </h3>
            </div>
          </div>
          <button
            onClick={onClose}
            className="w-8 h-8 rounded-full flex items-center justify-center text-[#6b7280] hover:bg-[#f8f9fa] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="py-4 space-y-4 max-h-[60vh] overflow-y-auto">
          {/* Notes list */}
          <div>
            <h4 className="text-[13px] font-semibold text-[#111111] mb-2 flex items-center gap-1.5">
              <CheckCircle2 className="w-4 h-4 text-[#10b981]" />
              注意事项与说明
            </h4>
            {profile.notes && profile.notes.length > 0 ? (
              <ol className="list-decimal list-inside space-y-2 text-[13px] text-[#374151] leading-relaxed bg-[#f8f9fa] p-3.5 rounded-[8px] border border-[#e5e7eb]">
                {profile.notes.map((note, idx) => (
                  <li key={idx} className="pl-1">
                    {note}
                  </li>
                ))}
              </ol>
            ) : (
              <p className="text-xs text-[#898989] bg-[#f8f9fa] p-3 rounded-[8px] border border-[#e5e7eb]">
                （配置中未提供特别说明）
              </p>
            )}
          </div>

          {/* Safety alert */}
          <div className="bg-[#fffbeb] border border-[#fef3c7] rounded-[8px] p-3 text-[12px] text-[#b45309] flex items-start gap-2">
            <ShieldAlert className="w-4 h-4 shrink-0 mt-0.5" />
            <div>
              <span className="font-bold">安全声明：</span>
              本工具严格遵循开源原则，刷机过程中<strong className="text-[#dc2626]">绝不会</strong>执行锁 Bootloader 操作（fastboot oem lock 等），防止任何意外变砖情况。
            </div>
          </div>

          {/* Package details */}
          <div className="bg-[#f5f5f5] rounded-[8px] p-3.5 border border-[#e5e7eb] text-xs space-y-2">
            <div className="flex justify-between">
              <span className="text-[#6b7280]">支持机型代号</span>
              <span className="font-mono font-medium text-[#111111]">{profile.codenames.join(', ')}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-[#6b7280]">作者 / 团队</span>
              <span className="text-[#111111] font-medium">{profile.author}</span>
            </div>
            {profile.qq_group && (
              <div className="flex justify-between">
                <span className="text-[#6b7280]">QQ 交流群</span>
                <span className="font-mono text-[#111111] font-medium">{profile.qq_group}</span>
              </div>
            )}
            {profile.build_date && (
              <div className="flex justify-between">
                <span className="text-[#6b7280]">构建日期</span>
                <span className="font-mono text-[#111111] font-medium">{profile.build_date}</span>
              </div>
            )}
            <div className="flex justify-between">
              <span className="text-[#6b7280]">配置文件路径</span>
              <span className="font-mono text-[#111111] truncate max-w-[260px]" title={profile.profile_path}>
                {profile.profile_path}
              </span>
            </div>
          </div>
        </div>

        <div className="flex items-center justify-end pt-3 border-t border-[#f3f4f6]">
          <button
            onClick={onClose}
            className="h-[38px] px-5 rounded-[8px] text-[13px] font-semibold text-white bg-[#111111] hover:bg-[#242424] transition-all cursor-pointer"
          >
            我已知晓
          </button>
        </div>
      </div>
    </div>
  );
};
