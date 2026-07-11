import { useState } from 'react';
import type { TimelineEnvelope, TimelineTrack, TimelineClip } from '../ipc/types';
import { flowerTpls } from '../data/mock';

interface Props {
  envelope: TimelineEnvelope;
  onChange: (env: TimelineEnvelope) => void;
  onSave: () => void;
}

const KIND_LABEL: Record<string, string> = { video: '视频', audio: '音频', subtitle: '字幕', gen: '生成' };

function fmt(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

/**
 * M2 上基础精修组件：
 *  - 多轨只读预览（视频/音频/字幕/生成）
 *  - 选中片段后可基础裁剪（改 srcStart/srcEnd）
 *  - 轨道静音 / 音量
 *  - 删除片段
 *  - 字幕/生成片段可指定花字模板
 *  - 经 film_timeline_save 回存
 * 复杂拖拽排序 / 转场可视化留 M2 下。
 */
export default function TimelineEditor({ envelope, onChange, onSave }: Props) {
  const [sel, setSel] = useState<{ trackId: string; clipId: string } | null>(null);
  const [sVal, setSVal] = useState('0');
  const [eVal, setEVal] = useState('0');

  // 当前选中片段
  let selClip: TimelineClip | null = null;
  let selTrack: TimelineTrack | null = null;
  if (sel) {
    selTrack = envelope.tracks.find((t) => t.id === sel.trackId) || null;
    selClip = selTrack?.clips.find((c) => c.id === sel.clipId) || null;
  }

  const selectClip = (trackId: string, clip: TimelineClip) => {
    setSel({ trackId, clipId: clip.id });
    setSVal(String(clip.srcStart));
    setEVal(String(clip.srcEnd));
  };

  const patchClip = (trackId: string, clipId: string, patch: Partial<TimelineClip>) => {
    onChange({
      ...envelope,
      tracks: envelope.tracks.map((tr) =>
        tr.id === trackId
          ? { ...tr, clips: tr.clips.map((c) => (c.id === clipId ? { ...c, ...patch } : c)) }
          : tr,
      ),
    });
  };

  const deleteClip = (trackId: string, clipId: string) => {
    onChange({
      ...envelope,
      tracks: envelope.tracks.map((tr) =>
        tr.id === trackId ? { ...tr, clips: tr.clips.filter((c) => c.id !== clipId) } : tr,
      ),
    });
    if (sel && sel.clipId === clipId) setSel(null);
  };

  const patchTrack = (trackId: string, patch: Partial<TimelineTrack>) => {
    onChange({
      ...envelope,
      tracks: envelope.tracks.map((tr) => (tr.id === trackId ? { ...tr, ...patch } : tr)),
    });
  };

  const applyTrim = () => {
    if (!sel || !selClip) return;
    const s = Math.max(0, parseFloat(sVal) || 0);
    const e = Math.max(s, parseFloat(eVal) || 0);
    patchClip(sel.trackId, sel.clipId, { srcStart: s, srcEnd: e });
  };

  const total = envelope.tracks.reduce(
    (m, tr) => tr.clips.reduce((mm, c) => Math.max(mm, c.timelineEnd), m),
    0,
  );

  return (
    <div className="card">
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div className="sec-title" style={{ margin: 0 }}>时间线精修（{envelope.tracks.length} 轨 · 总时长 {fmt(total)}）</div>
        <button className="btn sm" onClick={onSave}>💾 保存时间线</button>
      </div>

      <div className="timeline" style={{ marginTop: 12 }}>
        {envelope.tracks.map((tr) => (
          <div className="track" key={tr.id}>
            <span className="tl" style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <span>{KIND_LABEL[tr.kind] || tr.name}</span>
              <span style={{ display: 'inline-flex', gap: 4, alignItems: 'center' }}>
                <button className="mini" style={{ fontSize: 10, padding: '1px 4px' }}
                  onClick={() => patchTrack(tr.id, { muted: !tr.muted })}>{tr.muted ? '🔇' : '🔈'}</button>
              </span>
              <input type="range" min={0} max={100} value={Math.round(tr.volume * 100)}
                style={{ width: 64 }} title="音量"
                onChange={(e) => patchTrack(tr.id, { volume: +e.target.value / 100 })} />
            </span>
            <div style={{ flex: 1, display: 'flex', gap: 6, flexWrap: 'wrap' }}>
              {tr.clips.length === 0 && <span className="muted sm">（空轨）</span>}
              {tr.clips.map((c) => {
                const w = total > 0 ? Math.max(6, ((c.timelineEnd - c.timelineStart) / total) * 100) : 20;
                return (
                  <div
                    key={c.id}
                    className={'clip ' + tr.kind + (sel && sel.clipId === c.id ? ' sel' : '')}
                    style={{ width: w + '%' }}
                    onClick={() => selectClip(tr.id, c)}
                    title={`${fmt(c.srcStart)}–${fmt(c.srcEnd)} ${c.label || c.text}`}
                  >
                    {c.flower ? '✦ ' : ''}{c.label || c.text || c.source}
                  </div>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      {selClip && selTrack && (
        <div className="card" style={{ marginTop: 14, background: 'var(--bg-soft, #f6f6f4)' }}>
          <div className="sec-title">选中片段 · {KIND_LABEL[selTrack.kind] || selTrack.name}</div>
          <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', gap: 10 }}>
            <div className="field">
              <label>片段标签</label>
              <input className="mini" value={selClip.label}
                onChange={(e) => patchClip(selTrack!.id, selClip!.id, { label: e.target.value })} />
            </div>
            <div className="field">
              <label>文本 / 字幕</label>
              <input className="mini" value={selClip.text}
                onChange={(e) => patchClip(selTrack!.id, selClip!.id, { text: e.target.value })} />
            </div>
          </div>

          <div className="grid" style={{ gridTemplateColumns: '1fr 1fr auto', gap: 10, marginTop: 10, alignItems: 'end' }}>
            <div className="field"><label>源入点 (srcStart)</label>
              <input className="mini" type="number" step="0.1" value={sVal} onChange={(e) => setSVal(e.target.value)} /></div>
            <div className="field"><label>源出点 (srcEnd)</label>
              <input className="mini" type="number" step="0.1" value={eVal} onChange={(e) => setEVal(e.target.value)} /></div>
            <button className="btn sm" onClick={applyTrim}>✂ 应用裁剪</button>
          </div>

          {(selTrack.kind === 'subtitle' || selTrack.kind === 'gen') && (
            <div className="field" style={{ marginTop: 10 }}>
              <label>花字模板</label>
              <select className="mini" value={selClip.flower}
                onChange={(e) => patchClip(selTrack!.id, selClip!.id, { flower: e.target.value })}>
                <option value="">（无）</option>
                {flowerTpls.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
              </select>
            </div>
          )}

          <div style={{ marginTop: 12 }}>
            <button className="btn sm ghost" onClick={() => deleteClip(selTrack.id, selClip.id)}>🗑 删除片段</button>
          </div>
        </div>
      )}

      {!selClip && <div className="muted sm" style={{ marginTop: 12 }}>点击任意片段进行裁剪 / 静音 / 花字设置。</div>}
    </div>
  );
}
