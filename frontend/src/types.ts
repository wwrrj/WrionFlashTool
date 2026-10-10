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
  phase: 'plan' | 'apply' | 'restore';
  steps: string[];
  /** 当前步骤序号（从 0 开始）；等于 steps.length 表示全部完成。 */
  current: number;
  detail?: string | null;
}
