export interface ImageOp {
  action: string;
  partition: string;
  file?: string;
}

export interface DynamicPartition {
  name: string;
  dat_bytes: number;
  pad_to_bytes?: number;
}

export interface ExpansionConfig {
  enabled: boolean;
  codenames: string[];
  target_bytes: number;
  disk: string;
  super_part: number;
  userdata_part: number;
  backup_dir: string;
  sgdisk_dir: string;
  remote_sgdisk_dir: string;
  confirm_word: string;
  restore_confirm_word: string;
}

export interface FlashProfile {
  rom_root: string;
  profile_path: string;
  codenames: string[];
  package_name: string;
  display_name: string;
  notes: string[];
  author: string;
  qq_group: string;
  build_date: string;
  minimum_super_bytes: number;
  anti_version_file?: string;
  erase_if_present: string[];
  pre_erase: string[];
  images_dir: string;
  super_dir: string;
  work_dir: string;
  logs_dir: string;
  expansion_dir?: string;
  tools_dir: string;
  firmware: ImageOp[];
  boot_chain: ImageOp[];
  final_data: ImageOp[];
  wipe_super_image?: string;
  wipe_super_dat?: string;
  slot_suffix: string;
  dynamic_partitions: DynamicPartition[];
  set_active_slot?: string;
  flash_crc_lists: boolean;
  crc_list_file: string;
  sparse_crc_list_file: string;
  require_wipe_confirmation: boolean;
  confirmation_word: string;
  update_confirmation_word: string;
  reboot_after_success: boolean;
  enable_root_menu: boolean;
  post_flash_id: boolean;
  expansion?: ExpansionConfig;
  /** profile：FlashProfile 刷机包（flash_profile.toml 驱动）；official：小米官方线刷包（按 flash_all 脚本刷入）。 */
  kind: 'profile' | 'official';
  official?: OfficialInfo | null;
}

export interface DeviceSummary {
  serial: string;
  mode: string;
  product?: string;
}

export interface EnvReport {
  serial: string;
  product: string;
  super_bytes: number;
  super_required: number;
  super_ready: boolean;
  is_userspace: boolean;
  anti?: string | null;
  anti_package?: number | null;
  anti_ok: boolean;
}

export interface PartInfo {
  number: number;
  code: string;
  guid: string;
  first: number;
  last: number;
  name: string;
}

export interface ExpandPlan {
  disk: string;
  sector_bytes: number;
  disk_sectors: number;
  last_usable_sector: number;
  target_bytes: number;
  target_sectors: number;
  super_info: PartInfo;
  userdata_info: PartInfo;
  current_super_bytes: number;
  new_super_last: number;
  new_userdata_first: number;
  new_userdata_bytes: number;
  super_part: number;
  userdata_part: number;
}

export interface FlashStageEvent {
  stage: number;
  total: number;
  title: string;
}

export interface PartitionProgressEvent {
  name: string;
  done: number;
  total: number;
  percent: number;
}

export interface PackageInfo {
  zip_path: string;
  package_name: string;
  display_name: string;
  codenames: string[];
  total_bytes: number;
  file_count: number;
  default_dest: string;
  already_extracted: boolean;
  free_bytes?: number | null;
}

export interface ExtractProgressEvent {
  done_bytes: number;
  total_bytes: number;
  file_index: number;
  file_count: number;
  current: string;
}

export interface BackupInfo {
  path: string;
  file_name: string;
  kind: 'zip' | 'dir';
  device?: string | null;
  disk?: string | null;
  created?: string | null;
  super_old_bytes?: number | null;
  super_new_bytes?: number | null;
  supported: boolean;
  problems: string[];
  warnings: string[];
}

export interface ExpandProgressEvent {
  phase: 'plan' | 'apply' | 'restore' | 'id';
  steps: string[];
  /** 当前步骤序号（从 0 开始）；等于 steps.length 表示全部完成。 */
  current: number;
  detail?: string | null;
}

export interface OfficialInfo {
  device: string;
  build_number: string;
  /** 完整线刷脚本（flash_all.bat），没有时为空 */
  clean_script?: string | null;
  /** 保留数据的脚本（flash_all_except_storage.bat），没有或不安全时为空 */
  keep_data_script?: string | null;
}

/** 官方包逐条执行脚本时的进度。 */
export interface FlashStepEvent {
  index: number;
  total: number;
  label: string;
}

export interface UsbDriverStatus {
  /** 系统里已有内置的 Android USB 驱动 */
  installed: boolean;
  /** 已连接但缺少驱动的 Android 接口 */
  missing_devices: string[];
}

export interface UsbDriverInstallResult {
  reboot_required: boolean;
  message: string;
}

/** 刷入类命令的结果。 */
export interface FlashOutcome {
  log_path: string;
  /** 手机是否已收到重启命令（关闭自动重启或重启失败时为 false） */
  rebooted: boolean;
}

/** 检查更新的结果（来自网站上的 version.json）。 */
export interface UpdateInfo {
  current: string;
  latest: string;
  available: boolean;
  /** 当前版本低于 min_version，必须更新后才能继续使用 */
  required: boolean;
  date: string;
  notes: string[];
  download_url: string;
}

/** 读取主板 ID 的方式：普通 ADB Shell，或通过 su 获取 Root 权限。 */
export type BoardIdMethod = 'shell' | 'root';

/** 读取失败时界面上可以一键执行的解决操作。 */
export type BoardIdFix = 'retry_root' | 'install_driver' | 'restart_adb' | 'reboot_system';

export interface BoardIdCheck {
  label: string;
  ok: boolean;
  detail?: string | null;
}

export interface BoardIdProblem {
  /** 错误类型，例如 unauthorized、permission_denied、no_device */
  code: string;
  title: string;
  detail: string;
  steps: string[];
  actions: BoardIdFix[];
  /** 出错命令的原始输出 */
  raw?: string | null;
}

export interface BoardInfo {
  /** 节点原始内容（十进制） */
  serial_number: string;
  hex: string;
  node: string;
  adb_serial: string;
  /** device（系统）或 recovery */
  mode: string;
  model?: string | null;
  device?: string | null;
  via_root: boolean;
}

/** 读取主板 ID 的结果：成功时 info 有值，失败时 problem 说明卡在哪一步、怎么解决。 */
export interface BoardIdOutcome {
  checks: BoardIdCheck[];
  devices: { serial: string; state: string }[];
  info?: BoardInfo | null;
  problem?: BoardIdProblem | null;
}
