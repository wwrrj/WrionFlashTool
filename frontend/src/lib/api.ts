import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { FLASH_STAGES } from '../wizard';
import type {
  BackupInfo, DeviceSummary, FlashOutcome, UsbDriverInstallResult, UsbDriverStatus, EnvReport, ExpandPlan, ExpandProgressEvent, ExtractProgressEvent, FlashProfile,
  FlashStageEvent, PackageInfo, PartitionProgressEvent,
} from '../types';

export const isTauri = () =>
  typeof window !== 'undefined' && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

const wait = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** 浏览器预览（vite dev）里使用的演示数据，方便不打包也能看界面。 */
const previewProfile: FlashProfile = {
  rom_root: 'D:\\Preview\\ROM', profile_path: '', codenames: ['shennong', 'shennongpro'],
  package_name: 'ColorOS 16 Porting for Xiaomi 14 Pro · Vanilla', display_name: 'Xiaomi 14 Pro', notes: ['刷入前请备份重要资料。', '刷入过程中不要断开 USB。'],
  author: 'Wrion Team', qq_group: '', build_date: '2026-09-19', minimum_super_bytes: 0,
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
  kind: 'profile',
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

/** 浏览器预览：以 `preview:<后端事件名>` 派发窗口事件，代替 Tauri 的 listen。 */
export const emitPreview = <T,>(name: string, payload: T) =>
  window.dispatchEvent(new CustomEvent<T>(`preview:${name}`, { detail: payload }));

export function onPreview<T>(name: string, handler: (payload: T) => void) {
  const listener = (e: Event) => handler((e as CustomEvent<T>).detail);
  window.addEventListener(`preview:${name}`, listener);
  return () => window.removeEventListener(`preview:${name}`, listener);
}

/** 浏览器预览：模拟刷入的阶段、分区转换进度和日志。 */
async function simulateFlash(preserveData: boolean, reboot: boolean) {
  const titles = FLASH_STAGES.map((s) => s.name);
  const stage = (n: number) => {
    emitPreview<FlashStageEvent>('flash_stage', { stage: n, total: titles.length, title: titles[n - 1] });
    emitPreview('log_line', `--- 阶段 ${n}/${titles.length}：${titles[n - 1]}`);
  };
  const log = (line: string) => emitPreview('log_line', line);
  stage(1);
  log('[检查] shennong · 防回滚 1 / 刷机包 1');
  await wait(700);
  stage(2);
  for (const part of ['xbl', 'xbl_config', 'abl', 'tz', 'hyp', 'modem']) {
    log(`fastboot flash ${part}_ab images/${part}.img`);
    await wait(160);
    log(`Writing '${part}_ab' OKAY`);
  }
  stage(3);
  for (const part of ['boot', 'init_boot', 'vendor_boot', 'dtbo', 'vbmeta', 'vbmeta_system']) {
    log(`fastboot flash ${part}_ab images/${part}.img`);
    await wait(140);
  }
  stage(4);
  for (const name of ['system', 'system_ext', 'product', 'vendor', 'odm', 'mi_ext']) {
    for (let percent = 10; percent <= 100; percent += 15) {
      emitPreview<PartitionProgressEvent>('partition_progress', { name, done: percent, total: 100, percent: Math.min(percent, 100) });
      await wait(90);
    }
    log(`[super] ${name} 写入完成`);
  }
  stage(5);
  log(preserveData ? '[userdata] 保留用户数据' : 'fastboot erase userdata OKAY');
  await wait(600);
  stage(6);
  log(reboot ? 'fastboot reboot OKAY' : '[提示] 已关闭自动重启');
  await wait(500);
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
    // 预览时地址栏带 ?devices=none 可模拟没有连接手机
    if (new URLSearchParams(window.location.search).get('devices') === 'none') return [];
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
    const files = ['images/xbl.img', 'images/boot.img', 'images/super/system.new.dat', 'images/super/product.new.dat', 'images/super/vendor.new.dat'];
    const total = 10453863473;
    for (let i = 0; i <= 20; i++) {
      const fileIndex = Math.min(Math.floor((i / 20) * files.length), files.length - 1);
      emitPreview<ExtractProgressEvent>('extract_progress', { done_bytes: (total * i) / 20, total_bytes: total, file_index: fileIndex, file_count: 80, current: files[fileIndex] });
      await wait(80);
    }
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
  usbDriverStatus: async (): Promise<UsbDriverStatus> => {
    if (isTauri()) return invoke('usb_driver_status_cmd');
    // 预览时地址栏带 ?driver=missing 可模拟缺少驱动
    const missing = new URLSearchParams(window.location.search).get('driver') === 'missing';
    return { installed: !missing, missing_devices: missing ? ['Android'] : [] };
  },
  installUsbDriver: async (): Promise<UsbDriverInstallResult> => {
    if (isTauri()) return invoke('install_usb_driver_cmd');
    await wait(800);
    return { reboot_required: false, message: 'USB 驱动已安装' };
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
  runFlash: async (args: { chosenSerial: string | null; preserveData: boolean; confirmWord: string; reboot: boolean }): Promise<FlashOutcome> => {
    if (isTauri()) return invoke('run_flash_cmd', args);
    await simulateFlash(args.preserveData, args.reboot);
    return { log_path: 'preview.log', rebooted: args.reboot };
  },
  runIdFlash: async (args: { chosenSerial: string | null; idFile: string; reboot: boolean; formatData?: boolean }): Promise<FlashOutcome> => {
    if (isTauri()) return invoke('run_id_flash_cmd', { formatData: false, ...args, confirmWord: 'ID' });
    const steps = ['连接手机', '校验机型', '进入 Fastbootd', '刷入 mi_ext', ...(args.formatData ? ['清除数据'] : []), ...(args.reboot ? ['重启手机'] : [])];
    await simulateExpand('id', steps);
    return { log_path: 'preview-id.log', rebooted: args.reboot };
  },
  /** 刷机包目录里文件名像 ID 补丁的文件。 */
  idCandidates: (): Promise<string[]> =>
    isTauri() ? invoke('id_candidates_cmd', {}) : Promise.resolve(['E:\\latest=rom\\shennong_ID_mi_ext.zip']),
  runRootFlash: (args: { chosenSerial: string | null; imagePath: string; target: string; reboot: boolean }): Promise<FlashOutcome> =>
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
  // 浏览器预览没有系统文件对话框，返回演示路径以便走完流程
  if (!isTauri()) return extensions.includes('zip') ? 'D:\\Downloads\\Xiaomi14Pro_Vanilla.zip' : 'D:\\Downloads\\magisk_patched.img';
  const picked = await open({ multiple: false, directory: false, filters: [{ name, extensions }] });
  return typeof picked === 'string' ? picked : null;
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) return 'E:\\rom\\Xiaomi14Pro_Vanilla';
  const picked = await open({ multiple: false, directory: true });
  return typeof picked === 'string' ? picked : null;
}
