import { ShieldAlert } from 'lucide-react';
import { PageFrame } from '../components/PageFrame';
import { ProgressBar } from '../components/ProgressBar';
import { LogTail, StepTimeline } from '../components/StepTimeline';
import { FLASH_STAGES } from '../wizard';
import type { FlashStageEvent, PartitionProgressEvent } from '../types';

interface RunPageProps {
  preparing: boolean;
  extraLabel: string | null;
  stage: FlashStageEvent | null;
  partition: PartitionProgressEvent | null;
  logs: string[];
  onShowLogs: () => void;
}

export function RunPage({ preparing, extraLabel, stage, partition, logs, onShowLogs }: RunPageProps) {
  const current = stage?.stage ?? 0;
  const total = FLASH_STAGES.length;
  const sub = partition && partition.percent > 0 && partition.percent < 100 ? partition.percent / 100 : 0;
  const finished = current === total;
  const percent = preparing ? 2 : finished ? 100 : Math.max(3, ((Math.max(current - 1, 0) + sub) / total) * 100);

  const title = extraLabel ?? (preparing ? '正在检查手机…' : '正在刷入，请耐心等待');

  return (
    <PageFrame stepNo={4} title={title} description="整个过程通常需要十几分钟，取决于电脑和数据线速度。">
      <div className="callout warn strong"><ShieldAlert size={17} /><span><b>请勿拔出数据线，也不要操作手机。</b>屏幕出现 Fastboot 或重启都是正常现象。</span></div>
      <section className="card">
        <ProgressBar percent={percent} label={preparing ? '设备预检' : stage?.title || FLASH_STAGES[Math.max(0, current - 1)].name} />
        <StepTimeline
          steps={FLASH_STAGES}
          current={preparing ? -1 : finished ? total : current - 1}
          detail={current === 4 && partition && sub > 0 ? `${partition.name} ${partition.percent}%` : null}
        />
      </section>
      <LogTail logs={logs} onShowLogs={onShowLogs} />
    </PageFrame>
  );
}
