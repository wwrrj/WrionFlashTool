export const GIB = 1073741824;

export const formatGiB = (bytes?: number | null) =>
  typeof bytes === 'number' && bytes > 0 ? `${(bytes / GIB).toFixed(1)} GiB` : '—';

export const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export const shortSerial = (serial?: string) =>
  !serial ? '未选择设备' : serial.length > 22 ? `${serial.slice(0, 10)}…${serial.slice(-8)}` : serial;

export const baseName = (path: string) => path.split(/[/\\]/).pop() || path;

export const MODE_LABELS: Record<string, string> = {
  fastbootd: 'Fastbootd',
  fastboot: 'Fastboot',
  device: '已开机',
  recovery: 'Recovery',
  sideload: 'Sideload',
};

export const modeLabel = (mode?: string) => (mode ? MODE_LABELS[mode] ?? mode : '未连接');
