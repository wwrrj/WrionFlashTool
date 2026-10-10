import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  BackupInfo, DeviceSummary, EnvReport, ExpandPlan, ExpandProgressEvent, FlashProfile, PackageInfo,
} from '../types';

export const isTauri = () =>
  typeof window !== 'undefined' && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

const wait = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** 浏览器预览（vite dev）里使用的演示数据，方便不打包也能看界面。 */
const previewProfile: FlashProfile = {
  rom_root: 'D:\\Preview\\ROM', profile_path: '', codenames: ['shennong', 'shennongpro'],
  package_name: 'ColorOS 16 Porting for Xiaomi 14 Pro · Vanilla', display_name: 'Xiaomi 14 Pro', notes: ['刷入前请备份重要资料。', '刷入过程中不要断开 USB。'],
  author: 'Harusora Team', qq_group: '2858594753', build_date: '2026-09-19', minimum_super_bytes: 0,
  erase_if_present: [], pre_erase: [], images_dir: '', super_dir: '', work_dir: '', logs_dir: '', tools_dir: '',
  firmware: [], boot_chain: [], final_data: [], slot_suffix: '', dynamic_partitions: [{ name: 'mi_ext', dat_bytes: 0 }],
  flash_crc_lists: false, crc_list_file: '', sparse_crc_list_file: '', require_wipe_confirmation: true,
  confirmation_word: 'WIPE', update_confirmation_word: 'UPDATE', reboot_after_success: true,
  enable_root_menu: true, post_flash_id: true,
  expansion: {
    enabled: true, codenames: ['shennong'], target_bytes: 17179869184, disk: '/dev/block/sda', super_part: 32,
    userdata_part: 33, backup_dir: 'D:\\Preview\\ROM\\backup', sgdisk_dir: '', remote_sgdisk_dir: '',
    confirm_word: 'EXPAND', restore_confirm_word: 'RESTORE',
  },
  kind: 'harusora',
};

/** 预览用的小米官方线刷包（?preview=official）。 */
const previewOfficialProfile: FlashProfile = {
  ...previewProfile,
  package_name: '小米官方线刷包 OS2.0.221.0.VNBCNXM', display_name: 'shennong', author: 'Xiaomi 官方', qq_group: '', build_date: '2025-09-28',
  notes: ['这是小米官方线刷包。工具会逐条解析并执行包内 flash_all 脚本中的命令，遇到无法识别的内容会直接拒绝。', '工具不会执行 flash_all_lock 或任何锁定 Bootloader 的命令。'],
  dynamic_partitions: [], post_flash_id: false, expansion: undefined, kind: 'official',
  official: { device: 'shennong', build_number: 'OS2.0.221.0.VNBCNXM', clean_script: 'flash_all.bat', keep_data_script: 'flash_all_except_storage.bat' },
};

const previewBackups: BackupInfo[] = [
  { path: 'D:\\Preview\\ROM\\backup\\shennong-super-expand-20260920-101500.zip', file_name: 'shennong-super-expand-20260920-101500.zip', kind: 'zip', device: 'shennong', disk: '/dev/block/sda', created: '2026-09-20 10:15', super_old_bytes: 10737418240, super_new_bytes: 17179869184, supported: true, problems: [], warnings: [] },
  { path: 'D:\\Downloads\\houji-super-expand-20260901-090000.zip', file_name: 'houji-super-expand-20260901-090000.zip', kind: 'zip', device: 'houji', disk: '/dev/block/sda', created: '2026-09-01 09:00', super_old_bytes: 10737418240, super_new_bytes: 17179869184, supported: false, problems: ['备份来自 houji，当前刷机包支持 shennong'], warnings: [] },
];

export const PREVIEW_EXPAND_EVENT = 'preview:expand_progress';

/** 浏览器预览：逐步派发扩容进度事件，模拟真实流程。 */
async function simulateExpand(phase: ExpandProgressEvent['phase'], steps: string[]) {
  const emit = (current: number, detail?: string) =>
    window.dispatchEvent(new CustomEvent<ExpandProgressEvent>(PREVIEW_EXPAND_EVENT, { detail: { phase, steps, current, detail } }));
  for (let i = 0; i < steps.length; i++) {
    emit(i);
    await wait(500);
    if (steps[i].startsWith('等待')) { emit(i, '等待手机进入 Recovery… 已等待 3 秒'); await wait(500); }
  }
  emit(steps.length);
}

const previewPlan: ExpandPlan = {
  disk: '/dev/block/sda', sector_bytes: 4096, disk_sectors: 0, last_usable_sector: 0, target_bytes: 17179869184, target_sectors: 0,
  super_info: { number: 32, code: '', guid: '', first: 0, last: 0, name: 'super' },
  userdata_info: { number: 33, code: '', guid: '', first: 0, last: 0, name: 'userdata' },
  current_super_bytes: 8321499136, new_super_last: 0, new_userdata_first: 0, new_userdata_bytes: 210 * 1073741824, super_part: 32, userdata_part: 33,
};

/** 预览模式下“已解压”的状态；地址栏带 ?preview=ready 时直接视为已就绪。 */
const previewParam = typeof window !== 'undefined' ? new URLSearchParams(window.location.search).get('preview') : null;
let previewReady = previewParam === 'ready' || previewParam === 'official';

export const api = {
  getProfile: (): Promise<FlashProfile> => {
    if (isTauri()) return invoke('get_profile', {});
    return previewReady ? Promise.resolve(previewParam === 'official' ? previewOfficialProfile : previewProfile) : Promise.reject(new Error('未找到刷机配置（浏览器预览）'));
  },

  scanDevices: async (): Promise<DeviceSummary[]> => {
    if (isTauri()) return invoke('scan_devices', {});
    await wait(300);
    return [{ serial: 'PREVIEW0123456789', mode: 'fastboot', product: 'shennong' }];
  },

  inspectPackage: async (zipPath: string): Promise<PackageInfo> => {
    if (isTauri()) return invoke('inspect_package_cmd', { zipPath });
    await wait(400);
    return {
      zip_path: zipPath, package_name: 'ColorOS 16 Porting for Xiaomi 14 Pro · Vanilla', display_name: 'Xiaomi 14 Pro',
      codenames: ['shennong', 'shennongpro'], total_bytes: 10453863473, file_count: 80,
      default_dest: 'E:\\rom\\Xiaomi14Pro_Vanilla', already_extracted: false, free_bytes: 120 * 1073741824,
    };
  },

  extractPackage: async (zipPath: string, destDir: string): Promise<string> => {
    if (isTauri()) return invoke('extract_package_cmd', { zipPath, destDir });
    await wait(800);
    previewReady = true;
    return destDir;
  },
  /** 直接使用已解压的 ROM 目录，返回识别出的 ROM 根目录。 */
  useRomDir: async (dir: string): Promise<string> => {
    if (isTauri()) return invoke('use_rom_dir_cmd', { dir });
    await wait(300);
    previewReady = true;
    return dir;
  },
  cancelExtract: (): Promise<void> => (isTauri() ? invoke('cancel_extract_cmd') : Promise.resolve()),
  destFreeSpace: (dest: string): Promise<number | null> =>
    isTauri() ? invoke('dest_free_space_cmd', { dest }) : Promise.resolve(null),

  diagnostics: async (chosenSerial: string | null): Promise<EnvReport> => {
    if (isTauri()) return invoke('run_diagnostics_cmd', { chosenSerial });
    await wait(600);
    // 预览时地址栏带 ?super=short 可模拟 super 容量不足
    const short = new URLSearchParams(window.location.search).get('super') === 'short';
    const superBytes = short ? 8321499136 : 17179869184;
    return { serial: 'preview', product: 'shennong', super_bytes: superBytes, super_required: 17179869184, super_ready: !short, is_userspace: false, anti_ok: true };
  },
  runFlash: async (args: { chosenSerial: string | null; preserveData: boolean; confirmWord: string; reboot: boolean }): Promise<string> => {
    if (isTauri()) return invoke('run_flash_cmd', args);
    await wait(1200);
    return 'preview.log';
  },
  runIdFlash: (args: { chosenSerial: string | null; idFile: string; reboot: boolean }): Promise<string> =>
    invoke('run_id_flash_cmd', { ...args, formatData: false, confirmWord: 'ID' }),
  runRootFlash: (args: { chosenSerial: string | null; imagePath: string; target: string; reboot: boolean }): Promise<string> =>
    invoke('run_root_flash_cmd', { ...args, confirmWord: 'ROOT' }),

  expandPlan: async (): Promise<ExpandPlan> => {
    if (isTauri()) return invoke('get_expand_plan_cmd', {});
    await simulateExpand('plan', ['连接手机', '刷入包内 Recovery', '等待进入 Recovery', '准备分区工具', '读取分区表']);
    return previewPlan;
  },
  applyExpand: async (confirmWord: string): Promise<string> => {
    if (isTauri()) return invoke('apply_expand_cmd', { confirmWord });
    await simulateExpand('apply', ['连接手机', '刷入包内 Recovery', '等待进入 Recovery', '准备分区工具', '读取分区表', '备份分区表', '清除数据', '重写分区表', '格式化 userdata']);
    return 'D:\\Preview\\ROM\\backup\\shennong-super-expand-20261010-170000.zip';
  },
  listBackups: (): Promise<BackupInfo[]> => (isTauri() ? invoke('list_backups_cmd', {}) : Promise.resolve(previewBackups)),
  inspectBackup: (backupPath: string): Promise<BackupInfo> => invoke('inspect_backup_cmd', { backupPath }),
  restoreBackup: async (backupPath: string, confirmWord: string): Promise<string> => {
    if (isTauri()) return invoke('restore_backup_cmd', { backupPath, confirmWord });
    await simulateExpand('restore', ['检查备份', '连接手机', '刷入包内 Recovery', '等待进入 Recovery', '准备分区工具', '写入官方分区表', '核对分区范围', '清除数据']);
    return '官方分区表恢复成功';
  },
  openFolder: (path: string): Promise<void> => invoke('open_folder_cmd', { path }),
};

export async function pickFile(name: string, extensions: string[]): Promise<string | null> {
  if (!isTauri()) return null;
  const picked = await open({ multiple: false, directory: false, filters: [{ name, extensions }] });
  return typeof picked === 'string' ? picked : null;
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) return null;
  const picked = await open({ multiple: false, directory: true });
  return typeof picked === 'string' ? picked : null;
}
