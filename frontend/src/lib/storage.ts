const read = (key: string) => {
  try { return localStorage.getItem(key); } catch { return null; }
};
const write = (key: string, value: string) => {
  try { localStorage.setItem(key, value); } catch { /* 本机存储不可用时忽略 */ }
};

export const storage = {
  agreed: () => read('harusora.usage-agreed') === 'yes',
  setAgreed: () => write('harusora.usage-agreed', 'yes'),
  /** 上次使用的刷机包：.zip 文件或已解压的目录。 */
  lastPackage: () => read('harusora.last-package') ?? read('harusora.last-zip'),
  setLastPackage: (path: string) => write('harusora.last-package', path),
};
