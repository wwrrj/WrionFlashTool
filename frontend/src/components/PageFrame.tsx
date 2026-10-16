import type { ReactNode } from 'react';

interface PageFrameProps {
  /** 第几大步（1–5），用于标题上方的“第 N 步”提示。 */
  stepNo?: number;
  title: string;
  description?: string;
  children: ReactNode;
  /** 左下角次要操作（返回等）。 */
  secondary?: ReactNode;
  /** 右下角主操作。 */
  primary?: ReactNode;
  /** 底栏提示文字，例如为什么不能继续。 */
  hint?: ReactNode;
}

export function PageFrame({ stepNo, title, description, children, secondary, primary, hint }: PageFrameProps) {
  return (
    <main className="page">
      <div className="page-scroll">
        <div className="page-inner">
          <div className="page-head">{stepNo && <span className="eyebrow">第 {stepNo} 步 · 共 5 步</span>}<h1>{title}</h1>{description && <p>{description}</p>}</div>
          {children}
        </div>
      </div>
      {(secondary || primary || hint) && (
        <footer className="page-foot">
          <div className="foot-left">{secondary}</div>
          <div className="foot-right">{hint && <span className="foot-hint">{hint}</span>}{primary}</div>
        </footer>
      )}
    </main>
  );
}
