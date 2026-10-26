const read = (key: string) => {
  try { return localStorage.getItem(key); } catch { return null; }
};
const write = (key: string, value: string) => {
  try { localStorage.setItem(key, value); } catch { /* 本机存储不可用时忽略 */ }
};
/** 先读新键，没有时读改名前（harusora.*）留下的旧键。 */
const readWithLegacy = (key: string) => read(`wrion.${key}`) ?? read(`harusora.${key}`);

export const storage = {
  agreed: () => readWithLegacy('usage-agreed') === 'yes',
  setAgreed: () => write('wrion.usage-agreed', 'yes'),
  /** 上次使用的刷机包：.zip 文件或已解压的目录。 */
  lastPackage: () => readWithLegacy('last-package') ?? read('harusora.last-zip'),
  setLastPackage: (path: string) => write('wrion.last-package', path),
  /** 用户选择“跳过此版本”的版本号，同一版本不再自动弹出更新提示。 */
  skippedVersion: () => read('wrion.skipped-version'),
  setSkippedVersion: (version: string) => write('wrion.skipped-version', version),
};
