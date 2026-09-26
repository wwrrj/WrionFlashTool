import React, { useState, useEffect } from 'react';
import { X, HardDrive } from 'lucide-react';
import type { ExpandPlan, ExpansionConfig } from '../types';

interface ExpandModalProps {
  isOpen: boolean;
  onClose: () => void;
  expansionConfig: ExpansionConfig | undefined;
  onGetPlan: () => Promise<ExpandPlan>;
  onApplyExpand: (confirmWord: string) => Promise<string>;
  onListBackups: () => Promise<string[]>;
  onRestoreBackup: (backupDir: string, confirmWord: string) => Promise<string>;
  isProcessing: boolean;
}

export const ExpandModal: React.FC<ExpandModalProps> = ({
  isOpen,
  onClose,
  expansionConfig,
  onGetPlan,
  onApplyExpand,
  onListBackups,
  onRestoreBackup,
  isProcessing,
}) => {
  const [activeSubTab, setActiveSubTab] = useState<'expand' | 'restore'>('expand');
  const [plan, setPlan] = useState<ExpandPlan | null>(null);
  const [loadingPlan, setLoadingPlan] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [confirmWord, setConfirmWord] = useState('');

  const [backups, setBackups] = useState<string[]>([]);
  const [selectedBackup, setSelectedBackup] = useState('');
  const [restoreConfirmWord, setRestoreConfirmWord] = useState('');

  useEffect(() => {
    if (isOpen) {
      setPlan(null);
      setPlanError(null);
      setConfirmWord('');
      setRestoreConfirmWord('');
      if (activeSubTab === 'restore') {
        loadBackupsList();
      }
    }
  }, [isOpen, activeSubTab]);

  const loadBackupsList = async () => {
    try {
      const list = await onListBackups();
      setBackups(list);
      if (list.length > 0) {
        setSelectedBackup(list[0]);
      }
    } catch (e: any) {
      console.error(e);
    }
  };

  const handleFetchPlan = async () => {
    setLoadingPlan(true);
    setPlanError(null);
    try {
      const p = await onGetPlan();
      setPlan(p);
    } catch (e: any) {
      setPlanError(e.toString());
    } finally {
      setLoadingPlan(false);
    }
  };

  if (!isOpen) return null;

  const targetConfirm = expansionConfig?.confirm_word || 'EXPAND';
  const targetRestoreConfirm = expansionConfig?.restore_confirm_word || 'RESTORE';

  const canApplyExpand = plan && confirmWord.trim() === targetConfirm;
  const canApplyRestore = selectedBackup && restoreConfirmWord.trim() === targetRestoreConfirm;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 select-none">
      <div className="bg-white rounded-[12px] border border-[#e5e7eb] max-w-xl w-full p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-[#f3f4f6]">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-full bg-[#111111] text-white flex items-center justify-center">
              <HardDrive className="w-4 h-4" />
            </div>
            <div>
              <h3 className="font-['Cal_Sans',sans-serif] text-[18px] font-semibold tracking-[-0.3px] text-[#111111]">
                Super 物理扩容 / 官方布局还原
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

        {/* Sub-tab navigation */}
        <div className="flex border-b border-[#e5e7eb] mt-3">
          <button
            onClick={() => setActiveSubTab('expand')}
            className={`pb-2 px-4 text-xs font-semibold border-b-2 cursor-pointer transition-colors ${
              activeSubTab === 'expand'
                ? 'border-[#111111] text-[#111111]'
                : 'border-transparent text-[#6b7280] hover:text-[#111111]'
            }`}
          >
            扩容到目标大小 (EXPAND)
          </button>
          <button
            onClick={() => {
              setActiveSubTab('restore');
              loadBackupsList();
            }}
            className={`pb-2 px-4 text-xs font-semibold border-b-2 cursor-pointer transition-colors ${
              activeSubTab === 'restore'
                ? 'border-[#111111] text-[#111111]'
                : 'border-transparent text-[#6b7280] hover:text-[#111111]'
            }`}
          >
            恢复官方 GPT 分区表 (RESTORE)
          </button>
        </div>

        <div className="py-4">
          {activeSubTab === 'expand' ? (
            <div className="space-y-4">
              <div className="bg-[#fff1f2] border border-[#ffe4e6] rounded-[8px] p-3 text-[12px] text-[#e11d48] font-medium leading-relaxed">
                物理扩容会重新划分 UFS/EMMC 上的 GPT 物理分区表，并移动 userdata 起始扇区。
                手机中的全部数据都会被清空！操作前会自动创建全量 GPT 备份。
              </div>

              {!plan && (
                <div className="text-center py-6 border border-dashed border-[#e5e7eb] rounded-[10px] bg-[#fafafa]">
                  <p className="text-xs text-[#6b7280] mb-3">
                    连接手机后直接开始。工具会从 Android、Recovery、fastboot 或 fastbootd 自动切换，先刷入包内 Recovery，再读取 GPT 计算方案：
                  </p>
                  <button
                    onClick={handleFetchPlan}
                    disabled={loadingPlan}
                    className="h-[36px] px-4 rounded-[6px] text-xs font-semibold text-white bg-[#111111] hover:bg-[#242424] cursor-pointer"
                  >
                    {loadingPlan ? '正在刷入 Recovery 并读取 GPT…' : '刷入 Recovery 并计算方案'}
                  </button>
                  {planError && (
                    <div className="text-[12px] text-[#ef4444] mt-3 px-4 font-mono">
                      {planError}
                    </div>
                  )}
                </div>
              )}

              {plan && (
                <div className="space-y-3">
                  <div className="bg-[#f8f9fa] border border-[#e5e7eb] rounded-[8px] p-3 text-xs space-y-2">
                    <div className="flex justify-between font-mono">
                      <span className="text-[#6b7280]">目标磁盘设备:</span>
                      <span className="font-semibold text-[#111111]">{plan.disk}</span>
                    </div>
                    <div className="flex justify-between font-mono">
                      <span className="text-[#6b7280]">当前 Super 大小:</span>
                      <span className="text-[#111111]">
                        {(plan.current_super_bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB
                      </span>
                    </div>
                    <div className="flex justify-between font-mono">
                      <span className="text-[#6b7280]">目标 Super 大小:</span>
                      <span className="font-bold text-[#10b981]">
                        {(plan.target_bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB
                      </span>
                    </div>
                    <div className="flex justify-between font-mono">
                      <span className="text-[#6b7280]">Userdata 扇区范围变更:</span>
                      <span className="text-[#374151]">
                        {plan.userdata_info.first} → {plan.new_userdata_first}
                      </span>
                    </div>
                  </div>

                  <div>
                    <label className="block text-[12px] font-medium text-[#6b7280] mb-1.5">
                      输入确认词 <span className="font-mono font-bold text-[#111111] bg-[#f5f5f5] px-1.5 py-0.5 rounded border border-[#e5e7eb]">{targetConfirm}</span> 开始执行扩容：
                    </label>
                    <input
                      type="text"
                      value={confirmWord}
                      onChange={(e) => setConfirmWord(e.target.value)}
                      placeholder={targetConfirm}
                      className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[14px] font-mono text-[#111111] focus:outline-none transition-colors"
                    />
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="space-y-4">
              <div className="bg-[#fffbeb] border border-[#fef3c7] rounded-[8px] p-3 text-[12px] text-[#b45309] font-medium leading-relaxed">
                恢复官方 GPT 分区表仅接受本机扩容时自动生成的历史备份。恢复后必须立即刷官方完整线刷包，禁止直接启动旧系统。
              </div>

              <div>
                <label className="block text-[12px] font-semibold text-[#111111] mb-1.5">
                  选择扩容前生成的 GPT 备份：
                </label>
                {backups.length > 0 ? (
                  <select
                    value={selectedBackup}
                    onChange={(e) => setSelectedBackup(e.target.value)}
                    className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[12px] font-mono text-[#111111] focus:outline-none cursor-pointer"
                  >
                    {backups.map((b) => (
                      <option key={b} value={b}>
                        {b.split(/[/\\]/).pop()}
                      </option>
                    ))}
                  </select>
                ) : (
                  <div className="p-3 text-center text-xs text-[#898989] bg-[#f8f9fa] rounded-[8px] border border-[#e5e7eb]">
                    未在 backup/ 目录下找到 GPT 备份
                  </div>
                )}
              </div>

              {selectedBackup && (
                <div>
                  <label className="block text-[12px] font-medium text-[#6b7280] mb-1.5">
                    输入确认词 <span className="font-mono font-bold text-[#111111] bg-[#f5f5f5] px-1.5 py-0.5 rounded border border-[#e5e7eb]">{targetRestoreConfirm}</span> 确认恢复：
                  </label>
                  <input
                    type="text"
                    value={restoreConfirmWord}
                    onChange={(e) => setRestoreConfirmWord(e.target.value)}
                    placeholder={targetRestoreConfirm}
                    className="w-full h-[40px] px-3.5 bg-white border border-[#e5e7eb] focus:border-[#111111] rounded-[8px] text-[14px] font-mono text-[#111111] focus:outline-none transition-colors"
                  />
                </div>
              )}
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t border-[#f3f4f6]">
          <button
            onClick={onClose}
            className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-[#111111] bg-white border border-[#e5e7eb] hover:bg-[#f8f9fa] transition-all cursor-pointer"
          >
            取消
          </button>
          {activeSubTab === 'expand' ? (
            <button
              onClick={() => onApplyExpand(confirmWord)}
              disabled={!canApplyExpand || isProcessing}
              className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-white bg-[#ef4444] hover:bg-[#dc2626] active:bg-[#b91c1c] transition-all cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed"
            >
              {isProcessing ? '正在重写 GPT…' : '确认物理扩容 (清空数据)'}
            </button>
          ) : (
            <button
              onClick={() => onRestoreBackup(selectedBackup, restoreConfirmWord)}
              disabled={!canApplyRestore || isProcessing}
              className="h-[40px] px-5 rounded-[8px] text-[14px] font-semibold text-white bg-[#ef4444] hover:bg-[#dc2626] active:bg-[#b91c1c] transition-all cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed"
            >
              {isProcessing ? '正在恢复 GPT…' : '确认恢复官方分区'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
