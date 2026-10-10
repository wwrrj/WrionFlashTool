import { useCallback, useEffect, useRef, useState } from 'react';
import { api, isTauri } from '../lib/api';
import type { DeviceSummary } from '../types';

/** 设备列表。active 为 true 时每隔几秒自动刷新，用户不需要手动点刷新。 */
export function useDevices(enabled: boolean, active: boolean) {
  const [devices, setDevices] = useState<DeviceSummary[]>([]);
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const inFlight = useRef(false);

  const scan = useCallback(async () => {
    if (!enabled || inFlight.current) return;
    inFlight.current = true;
    setScanning(true);
    try {
      const next = await api.scanDevices();
      setDevices(next);
      setScanError(null);
      setSelectedSerial((prev) => (prev && next.some((d) => d.serial === prev) ? prev : next[0]?.serial ?? null));
    } catch (e) {
      setScanError(e instanceof Error ? e.message : String(e));
    } finally {
      inFlight.current = false;
      setScanning(false);
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled || !active) return;
    void scan();
    if (!isTauri()) return;
    const timer = window.setInterval(() => void scan(), 4000);
    return () => window.clearInterval(timer);
  }, [enabled, active, scan]);

  const current = devices.find((d) => d.serial === selectedSerial) ?? devices[0];
  return { devices, current, selectedSerial, setSelectedSerial, scanning, scanError, scan };
}
