import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../lib/api';
import { errorMessage } from '../lib/format';
import type { UsbDriverInstallResult, UsbDriverStatus } from '../types';

/**
 * Android USB 驱动状态。active 时每隔一段时间在后台检测
 * “手机插上了但缺驱动”（查询较慢，只在找不到手机时运行）。
 */
export function useUsbDriver(active: boolean) {
  const [status, setStatus] = useState<UsbDriverStatus | null>(null);
  const [installing, setInstalling] = useState(false);
  const [result, setResult] = useState<UsbDriverInstallResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try { setStatus(await api.usbDriverStatus()); } catch { /* 检测失败不影响刷机 */ } finally { inFlight.current = false; }
  }, []);

  useEffect(() => {
    if (!active) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10000);
    return () => window.clearInterval(timer);
  }, [active, refresh]);

  const install = useCallback(async () => {
    setInstalling(true);
    setError(null);
    setResult(null);
    try {
      setResult(await api.installUsbDriver());
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInstalling(false);
    }
  }, [refresh]);

  return { status, installing, result, error, install };
}
