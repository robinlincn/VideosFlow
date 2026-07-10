import { useApp } from '../state/AppContext';
import { cSteps, stylePresets, refCats, defaultSubs } from '../data/mock';

const STEP_DESC: Record<string, string> = {
  req: '描述大体需求，自动写文案', script: '文案初稿', human: '口语化去 AI 味',
  story: '生成可编辑的分镜文案', image: '按分镜生成图片（可加分类参考图）',
  frames: '按图片与分镜生成首尾帧视频', voice: '多声音配音 + 字幕', export: '合成与导出',
};

export default function Creation() {
  const { state, actions } = useApp();
  const { cStage, cState } = state;
  return (
    <div>
      <div className="muted sm" style={{ marginBottom: 12 }}>{STEP_DESC[cStage]}</div>
      <StepPills current={cStage} onPick={(id) => actions.set({ cStage: id })} />
      {cStage === 'req' && <ReqView />}
      {cStage === 'script' && <ScriptView />}
      {cStage === 'human' && <HumanView />}
      {cStage === 'story' && <StoryView />}
      {cStage === 'image' && <ImageView />}
      {cStage === 'frames' && <FramesView />}
      {cStage === 'voice' && <VoiceView />}
      {cStage === 'export' && <ExportView />}
    </div>
  );
}

function StepPills({ current, onPick }: { current: string; onPick: (id: string) => void; }) {
  return (
    <div className="steps">
      {cSteps.map((s, i) => {
        const cur = cSteps.findIndex((x) => x.id === current);
        const cls = s.id === current ? 'active' : i < cur ? 'done' : '';
        return <div key={s.id} className={'step ' + cls} onClick={() => onPick(s.id)}>{(i + 1)} {s.name}</div>;
      })}
    </div>
  );
}

function ReqView() {
  const { state, actions } = useApp();
  return (
    <div className="card" style={{ maxWidth: 760 }}>
      <div className="sec-title">大体需求{state.cState.reqFromSpoken ? '（已带入口播文案）' : ''}</div>
      <div className="field"><label>主题 / 一句话需求</label>
        <textarea id="brief" rows={4} placeholder="例如：做一个 60 秒的科普短视频，介绍 AI 视频剪辑，风格轻松活泼，面向新手">{state.cState.reqFromSpoken || '做一个 60 秒的科普短视频，介绍 AI 视频剪辑，风格轻松活泼，面向新手。'}</textarea>
      </div>
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <div className="field"><label>风格</label><select><option>轻松活泼</option><option>专业沉稳</option><option>温情叙事</option><option>炫酷科技</option></select></div>
        <div className="field"><label>时长</label><select><option>30 秒</option><option>60 秒</option><option>2 分钟</option><option>5 分钟</option></select></div>
        <div className="field"><label>受众</label><select><option>新手</option><option>从业者</option><option>大众</option></select></div>
        <div className="field"><label>平台</label><select><option>抖音</option><option>视频号</option><option>B站</option><option>YouTube</option></select></div>
      </div>
      <button className="btn" onClick={actions.genScript}>✨ 自动写文案</button>
    </div>
  );
}

function ScriptView() {
  const { state, actions } = useApp();
  if (!state.cState.script) return <div className="empty-hint">请先完成「需求」步骤生成文案。</div>;
  return (
    <div>
      <div className="sec-title">自动写文案（初稿）</div>
      <div className="script-box">{state.cState.script}</div>
      <div style={{ marginTop: 12 }}><button className="btn" onClick={actions.goHuman}>下一步：去 AI 味 →</button></div>
    </div>
  );
}

function HumanView() {
  const { state, actions } = useApp();
  const { cState, settingsState } = state;
  const pt = settingsState.prompts;
  const selTpl = cState.humanPrompt || 'humanize';
  if (!cState.script) return <div className="empty-hint">请先生成文案。</div>;
  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
      <div>
        <div className="sec-title">去 AI 味前</div>
        <div className="script-box" style={{ color: 'var(--muted)' }}>{cState.script}</div>
      </div>
      <div>
        <div className="sec-title">去 AI 味后（口语化）</div>
        <div className="script-box">{cState.human || <span className="muted">点击「去 AI 味」生成</span>}</div>
        <div className="field" style={{ marginTop: 10 }}>
          <label>提示词模板</label>
          <select value={selTpl} onChange={(e) => actions.pickHumanPrompt(e.target.value)}>
            {Object.keys(pt).map((k) => <option key={k} value={k}>{pt[k].name}</option>)}
          </select>
        </div>
        <div className="muted sm" style={{ margin: '4px 0 8px' }}>去 AI 味将使用「{pt[selTpl].name}」模板对文案做口语化改写。</div>
        <button className="btn sm" onClick={actions.doHuman}>🪄 去 AI 味</button>
      </div>
      <div style={{ marginTop: 12, gridColumn: '1 / -1' }}>
        <button className="btn" disabled={!cState.human} onClick={actions.goStory}>下一步：生成分镜 →</button>
      </div>
    </div>
  );
}

function StoryView() {
  const { state, actions } = useApp();
  const { cState } = state;
  const sr = cState.styleRef || '现实';
  const sp = stylePresets[sr] || stylePresets['现实'];
  return (
    <div>
      <div className="style-ref">
        <div className="ri">风格</div>
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 700, marginBottom: 7 }}>风格约束卡（保证一致性 · 所有镜头共享）</div>
          <div className="style-chips">
            {Object.keys(stylePresets).map((k) => (
              <button key={k} className={'schip ' + (k === sr ? 'on' : '')} onClick={() => actions.pickStyleRef(k)}>{k}</button>
            ))}
          </div>
          <div className="muted sm" style={{ marginTop: 8 }}>当前：<b>{sr}</b> · 色调 {sp.tone} · 字体 {sp.font} · 运镜 {sp.cam}</div>
        </div>
      </div>
      {cState.story.length === 0 ? (
        <div className="empty-hint">还没生成分镜。<br /><button className="btn" style={{ marginTop: 12 }} onClick={actions.genStory}>✨ 生成分镜文案</button></div>
      ) : (
        <>
          <div className="sec-title">分镜文案（{cState.story.length} 镜 · 可直接修改）</div>
          {cState.story.map((s, i) => (
            <div key={i} className="shot">
              <div className="num">{String(i + 1).padStart(2, '0')}</div>
              <div className="body">
                <div className="idx"><span className="tag spoken">镜头 {i + 1}</span>
                  <input className="mini" value={s.cam} onChange={(e) => actions.editStory(i, 'cam', e.target.value)} style={{ width: 90 }} placeholder="运镜" />
                </div>
                <div className="ed-row"><label>画面</label><textarea className="ed" rows={2} value={s.desc} onChange={(e) => actions.editStory(i, 'desc', e.target.value)} /></div>
                <div className="ed-row"><label>台词</label><textarea className="ed" rows={2} value={s.dialogue} onChange={(e) => actions.editStory(i, 'dialogue', e.target.value)} /></div>
                <div className="meta"><span>⏱ <input className="mini" value={s.dur} onChange={(e) => actions.editStory(i, 'dur', e.target.value)} style={{ width: 46 }} />秒</span></div>
              </div>
            </div>
          ))}
          <button className="btn" onClick={actions.goImage}>下一步：生成图片 →</button>
        </>
      )}
    </div>
  );
}

function ImageView() {
  const { state, actions } = useApp();
  const { cState } = state;
  if (cState.story.length === 0) return <div className="empty-hint">请先生成分镜。</div>;
  const onAddRef = (i: number) => {
    const inp = document.createElement('input');
    inp.type = 'file'; inp.accept = 'image/*'; inp.multiple = true;
    inp.onchange = () => {
      const files = Array.from(inp.files || []);
      if (!files.length) return;
      let pending = files.length; const out: { name: string; dataUrl: string }[] = [];
      files.forEach((f) => {
        const rd = new FileReader();
        rd.onload = () => { out.push({ name: f.name, dataUrl: String(rd.result) }); if (--pending === 0) actions.addRef(i, out); };
        rd.readAsDataURL(f);
      });
    };
    inp.click();
  };
  return (
    <div>
      <div className="gen-grid">
        {cState.story.map((s, i) => {
          const refs = cState.refs[i] || [];
          const curCat = cState.refCat[i] || 'IP形象';
          const grouped = refCats.filter((c) => refs.some((r) => r.cat === c)).map((c) => ({ c, items: refs.map((r, ri) => ({ r, ri })).filter((o) => o.r.cat === c) }));
          return (
            <div key={i} className="frame-card">
              <div className={'fr ' + (cState.imgs[i] ? '' : 'empty')} style={cState.imgs[i] ? { background: 'linear-gradient(135deg,#6366f1,#8b5cf6)' } : undefined}>{cState.imgs[i] ? '✓ 已生成' : '点击生成'}</div>
              <div className="fb">
                <span className="muted sm">镜头 {i + 1}</span>
                <select className="mini" value={curCat} onChange={(e) => actions.setRefCat(i, e.target.value)}>
                  {refCats.map((c) => <option key={c} value={c}>{c}</option>)}
                </select>
                <button className="btn sm" onClick={() => actions.genImg(i)}>⚡ 生成图片</button>
                <button className="btn sm ghost" onClick={() => onAddRef(i)}>🖼 加参考图{refs.length ? `(${refs.length})` : ''}</button>
              </div>
              <div className="ref-thumbs">
                {grouped.length ? grouped.map((g) => (
                  <div key={g.c} className="ref-group">
                    <div className="ref-g-label">{g.c} · {g.items.length}</div>
                    <div className="ref-row">
                      {g.items.map((o) => (
                        <div key={o.ri} className="ref-thumb">
                          <img src={o.r.dataUrl} alt={o.r.name} />
                          <select className="rcat" value={o.r.cat} onChange={(e) => actions.setRefCatItem(i, o.ri, e.target.value)}>
                            {refCats.map((c) => <option key={c} value={c}>{c}</option>)}
                          </select>
                          <button className="x" onClick={() => actions.delRef(i, o.ri)}>×</button>
                        </div>
                      ))}
                    </div>
                  </div>
                )) : <span className="muted sm" style={{ padding: '2px 0' }}>暂无参考图，选分类后点「加参考图」上传</span>}
              </div>
            </div>
          );
        })}
      </div>
      <div style={{ marginTop: 14 }}><button className="btn" onClick={actions.goFrames}>下一步：首尾帧视频 →</button></div>
    </div>
  );
}

function FramesView() {
  const { state, actions } = useApp();
  const { cState } = state;
  if (cState.story.length === 0) return <div className="empty-hint">请先生成图片。</div>;
  return (
    <div>
      <div className="gen-grid">
        {cState.story.map((s, i) => (
          <div key={i} className="frame-card">
            <div className={'fr ' + (cState.frames[i] ? '' : 'empty')} style={cState.frames[i] ? { background: 'linear-gradient(135deg,#6366f1,#8b5cf6)' } : undefined}>{cState.frames[i] ? '▶ 首尾帧视频' : '首 ░ 尾'}</div>
            <div className="fb"><span className="muted sm">镜头 {i + 1} · {s.dur}s</span>
              <button className="btn sm" onClick={() => actions.genFrames(i)}>🎬 生成视频</button></div>
          </div>
        ))}
      </div>
      <div style={{ marginTop: 14 }}><button className="btn" onClick={actions.goVoice}>下一步：配音+字幕 →</button></div>
    </div>
  );
}

function VoiceView() {
  const { state, actions } = useApp();
  const { cState } = state;
  const allVoices = ['知性女声', '沉稳男声', '活泼童声', '方言·长沙', '科技男声', '温柔姐姐'];
  const selV = cState.voices.map((v) => v.name);
  if (cState.story.length === 0) return <div className="empty-hint">请先生成分镜。</div>;
  return (
    <div>
      <div className="card" style={{ maxWidth: 760 }}>
        <div className="sec-title">配音（TTS · 可多选，每个声音配一个 IP 形象）</div>
        <div className="voice-list">
          {allVoices.map((n) => {
            const on = selV.includes(n);
            const vo = cState.voices.find((x) => x.name === n);
            return (
              <div key={n} className={'voice-item ' + (on ? 'on' : '')}>
                <label className="vc"><input type="checkbox" checked={on} onChange={() => actions.toggleVoice(n)} /> <b>{n}</b></label>
                {on && <span className="ip-in">IP形象 <input className="mini" value={vo ? vo.ip : n} onChange={(e) => actions.setVoiceIP(n, e.target.value)} style={{ width: 120 }} /></span>}
              </div>
            );
          })}
        </div>
        <div className="field" style={{ marginTop: 10 }}>
          <label>语速</label>
          <select onChange={(e) => actions.updOther('voiceSpeed', e.target.value)}><option>正常</option><option>稍快</option><option>稍慢</option></select>
        </div>
        <button className="btn sm" onClick={actions.genVoice}>🔊 生成配音（{cState.voices.length} 声音）</button>
        <div className="wave" style={{ marginTop: 12, height: 54 }}>
          {Array.from({ length: 60 }).map((_, i) => <i key={i} style={{ height: `${cState.voice.ok ? 20 + Math.abs(Math.sin(i * 0.9)) * 70 : 8}%` }} />)}
        </div>
        {cState.voices.length > 0 && <div className="muted sm" style={{ marginTop: 8 }}>已选声音与 IP：{cState.voices.map((v) => v.name + '→' + v.ip).join('； ')}</div>}
      </div>
      <div className="card" style={{ padding: '8px 0', marginTop: 14, maxWidth: 760 }}>
        <div className="sec-title" style={{ padding: '8px 14px 4px' }}>字幕（由文案对齐生成）</div>
        {(cState.subs.length ? cState.subs : defaultSubs()).map((s, i) => (
          <div key={i} className="sub-row"><span className="st">{s.t}</span><span>{s.x}</span></div>
        ))}
      </div>
      <div style={{ marginTop: 14 }}><button className="btn" onClick={actions.goExport}>下一步：合成导出 →</button></div>
    </div>
  );
}

function ExportView() {
  const { state, actions } = useApp();
  return (
    <div className="card" style={{ maxWidth: 760 }}>
      <div className="sec-title">合成与导出</div>
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <div className="field"><label>分辨率</label><select><option>1920×1080</option><option>1280×720</option><option>3840×2160</option></select></div>
        <div className="field"><label>封装</label><select><option>MP4 (H.264)</option><option>MOV</option></select></div>
      </div>
      <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}><input type="checkbox" defaultChecked /> 混音配音</label>
      <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)', marginBottom: 6 }}><input type="checkbox" defaultChecked /> 烧录字幕</label>
      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button className="btn" onClick={() => actions.task('合成中…', 20)}>🚀 开始合成导出</button>
        <button className="btn ghost" onClick={actions.exportCreationJianYing}>🎬 导出剪映工程</button>
      </div>
      <div className="preview-box" style={{ marginTop: 14 }}>成片预览<div className="pbar" /></div>
    </div>
  );
}
