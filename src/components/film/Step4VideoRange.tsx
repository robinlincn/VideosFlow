// 步骤 4：设置视频范围（模态弹窗 + 双滑块 + 4 快捷 + 4 微调）
// v2.0 重构：新增模态组件，替代 v1.0 缺失的范围裁剪功能

import { useState, useEffect } from 'react';

interface Props {
  open: boolean;
  totalDuration: number; // 秒
  initialStart?: number;
  initialEnd?: number;
  onConfirm: (start: number, end: number) => void;
  onCancel: () => void;
}

function fmt(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export default function Step4VideoRange({ open, totalDuration, initialStart, initialEnd, onConfirm, onCancel }: Props) {
  const [start, setStart] = useState(initialStart ?? 0);
  const [end, setEnd] = useState(initialEnd ?? totalDuration);

  useEffect(() => {
    if (open) {
      setStart(initialStart ?? 0);
      setEnd(initialEnd ?? totalDuration);
    }
  }, [open, initialStart, initialEnd, totalDuration]);

  if (!open) return null;

  // 时间轴位置计算（最简：用 min/max 映射到 0-100%）
  const dur = Math.max(totalDuration, 1);
  const startPct = (start / dur) * 100;
  const endPct = (end / dur) * 100;

  // 4 快捷
  const skipHead30 = () => { setStart(Math.min(end, 30)); };
  const skipTail30 = () => { setEnd(Math.max(start, end - 30)); };
  const skipHeadTail = () => { setStart(30); setEnd(Math.max(30, end)); };
  const reset = () => { setStart(0); setEnd(totalDuration); };

  // 4 微调
  const adjStartNeg10 = () => setStart(Math.max(0, start - 10));
  const adjStartPos10 = () => setStart(Math.min(end - 1, start + 10));
  const adjEndNeg5 = () => setEnd(Math.max(start + 1, end - 5));
  const adjEndPos5 = () => setEnd(Math.min(dur, end + 5));

  return (
    <div className="narrative-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="narrative-modal export-modal">
        <div className="narrative-modal__header">
          <h3>🎬 设置视频范围</h3>
          <button className="narrative-modal__close" onClick={onCancel}>×</button>
        </div>
        <div className="narrative-modal__body">
          <div className="film-step4__preview">
            <div className="film-step4__video" />
            <div className="film-step4__timecode">{fmt(0)} / {fmt(totalDuration)}</div>
          </div>
          <div className="muted" style={{ marginTop: 6, fontSize: 12 }}>拖动滑块选择要分析的视频片段</div>
          <div className="film-step4__range">
            <div className="film-step4__range-track" />
            <div className="film-step4__range-selected" style={{ left: `${startPct}%`, right: `${100 - endPct}%` }} />
            <div className="film-step4__range-handle" style={{ left: `${startPct}%` }} />
            <div className="film-step4__range-handle" style={{ left: `${endPct}%` }} />
          </div>
          <div className="film-step4__time-labels">
            <div className="film-step4__time-block">
              <div className="muted" style={{ fontSize: 11 }}>开始</div>
              <div className="film-step4__time-value">{fmt(start)}</div>
            </div>
            <div className="film-step4__time-block">
              <div className="muted" style={{ fontSize: 11 }}>时长</div>
              <div className="film-step4__time-value">{fmt(end - start)}</div>
            </div>
            <div className="film-step4__time-block">
              <div className="muted" style={{ fontSize: 11 }}>结束</div>
              <div className="film-step4__time-value">{fmt(end)}</div>
            </div>
          </div>
          <div className="form-row">
            <label className="form-label">快捷调整范围</label>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button className="btn sm" onClick={skipHead30}>跳过片头 30s</button>
              <button className="btn sm" onClick={skipTail30}>跳过片尾 60s</button>
              <button className="btn sm" onClick={skipHeadTail}>跳过片头片尾</button>
              <button className="btn sm" onClick={reset}>重置</button>
            </div>
          </div>
          <div className="form-row">
            <label className="form-label">精确调整范围</label>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <span className="muted">开始</span>
              <button className="btn sm" onClick={adjStartNeg10}>◀ 10秒</button>
              <button className="btn sm" onClick={adjStartPos10}>10秒 ▶</button>
              <span style={{ flex: 1 }} />
              <span className="muted">结束</span>
              <button className="btn sm" onClick={adjEndNeg5}>◀ 5秒</button>
              <button className="btn sm" onClick={adjEndPos5}>5秒 ▶</button>
            </div>
          </div>
        </div>
        <div className="narrative-modal__footer">
          <button className="btn" onClick={onCancel}>取消</button>
          <button className="btn primary" onClick={() => onConfirm(start, end)}>确认</button>
        </div>
      </div>
    </div>
  );
}
