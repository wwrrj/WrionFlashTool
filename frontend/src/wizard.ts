/** expand（扩容 / 恢复）和 idtool（刷入 ID）是独立的工具页，不属于线性流程。 */
export type Step = 'welcome' | 'package' | 'device' | 'plan' | 'review' | 'run' | 'done' | 'expand' | 'idtool';
export type FlashMode = 'clean' | 'update';

export interface FlashOptions {
  mode: FlashMode | null;
  idSource: 'package' | 'external';
  idPath: string;
  rootEnabled: boolean;
  rootPath: string;
  rootTarget: 'init_boot' | 'boot';
  reboot: boolean;
}

export const DEFAULT_OPTIONS: FlashOptions = {
  mode: null,
  idSource: 'package',
  idPath: '',
  rootEnabled: false,
  rootPath: '',
  rootTarget: 'init_boot',
  reboot: true,
};

/** 顶部进度条上的 5 个大步骤；review/run 同属“开始刷入”。 */
export const WIZARD_STEPS: { id: Step[]; label: string }[] = [
  { id: ['package'], label: '刷机包' },
  { id: ['device'], label: '连接手机' },
  { id: ['plan'], label: '选择方式' },
  { id: ['review', 'run'], label: '开始刷入' },
  { id: ['done'], label: '完成' },
];

export const FLASH_STAGES = [
  { name: '安全预检', hint: '检查机型、容量和防回滚' },
  { name: '写入固件', hint: '更新底层固件' },
  { name: '写入启动链', hint: '写入 boot / vbmeta 等' },
  { name: '写入系统分区', hint: '耗时最长，请耐心等待' },
  { name: '处理用户数据', hint: '按所选方式清理或保留' },
  { name: '完成并重启', hint: '收尾并重启手机' },
];
