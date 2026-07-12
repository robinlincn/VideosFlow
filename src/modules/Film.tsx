import { useState } from 'react';
import { useApp } from '../state/AppContext';
import { filmSteps } from '../data/mock';
import type { TimelineEnvelope, TimelineClip } from '../ipc/types';
import TimelineEditor from '../components/TimelineEditor';
import FlowerPreview from '../components/FlowerPreview';
import {
  PlusIcon, ArrowLeftIcon, UploadIcon, MoveUpIcon, MoveDownIcon, PencilIcon, Trash2Icon,
} from '../components/icons';

const STEP_DESC: Record<string, string> = {
  gen: '根据影片生成可编辑的解说文案',
  align: '导入视频并与解说文案对齐',
  voice: '为影片智能配音并生成字幕',
  cut: '按文案自动切点生成粗剪',
  time: '多轨时间线精修',
  out: '字幕 / 花字导出与归档',
};

export default function Film() {
  const { state, actions } = useApp();
  const { filmStage, filmCat, filmCats, filmProjects, editorSub, editorState } = state;
  const [newCat, setNewCat] = useState('');
  const [delCat, setDelCat] = useState<{ id: string; name: string } | null>(null);
  const [delStrategy, setDelStrategy] = useState<'cascade' | 'merge'>('cascade');
  const [delTarget, setDelTarget] = useState<string>('');

  if (filmStage === 'library') {
    const cats = filmCats;
    const projects = filmProjects[filmCat] || [];
    const otherCats = cats.filter((c) => c.id !== filmCat);
    return (
      <div className="edit-grid" style={{ gridTemplateColumns: '220px 1fr' }}>
        <div>
          <div className="side-label">影片类型</div>
          <div className="lp">
            {cats.map((c) => (
              <div key={c.id} className={'lp-cat ' + (c.id === filmCat ? 'active' : '')} onClick={() => actions.switchCat(c.id)}>
                <span>{c.name}</span>
                <span className="n">{filmProjects[c.id]?.length || 0}</span>
                <span className="lp-acts" style={{ display: 'inline-flex', gap: 2, marginLeft: 6 }} onClick={(e) => e.stopPropagation()}>
                  <button className="ic" title="上移" onClick={() => actions.moveCat(c.id, -1)}><MoveUpIcon size={12} /></button>
                  <button className="ic" title="下移" onClick={() => actions.moveCat(c.id, 1)}><MoveDownIcon size={12} /></button>
                  <button className="ic" title="重命名" onClick={() => { const n = window.prompt('重命名类型', c.name); if (n) actions.renameCat(c.id, n); }}><PencilIcon size={12} /></button>
                  <button className="ic danger" title="删除" onClick={() => { setDelCat({ id: c.id, name: c.name }); setDelStrategy('cascade'); setDelTarget(otherCats[0]?.id || ''); }}><Trash2Icon size={12} /></button>
                </span>
              </div>
            ))}
          </div>
          <div style={{ marginTop: 12, display: 'flex', gap: 6 }}>
            <input className="mini" style={{ flex: 1 }} placeholder="新建类型" value={newCat} onChange={(e) => setNewCat(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter' && newCat.trim()) { actions.createCat(newCat.trim()); setNewCat(''); } }} />
            <button className="btn sm" disabled={!newCat.trim()} onClick={() => { actions.createCat(newCat.trim()); setNewCat(''); }}><PlusIcon /> 类型</button>
          </div>
        </div>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <div className="sec-title" style={{ marginBottom: 0 }}>工程库 · {cats.find((c) => c.id === filmCat)?.name}</div>
            <button className="btn sm" onClick={() => actions.importFilm()}><UploadIcon /> 导入影片</button>
          </div>
          <div className="proj-grid">
            {projects.map((p) => (
              <div key={p.id} className="proj-card" onClick={() => actions.openEditor(filmCat, p.id, p.title)}>
                <button className="x" style={{ position: 'absolute', top: 6, right: 8 }} onClick={(e) => { e.stopPropagation(); if (window.confirm('删除工程「' + p.title + '」？')) actions.deleteProject(p.id); }}><Trash2Icon size={12} /></button>
                <div className="pt">{p.title}</div>
                <div className={'ps s-' + p.status}>{p.status}</div>
                <select className="mini" style={{ marginTop: 8, width: '100%' }} value={p.status}
                  onClick={(e) => e.stopPropagation()}
                  onChange={(e) => actions.updateProject(p.id, { status: e.target.value })}>
                  {['草稿', '制作中', '已发布'].map((s) => <option key={s} value={s}>{s}</option>)}
                </select>
              </div>
            ))}
          </div>
          {projects.length === 0 && (
            <div className="empty-hint" style={{ padding: 'var(--space-10) 0' }}>
              该类型下暂无工程<br />
              <button className="btn" style={{ marginTop: 16 }} onClick={() => actions.importFilm()}><UploadIcon /> 导入影片</button>
            </div>
          )}
          {projects.length > 0 && <div className="muted sm" style={{ marginTop: 14 }}>点击卡片进入剪辑台（基于文案智能剪辑 + 时间线精修 + 字幕花字）。</div>}
        </div>

        {delCat && (
          <div className="modal-mask" style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.45)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 }} onClick={() => setDelCat(null)}>
            <div className="modal" style={{ background: 'var(--paper, #fff)', borderRadius: 14, padding: 20, width: 380, maxWidth: '90vw', boxShadow: '0 12px 40px rgba(0,0,0,0.25)' }} onClick={(e) => e.stopPropagation()}>
              <div className="sec-title">删除类型 · {delCat.name}</div>
              <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 13, margin: '10px 0' }}>
                <input type="radio" checked={delStrategy === 'cascade'} onChange={() => setDelStrategy('cascade')} /> 级联删除（其下工程与时间线一并删除）
              </label>
              <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 13 }}>
                <input type="radio" checked={delStrategy === 'merge'} onChange={() => setDelStrategy('merge')} /> 归并到其它类型
              </label>
              {delStrategy === 'merge' && (
                <select className="mini" style={{ marginTop: 8, width: '100%' }} value={delTarget} onChange={(e) => setDelTarget(e.target.value)}>
                  {otherCats.length === 0 && <option value="">（无其它类型）</option>}
                  {otherCats.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
              )}
              <div style={{ marginTop: 16, display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
                <button className="btn sm ghost" onClick={() => setDelCat(null)}>取消</button>
                <button className="btn sm" disabled={delStrategy === 'merge' && !delTarget}
                  onClick={() => { actions.deleteCat(delCat.id, delStrategy, delStrategy === 'merge' ? delTarget : undefined); setDelCat(null); }}>确认删除</button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  // 剪辑台
  const timeline = editorState.timeline;
  const videoClips = timeline?.tracks.find((t) => t.kind === 'video')?.clips || [];
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 18, flexWrap: 'wrap', gap: 12 }}>
        <div className="sec-title" style={{ marginBottom: 0 }}>剪辑台 · {state.editingProj?.t}</div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn sm ghost" onClick={() => actions.goLibrary()}><ArrowLeftIcon /> 返回影片库</button>
          <button className="btn sm" onClick={() => actions.importFilm()}><UploadIcon /> 导入新影片</button>
        </div>
      </div>
      <div className="muted sm" style={{ marginBottom: 12 }}>{STEP_DESC[editorSub]}</div>
      <StepPills current={editorSub} onPick={actions.goEditorSub} />

      {editorSub === 'gen' && (
        <div className="card">
          <div className="sec-title">解说文案（可编辑）</div>
          <textarea className="ed" rows={6} value={editorState.script}
            onChange={(e) => actions.set({ editorState: { ...editorState, script: e.target.value } })} />
          <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
            <button className="btn sm" onClick={() => actions.genFilmScript()}>⚡ 自动生成解说文案</button>
            <button className="btn sm ghost" disabled={!editorState.script} onClick={() => actions.goEditorSub('align')}>下一步：导入对齐 →</button>
          </div>
        </div>
      )}

      {editorSub === 'align' && (
        <div className="card">
          <div className="kpis">
            <div className="kpi"><div className="v" style={{ fontSize: 14 }}>{editorState.videoName || '未指定视频'}</div><div className="l">源视频</div></div>
            <div className="kpi"><div className="v">{editorState.aligned ? '✓' : '—'}</div><div className="l">对齐状态</div></div>
            <div className="kpi"><div className="v">{editorState.alignedPct}%</div><div className="l">对齐度</div></div>
          </div>
          <div className="wave" style={{ height: 64 }}>
            {Array.from({ length: 48 }).map((_, i) => <i key={i} style={{ height: `${20 + Math.abs(Math.sin(i * 0.7)) * 70}%` }} />)}
          </div>
          {editorState.aligned && editorState.asr.length === 0 && (
            <div className="muted sm" style={{ marginTop: 10, color: 'var(--warn)' }}>ASR 降级：未获取到时间戳句（占位端点）。仍可继续粗剪 / 精修 / 导出。</div>
          )}
          {editorState.asr.length > 0 && (
            <div className="script-box" style={{ marginTop: 12 }}>
              {editorState.asr.map((a, i) => (
                <div key={i} className="tr-line"><span className="t">{fmt(a.start)}–{fmt(a.end)}</span><span className="x">{a.text}</span></div>
              ))}
            </div>
          )}
          <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
            <button className="btn sm" disabled={!editorState.script} onClick={() => actions.alignFilm()}>▶ 导入视频并对齐</button>
            <button className="btn sm ghost" disabled={!editorState.aligned} onClick={() => actions.goEditorSub('voice')}>下一步：解说配音 →</button>
          </div>
        </div>
      )}

      {editorSub === 'voice' && (
        <div className="edit-grid">
          <div className="card">
            <div className="sec-title">解说文案来源</div>
            <textarea className="ed" rows={5} value={editorState.script}
              onChange={(e) => actions.set({ editorState: { ...editorState, script: e.target.value } })} />
            <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', marginTop: 10 }}>
              <div className="field"><label>音色</label><select><option>知性女声</option><option>沉稳男声</option></select></div>
              <div className="field"><label>语速</label><select><option>正常</option><option>稍快</option><option>稍慢</option></select></div>
            </div>
            <div className="field" style={{ marginTop: 10 }}>
              <label>混音 · 原片原声占比：{Math.round(editorState.voiceMix * 100)}%（解说 {100 - Math.round(editorState.voiceMix * 100)}%）</label>
              <input type="range" min={0} max={100} value={Math.round(editorState.voiceMix * 100)} onChange={(e) => actions.setVoiceMix(+e.target.value / 100)} />
            </div>
            <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
              <button className="btn sm" onClick={() => actions.genVoiceForFilm()}>🔊 智能配音 + 生成字幕</button>
              <button className="btn sm ghost" onClick={() => actions.previewMix()}>🔈 预览混音</button>
            </div>
          </div>
          <div className="card">
            <div className="sec-title">混音预览（原声 / 解说 双轨）</div>
            <div className="wave" style={{ height: 46, marginBottom: 6 }}>
              {Array.from({ length: 40 }).map((_, i) => <i key={i} style={{ height: `${10 + Math.abs(Math.sin(i)) * editorState.voiceMix * 80}%`, background: '#8e8e93' }} />)}
            </div>
            <div className="wave" style={{ height: 46 }}>
              {Array.from({ length: 40 }).map((_, i) => <i key={i} style={{ height: `${10 + Math.abs(Math.cos(i)) * (1 - editorState.voiceMix) * 80}%` }} />)}
            </div>
            <div className="sec-title" style={{ marginTop: 12 }}>字幕（可编辑）</div>
            {(editorState.voiceLines || []).map((l) => (
              <div key={l.id} className="ed-row"><label>{l.t}</label>
                <textarea className="ed" rows={1} value={l.x} onChange={(e) => actions.editVoiceLine(l.id, e.target.value)} />
              </div>
            ))}
            {!editorState.voiceLines && <div className="muted sm">尚未生成配音。</div>}
          </div>
        </div>
      )}
      {editorSub === 'voice' && (
        <div style={{ marginTop: 12 }}><button className="btn sm ghost" disabled={!editorState.voiceLines} onClick={() => actions.goEditorSub('cut')}>下一步：自动切点 →</button></div>
      )}

      {editorSub === 'cut' && (
        <div className="card">
          <div className="sec-title">自动切点（基于文案）</div>
          {videoClips.length > 0 ? (
            videoClips.map((c: TimelineClip, i) => (
              <div key={c.id} className="iss-row">
                <span className="ti">{fmt(c.srcStart)}–{fmt(c.srcEnd)}</span>
                <span className="tx">{c.label || c.text || '片段 ' + (i + 1)}</span>
                <span className="muted">⏱ {(c.srcEnd - c.srcStart).toFixed(1)}s</span>
                <span className="acts"><span className="tag gen">保留</span></span>
              </div>
            ))
          ) : (
            <div className="muted sm">尚无粗剪时间线。请先完成「导入对齐」，再点下方按钮生成粗剪。</div>
          )}
          <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
            <button className="btn sm" disabled={!editorState.timeline} onClick={() => actions.autoCut()}>✂ 自动切点</button>
            <button className="btn sm ghost" disabled={!editorState.timeline} onClick={() => actions.goEditorSub('time')}>下一步：时间线精修 →</button>
          </div>
        </div>
      )}

      {editorSub === 'time' && (
        timeline ? (
          <TimelineEditor
            envelope={timeline}
            onSave={() => actions.saveTimeline()}
            onChange={(env: TimelineEnvelope) => actions.set({ editorState: { ...editorState, timeline: env } })}
          />
        ) : (
          <div className="card">
            <div className="muted sm">尚无时间线。请先到「自动切点」生成粗剪时间线。</div>
            <div style={{ marginTop: 10 }}>
              <button className="btn sm ghost" onClick={() => actions.goEditorSub('cut')}>← 返回自动切点</button>
            </div>
          </div>
        )
      )}

      {editorSub === 'out' && (
        <div className="edit-grid">
          <div className="card">
            <div className="sec-title">花字模板（6 套固化）</div>
            <FlowerPreview selected={editorState.flower} onPick={(id) => actions.pickFlower(id)} />
          </div>
          <div className="card">
            <div className="sec-title">导出设置</div>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}>
              <input type="checkbox" checked={editorState.exportOpts.burnSub} onChange={(e) => actions.setExportOpt({ burnSub: e.target.checked })} /> 烧录字幕 / 花字
            </label>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}>
              <input type="checkbox" checked={editorState.exportOpts.mixVoice} onChange={(e) => actions.setExportOpt({ mixVoice: e.target.checked })} /> 混音 AI 配音
            </label>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 10 }}>
              <input type="checkbox" checked={editorState.exportOpts.hw} onChange={(e) => actions.setExportOpt({ hw: e.target.checked })} /> 硬件加速（h264_nvenc）
            </label>
            <div className="field">
              <label>分辨率</label>
              <select value={editorState.exportOpts.resolution} onChange={(e) => actions.setExportOpt({ resolution: e.target.value })}>
                {['1920x1080', '1280x720', '3840x2160'].map((r) => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div style={{ marginTop: 14, display: 'flex', gap: 8 }}>
              <button className="btn sm" onClick={() => actions.exportFilm()}>🎬 导出 MP4</button>
              <button className="btn sm ghost" onClick={() => actions.archiveToFilm()}>📁 归档到影片库</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function fmt(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

function StepPills({ current, onPick }: { current: string; onPick: (id: string) => void; }) {
  return (
    <div className="steps">
      {filmSteps.map((s) => {
        const idx = filmSteps.findIndex((x) => x.id === s.id);
        const cur = filmSteps.findIndex((x) => x.id === current);
        const cls = s.id === current ? 'active' : idx < cur ? 'done' : '';
        return <div key={s.id} className={'step ' + cls} onClick={() => onPick(s.id)}>{(idx + 1)} {s.name}</div>;
      })}
    </div>
  );
}
