// 步骤 4：设置视频范围（模态弹窗）
// v2.0 重构：真实视频预览 + 可拖拽双滑块 + 选区平移 + 点击 seek +
//           实时时间码 + 精确时间输入 + 快捷/微调 + 键盘控制

import {
  useState, useEffect, useRef,
  type PointerEvent as ReactPointerEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { getVideoServerBase, initVideoServer } from '../../ipc/client';

interface Props {
  open: boolean;
  videoPath: string;
  totalDuration: number; // 秒
  initialStart?: number;
  initialEnd?: number;
  onConfirm: (start: number, end: number) => void;
  onCancel: () => void;
}

function fmt(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(ss)}` : `${m}:${pad(ss)}`;
}

function parseTime(str: string, dur: number): number | null {
  const t = str.trim();
  if (t === '') return null;
  if (/^\d+(\.\d+)?$/.test(t)) {
    const v = parseFloat(t);
    return isNaN(v) ? null : Math.min(dur, Math.max(0, v));
  }
  const m = t.match(/^(?:(\d+):)?(\d{1,2}):(\d{2})$/);
  if (m) {
    const h = m[1] ? parseInt(m[1], 10) : 0;
    const mm = parseInt(m[2], 10);
    const ss = parseInt(m[3], 10);
    const v = h * 3600 + mm * 60 + ss;
    return Math.min(dur, Math.max(0, v));
  }
  return null;
}

function toVideoSrc(p: string): string {
  if (!p) return '';
  // 桌面版：通过本地文件服务器（127.0.0.1）加载，支持 HTTP Range 任意位置 seek
  if (/^[a-zA-Z]:\\/.test(p) || p.startsWith('file://')) {
    const base = getVideoServerBase();
    if (base) return `${base}/file?path=${encodeURIComponent(p)}`;
    return '';
  }
  if (p.startsWith('blob:') || p.startsWith('http://') || p.startsWith('https://')) return p;
  return p; // 其他保持原样
}

export default function Step4VideoRange({
  open, videoPath, totalDuration, initialStart, initialEnd, onConfirm, onCancel,
}: Props) {
  const dur = Math.max(totalDuration, 0.1);
  const [start, setStart] = useState(initialStart ?? 0);
  const [end, setEnd] = useState(initialEnd ?? dur);
  const [actualDur, setActualDur] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [videoError, setVideoError] = useState(false);
  const [videoSrc, setVideoSrc] = useState('');
  const [startStr, setStartStr] = useState(fmt(initialStart ?? 0));
  const [endStr, setEndStr] = useState(fmt(initialEnd ?? dur));

  const videoRef = useRef<HTMLVideoElement>(null);
  const rangeRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ type: 'start' | 'end' | 'move'; grabTime: number; startVal: number; endVal: number } | null>(null);

  const effDur = actualDur && actualDur > 0 ? actualDur : dur;

  useEffect(() => {
    if (open) {
      const s0 = initialStart ?? 0;
      const e0 = initialEnd ?? dur;
      setStart(s0); setEnd(e0);
      setStartStr(fmt(s0)); setEndStr(fmt(e0));
      setActualDur(null); setPlaying(false); setCurrentTime(0); setVideoError(false);
    }
  }, [open, initialStart, initialEnd, dur]);

  useEffect(() => { setStartStr(fmt(start)); }, [start]);
  useEffect(() => { setEndStr(fmt(end)); }, [end]);

  // 视频预览源：桌面版通过本地文件服务器加载（异步获取 base 后再计算）
  useEffect(() => {
    let cancelled = false;
    initVideoServer().then(() => {
      if (!cancelled) setVideoSrc(toVideoSrc(videoPath));
    });
    return () => { cancelled = true; };
  }, [videoPath]);

  if (!open) return null;

  const pctToTime = (clientX: number): number => {
    const el = rangeRef.current;
    if (!el) return 0;
    const rect = el.getBoundingClientRect();
    const x = Math.min(rect.width, Math.max(0, clientX - rect.left));
    return (x / rect.width) * effDur;
  };

  const seekVideo = (t: number) => {
    const v = videoRef.current;
    if (v) { try { v.currentTime = Math.min(effDur, Math.max(0, t)); } catch { /* noop */ } }
  };

  const togglePlay = () => {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) {
      if (v.currentTime >= end - 0.05) v.currentTime = start;
      v.play().then(() => setPlaying(true)).catch(() => setPlaying(false));
    } else {
      v.pause(); setPlaying(false);
    }
  };

  const onHandleDown = (which: 'start' | 'end') => (e: ReactPointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragRef.current = { type: which, grabTime: 0, startVal: start, endVal: end };
  };
  const onMoveDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragRef.current = { type: 'move', grabTime: pctToTime(e.clientX), startVal: start, endVal: end };
  };
  const onDragMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const t = pctToTime(e.clientX);
    if (d.type === 'start') {
      const ns = Math.min(d.endVal - 0.5, Math.max(0, t));
      setStart(ns); seekVideo(ns);
    } else if (d.type === 'end') {
      const ne = Math.max(d.startVal + 0.5, Math.min(effDur, t));
      setEnd(ne); seekVideo(ne);
    } else {
      const delta = t - d.grabTime;
      let ns = d.startVal + delta;
      let ne = d.endVal + delta;
      if (ns < 0) { ne -= ns; ns = 0; }
      if (ne > effDur) { ns -= (ne - effDur); ne = effDur; }
      setStart(Math.max(0, ns)); setEnd(Math.min(effDur, ne));
      seekVideo(t);
    }
  };
  const onDragUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    dragRef.current = null;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch { /* noop */ }
  };

  const onTrackDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const t = pctToTime(e.clientX);
    seekVideo(t); setPlaying(false);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 5 : 1;
    if (e.key === ' ') { e.preventDefault(); togglePlay(); }
    else if (e.key === 'ArrowLeft') { e.preventDefault(); const ns = Math.max(0, start - step); setStart(ns); seekVideo(ns); }
    else if (e.key === 'ArrowRight') { e.preventDefault(); const ns = Math.min(end - 0.5, start + step); setStart(ns); seekVideo(ns); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); const ne = Math.min(effDur, end + step); setEnd(ne); seekVideo(ne); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); const ne = Math.max(start + 0.5, end - step); setEnd(ne); seekVideo(ne); }
    else if (e.key === 'Enter') { e.preventDefault(); if (start < end - 0.5) onConfirm(start, end); }
    else if (e.key === 'Escape') { e.preventDefault(); onCancel(); }
  };

  const onStartInput = (v: string) => {
    setStartStr(v);
    const t = parseTime(v, effDur);
    if (t !== null) setStart(Math.min(end - 0.5, Math.max(0, t)));
  };
  const onEndInput = (v: string) => {
    setEndStr(v);
    const t = parseTime(v, effDur);
    if (t !== null) setEnd(Math.max(start + 0.5, Math.min(effDur, t)));
  };

  // 快捷
  const skipHead = () => { const ns = Math.min(end - 0.5, 30); setStart(ns); seekVideo(ns); };
  const skipTail = () => { const ne = Math.max(start + 0.5, effDur - 30); setEnd(ne); seekVideo(ne); };
  const skipBoth = () => { const ns = 30; const ne = Math.max(ns + 0.5, effDur - 30); setStart(ns); setEnd(ne); seekVideo(ns); };
  const reset = () => { setStart(0); setEnd(effDur); seekVideo(0); };

  const fine = (which: 'start' | 'end', delta: number) => {
    if (which === 'start') { const ns = Math.max(0, Math.min(end - 0.5, start + delta)); setStart(ns); seekVideo(ns); }
    else { const ne = Math.max(start + 0.5, Math.min(effDur, end + delta)); setEnd(ne); seekVideo(ne); }
  };

  const startPct = (start / effDur) * 100;
  const endPct = (end / effDur) * 100;
  const segPct = ((end - start) / effDur) * 100;
  const invalid = start >= end - 0.5;

  return (
    <div className="narrative-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="narrative-modal export-modal" tabIndex={0} autoFocus onKeyDown={onKeyDown}>
        <div className="narrative-modal__header">
          <h3>🎬 设置视频范围</h3>
          <button className="narrative-modal__close" onClick={onCancel}>×</button>
        </div>
        <div className="narrative-modal__body">
          {/* 视频预览 + 播放控制 */}
          <div className={'film-step4__preview' + (videoError ? ' film-step4__preview--error' : '')}>
            {videoError ? (
              <div className="film-step4__video-err">
                此环境无法预览本地视频（桌面版需允许本地文件访问）。<br />
                范围设置功能不受影响，可正常拖动滑块与确认。
              </div>
            ) : (
              <video
                ref={videoRef}
                src={videoSrc || undefined}
                className="film-step4__video-el"
                preload="metadata"
                onLoadedMetadata={(e) => { const d = e.currentTarget.duration; if (isFinite(d) && d > 0) setActualDur(d); }}
                onTimeUpdate={(e) => {
                  const ct = e.currentTarget.currentTime;
                  setCurrentTime(ct);
                  if (playing && ct >= end - 0.05) { e.currentTarget.pause(); setPlaying(false); }
                }}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onEnded={() => setPlaying(false)}
                onClick={togglePlay}
                onError={() => setVideoError(true)}
              />
            )}
            {!videoError && (
              <div className="film-step4__play" onClick={togglePlay} title="播放/暂停（空格）">
                {playing ? '⏸' : '▶'}
              </div>
            )}
            <div className="film-step4__timecode">{fmt(currentTime)} / {fmt(effDur)}</div>
          </div>

          <div className="muted" style={{ marginTop: 6, fontSize: 12 }}>
            拖动滑块选择要分析的片段 · 点击时间轴定位播放头 · 空格播放/暂停
          </div>

          {/* 时间轴（双滑块 + 选区平移 + 点击 seek） */}
          <div className="film-step4__range" ref={rangeRef} onPointerDown={onTrackDown}>
            <div className="film-step4__range-track" />
            <div
              className="film-step4__range-selected"
              style={{ left: `${startPct}%`, width: `${endPct - startPct}%` }}
              onPointerDown={onMoveDown}
              onPointerMove={onDragMove}
              onPointerUp={onDragUp}
            />
            <div
              className="film-step4__range-handle"
              style={{ left: `${startPct}%` }}
              onPointerDown={onHandleDown('start')}
              onPointerMove={onDragMove}
              onPointerUp={onDragUp}
            />
            <div
              className="film-step4__range-handle"
              style={{ left: `${endPct}%` }}
              onPointerDown={onHandleDown('end')}
              onPointerMove={onDragMove}
              onPointerUp={onDragUp}
            />
          </div>

          {/* 时间显示 + 选区占比 */}
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
            <div className="film-step4__time-block">
              <div className="muted" style={{ fontSize: 11 }}>占比</div>
              <div className="film-step4__time-value">{segPct.toFixed(0)}%</div>
            </div>
          </div>

          {/* 精确时间输入（秒 或 mm:ss） */}
          <div className="form-row">
            <label className="form-label">精确时间（秒 或 mm:ss）</label>
            <div className="film-step4__inputs">
              <span className="muted">开始</span>
              <input
                className="film-step4__input"
                value={startStr}
                onChange={(e) => onStartInput(e.target.value)}
                onBlur={() => setStartStr(fmt(start))}
                inputMode="text"
                spellCheck={false}
              />
              <span className="muted">结束</span>
              <input
                className="film-step4__input"
                value={endStr}
                onChange={(e) => onEndInput(e.target.value)}
                onBlur={() => setEndStr(fmt(end))}
                inputMode="text"
                spellCheck={false}
              />
            </div>
          </div>

          {/* 快捷裁剪 */}
          <div className="form-row">
            <label className="form-label">快捷裁剪</label>
            <div className="film-step4__quick">
              <button className="btn sm" onClick={skipHead}>跳过片头 30s</button>
              <button className="btn sm" onClick={skipTail}>跳过片尾 30s</button>
              <button className="btn sm" onClick={skipBoth}>片头片尾各 30s</button>
              <button className="btn sm" onClick={reset}>重置整段</button>
            </div>
          </div>

          {/* 微调（Shift ×5） */}
          <div className="form-row">
            <label className="form-label">微调（Shift ×5）</label>
            <div className="film-step4__fine">
              <span className="muted">开始</span>
              <button className="btn sm" onClick={() => fine('start', -1)}>−1s</button>
              <button className="btn sm" onClick={() => fine('start', -5)}>−5s</button>
              <button className="btn sm" onClick={() => fine('start', 1)}>+1s</button>
              <button className="btn sm" onClick={() => fine('start', 5)}>+5s</button>
              <span style={{ flex: 1 }} />
              <span className="muted">结束</span>
              <button className="btn sm" onClick={() => fine('end', -1)}>−1s</button>
              <button className="btn sm" onClick={() => fine('end', -5)}>−5s</button>
              <button className="btn sm" onClick={() => fine('end', 1)}>+1s</button>
              <button className="btn sm" onClick={() => fine('end', 5)}>+5s</button>
            </div>
          </div>

          <div className="film-step4__hint">
            快捷键：空格 播放/暂停 · ← → 调开始 · ↑ ↓ 调结束 · Enter 确认 · Esc 取消
          </div>
        </div>
        <div className="narrative-modal__footer">
          <button className="btn" onClick={onCancel}>取消</button>
          <button className="btn primary" disabled={invalid} onClick={() => onConfirm(start, end)}>确认范围</button>
        </div>
      </div>
    </div>
  );
}
