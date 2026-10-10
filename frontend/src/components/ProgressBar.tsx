export function ProgressBar({ percent, label, detail }: { percent: number; label?: string; detail?: string }) {
  const value = Math.max(0, Math.min(100, percent));
  return (
    <div className="progress">
      {(label || detail) && <div className="progress-meta"><span>{label}</span><b>{detail ?? `${Math.floor(value)}%`}</b></div>}
      <div className="progress-track" role="progressbar" aria-valuenow={Math.floor(value)} aria-valuemin={0} aria-valuemax={100}>
        <span style={{ width: `${value}%` }} />
      </div>
    </div>
  );
}
