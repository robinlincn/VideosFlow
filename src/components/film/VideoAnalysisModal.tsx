// 影片视频分析进度弹窗（M2.6）
// 确认视频范围后触发：经多模态文字大模型分析导入影片，十步进度实时展示。

import { useState, useEffect } from 'react';
import type { ProgressMsg } from '../../ipc/types';

interface Props {
  open: boolean;
  /** 最新进度消息（来自 Channel） */
  progress: ProgressMsg | null;
  /** 当前进行到第几步（1-10，由 payload.step 决定） */
  currentStep: number; // 0 表示未开始
  failed: boolean;
  failReason: string;
  /** 最终报告（step=10 的 payload.report），markdown 文本 */
  report: string | null;
  onContinue: () => void;
  onClose: () => void;
}

// 十步固定标签（与后端 run_film_video_analysis 对齐）
export const ANALYSIS_STEPS: string[] = [
  '① 提取视频帧',
  '② 检测场景切换点',
  '③ 多维度特征编码中',
  '④ 深度语义理解中',
  '⑤ 语义块解析中',
  '⑥ 深度语义理解中',
  '⑦ 叙事结构生成中',
  '⑧ 解说词生成中',
  '⑨ 输出流水线生成中',
  '⑩ 最终影片分析内容总结报告',
];

export default function VideoAnalysisModal({
  open,
  progress,
  currentStep,
  failed,
  failReason,
  report,
  onContinue,
  onClose,
}: Props) {
  const [copied, setCopied] = useState(false);

  useEffect(() => { setCopied(false); }, [report]);

  if (!open) return null;

  const overall = progress?.progress ?? 0;
  const status = progress?.status ?? 'running';
  const done = status === 'done';
  const activeDetail = progress?.message ?? '';

  const copyReport = async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(report);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* noop */ }
  };

  return (
    <div className="narrative-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget && (done || failed)) onClose(); }}>
      <div className="narrative-modal export-modal video-analysis-modal" tabIndex={0} autoFocus>
        <div className="narrative-modal__header">
          <h3>🎞 影片视频分析</h3>
          {(done || failed) ? (
            <button className="narrative-modal__close" onClick={onClose}>×</button>
          ) : null}
        </div>

        <div className="narrative-modal__body">
          {/* 总体进度条 */}
          <div className="va-overall">
            <div className="va-overall__bar">
              <div
                className={'va-overall__fill' + (failed ? ' is-failed' : '')}
                style={{ width: `${overall}%` }}
              />
            </div>
            <div className="va-overall__pct">{overall.toFixed(0)}%</div>
          </div>

          {/* 十步列表 */}
          <ol className="va-steps">
            {ANALYSIS_STEPS.map((label, i) => {
              const idx = i + 1;
              const stateClass =
                failed && currentStep === idx ? 'is-failed'
                : done || currentStep > idx ? 'is-done'
                : currentStep === idx ? 'is-active'
                : 'is-pending';
              const isActive = currentStep === idx && !done && !failed;
              return (
                <li key={idx} className={'va-step ' + stateClass}>
                  <span className="va-step__icon">
                    {stateClass === 'is-done' ? '✓' : stateClass === 'is-failed' ? '✕' : idx}
                  </span>
                  <span className="va-step__label">{label}</span>
                  {isActive && activeDetail ? (
                    <span className="va-step__detail">{activeDetail}</span>
                  ) : null}
                  {isActive ? <span className="va-step__spinner" /> : null}
                </li>
              );
            })}
          </ol>

          {failed ? (
            <div className="va-fail">
              分析失败：{failReason || '未知错误'}。你仍可进入解说工作台继续（将使用基础切片）。
            </div>
          ) : null}

          {/* 完成后展示报告 */}
          {done && report ? (
            <div className="va-report">
              <div className="va-report__head">
                <span>📋 影片分析内容总结报告</span>
                <button className="btn sm ghost" onClick={copyReport}>{copied ? '已复制' : '复制'}</button>
              </div>
              <pre className="va-report__body">{report}</pre>
            </div>
          ) : null}
        </div>

        <div className="narrative-modal__footer">
          {!done && !failed ? (
            <div className="muted" style={{ fontSize: 12 }}>分析进行中，请稍候…（可保持此窗口，完成后可查看报告）</div>
          ) : (
            <button className="btn primary" onClick={onContinue}>
              {failed ? '仍进入解说工作台' : '进入解说工作台 →'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
