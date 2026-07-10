import { useApp } from '../state/AppContext';
import { filmSteps } from '../data/mock';

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

  if (filmStage === 'library') {
    const cats = filmCats;
    const projects = filmProjects[filmCat] || [];
    return (
      <div className="edit-grid" style={{ gridTemplateColumns: '200px 1fr' }}>
        <div>
          <div className="side-label">影片类型</div>
          <div className="lp">
            {cats.map((c) => (
              <div key={c.id} className={'lp-cat ' + (c.id === filmCat ? 'active' : '')} onClick={() => actions.set({ filmCat: c.id })}>
                <span>{c.name}</span><span className="n">{c.n}</span>
              </div>
            ))}
          </div>
          <button className="btn sm" style={{ marginTop: 14, width: '100%' }} onClick={actions.importFilm}>⬇ 导入影片</button>
        </div>
        <div>
          <div className="sec-title">工程库 · {cats.find((c) => c.id === filmCat)?.name}</div>
          <div className="proj-grid">
            {projects.map((p) => (
              <div key={p.t} className="proj-card" onClick={() => actions.openEditor(filmCat, p.t)}>
                <div className="pt">{p.t}</div>
                <div className={'ps s-' + p.s}>{p.s}</div>
              </div>
            ))}
          </div>
          <div className="muted sm" style={{ marginTop: 14 }}>点击卡片进入「剪辑台」（基于文案智能剪辑 + 时间线精修 + 字幕花字）。</div>
        </div>
      </div>
    );
  }

  // 剪辑台
  return (
    <div>
      <div className="sec-title">剪辑台 · {state.editingProj?.t}</div>
      <div className="muted sm" style={{ marginBottom: 12 }}>{STEP_DESC[editorSub]}</div>
      <StepPills current={editorSub} onPick={actions.goEditorSub} />

      {editorSub === 'gen' && (
        <div className="card">
          <div className="sec-title">解说文案（可编辑）</div>
          <textarea className="ed" rows={6} value={editorState.script} onChange={(e) => actions.set({ editorState: { ...editorState, script: e.target.value } })} />
          <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
            <button className="btn sm" onClick={actions.genFilmScript}>⚡ 自动生成解说文案</button>
            <button className="btn sm ghost" disabled={!editorState.script} onClick={() => actions.goEditorSub('align')}>下一步：导入对齐 →</button>
          </div>
        </div>
      )}

      {editorSub === 'align' && (
        <div className="card">
          <div className="kpis">
            <div className="kpi"><div className="v">{editorState.videoName}</div><div className="l">源视频</div></div>
            <div className="kpi"><div className="v">{editorState.aligned ? '✓' : '—'}</div><div className="l">对齐状态</div></div>
            <div className="kpi"><div className="v">{editorState.alignedPct}%</div><div className="l">对齐度</div></div>
          </div>
          <div className="wave" style={{ height: 64 }}>
            {Array.from({ length: 48 }).map((_, i) => <i key={i} style={{ height: `${20 + Math.abs(Math.sin(i * 0.7)) * 70}%` }} />)}
          </div>
          <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
            <button className="btn sm" disabled={!editorState.script} onClick={actions.alignFilm}>▶ 导入视频并对齐</button>
            <button className="btn sm ghost" disabled={!editorState.aligned} onClick={() => actions.goEditorSub('voice')}>下一步：解说配音 →</button>
          </div>
        </div>
      )}

      {editorSub === 'voice' && (
        <div className="edit-grid">
          <div className="card">
            <div className="sec-title">解说文案来源</div>
            <textarea className="ed" rows={5} value={editorState.script} onChange={(e) => actions.set({ editorState: { ...editorState, script: e.target.value } })} />
            <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', marginTop: 10 }}>
              <div className="field"><label>音色</label><select><option>知性女声</option><option>沉稳男声</option></select></div>
              <div className="field"><label>语速</label><select><option>正常</option><option>稍快</option><option>稍慢</option></select></div>
            </div>
            <div className="field" style={{ marginTop: 10 }}>
              <label>混音 · 原片原声占比：{Math.round(editorState.voiceMix * 100)}%（解说 {100 - Math.round(editorState.voiceMix * 100)}%）</label>
              <input type="range" min={0} max={100} value={Math.round(editorState.voiceMix * 100)} onChange={(e) => actions.setVoiceMix(+e.target.value / 100)} />
            </div>
            <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
              <button className="btn sm" onClick={actions.genVoiceForFilm}>🔊 智能配音 + 生成字幕</button>
              <button className="btn sm ghost" onClick={actions.previewMix}>🔈 预览混音</button>
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
          {(editorState.cuts || []).map((c, i) => (
            <div key={i} className="iss-row">
              <span className="ti">{c.t1}</span>
              <span className="tx">{c.tx}</span>
              <span className="muted">⏱ {c.dur}s</span>
              <span className="acts"><span className="tag gen">保留</span></span>
            </div>
          ))}
          {!editorState.cuts && <div className="muted sm">尚未切点。</div>}
          <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
            <button className="btn sm" onClick={actions.autoCut}>✂ 自动切点</button>
            <button className="btn sm ghost" disabled={!editorState.cuts} onClick={() => actions.goEditorSub('time')}>下一步：时间线精修 →</button>
          </div>
        </div>
      )}

      {editorSub === 'time' && (
        <div className="card">
          <div className="sec-title">时间线精修（四轨）</div>
          <div className="timeline">
            <div className="track"><span className="tl">视频</span>
              <div className="clip video" style={{ width: '60%' }}>旅行 vlog 主视频</div>
            </div>
            <div className="track"><span className="tl">音频</span>
              <div className="clip audio" style={{ width: '40%' }}>原声</div>
            </div>
            <div className="track"><span className="tl">字幕</span>
              <div className="clip sub" style={{ width: '52%' }}>解说字幕轨</div>
            </div>
            <div className="track"><span className="tl">生成</span>
              <div className="clip gen" style={{ width: '48%' }}>AI 生成片段</div>
            </div>
          </div>
          <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
            <button className="btn sm ghost" onClick={() => actions.goEditorSub('out')}>下一步：字幕花字导出 →</button>
          </div>
        </div>
      )}

      {editorSub === 'out' && (
        <div className="edit-grid">
          <div className="card">
            <div className="sec-title">字幕重点预览</div>
            <div className="script-box">{editorState.script}</div>
          </div>
          <div className="card">
            <div className="sec-title">导出设置</div>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}>
              <input type="checkbox" defaultChecked /> 烧录字幕（含重点高亮）
            </label>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}>
              <input type="checkbox" defaultChecked /> 混音配音
            </label>
            <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)' }}>
              <input type="checkbox" defaultChecked /> 硬件加速
            </label>
            <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
              <button className="btn sm" onClick={actions.archiveToFilm}>📁 归档到影片库</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
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
