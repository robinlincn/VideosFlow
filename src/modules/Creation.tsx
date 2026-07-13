import { useApp } from '../state/AppContext';
import { cSteps, stylePresets, refCats } from '../data/mock';
import type { CreationProject } from '../ipc/types';

const STEP_DESC: Record<string, string> = {
  req: '描述大体需求，自动写文案',
  script: 'AI 自动写文案初稿',
  human: '口语化去 AI 味',
  story: '生成可编辑的分镜文案',
  image: '按分镜生成图片（可加分类参考图）',
  frames: '按图片与分镜生成首尾帧视频（M5 待落地）',
  voice: '多声音配音 + 字幕（M5 待落地）',
  export: '合成与导出（M5 待落地）',
};

export default function Creation() {
  const { state, actions } = useApp();
  const { cStage, creationProjects, creationSel } = state;
  const cur = creationProjects.find((p) => p.id === creationSel) || null;

  return (
    <div>
      <div className="muted sm" style={{ marginBottom: 12 }}>{STEP_DESC[cStage]}</div>
      <StepPills current={cStage} onPick={(id) => actions.set({ cStage: id })} />

      {/* 工程列表条 */}
      <div className="card" style={{ marginBottom: 14, padding: 12, display: 'flex', alignItems: 'center', gap: 8 }}>
        <div className="side-label" style={{ flex: '0 0 auto' }}>创作工程</div>
        <select value={creationSel || ''} onChange={(e) => {
          const id = e.target.value;
          actions.set({ creationSel: id, cStage: 'req' });
          if (id) actions.refreshCreation(id);
        }} style={{ flex: 1 }}>
          <option value="">（新建）</option>
          {creationProjects.map((p) => (
            <option key={p.id} value={p.id}>{p.brief.slice(0, 30) || '未命名'} · {p.status}</option>
          ))}
        </select>
        <button className="btn sm ghost" disabled={!creationSel} onClick={() => creationSel && actions.deleteCreation(creationSel)}>删除</button>
      </div>

      {cStage === 'req' && <ReqView />}
      {(cStage === 'script' || cStage === 'human' || cStage === 'story' || cStage === 'image' || cStage === 'frames' || cStage === 'voice' || cStage === 'export') && (
        !cur ? <div className="empty-hint">请先创建创作工程。</div> :
        <>
          {cStage === 'script' && <ScriptView proj={cur} />}
          {cStage === 'human' && <HumanView proj={cur} />}
          {cStage === 'story' && <StoryView proj={cur} />}
          {cStage === 'image' && <ImageView proj={cur} />}
          {cStage === 'frames' && <div className="empty-hint">首尾帧视频生成在 M5 落地（M4 范围外）。</div>}
          {cStage === 'voice' && <div className="empty-hint">多声音配音 + 字幕在 M5 落地（M4 范围外）。</div>}
          {cStage === 'export' && <div className="empty-hint">合成导出在 M5 落地（M4 范围外）。</div>}
        </>
      )}
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
  const { actions } = useApp();
  return (
    <div className="card" style={{ maxWidth: 760 }}>
      <div className="sec-title">大体需求</div>
      <div className="field"><label>主题 / 一句话需求</label>
        <textarea id="brief" rows={4} placeholder="例如：做一个 60 秒的科普短视频，介绍 AI 视频剪辑，风格轻松活泼，面向新手"></textarea>
      </div>
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <div className="field"><label>风格</label><select><option>轻松活泼</option><option>专业沉稳</option><option>温情叙事</option><option>炫酷科技</option></select></div>
        <div className="field"><label>时长</label><select><option>30 秒</option><option>60 秒</option><option>2 分钟</option><option>5 分钟</option></select></div>
        <div className="field"><label>受众</label><select><option>新手</option><option>从业者</option><option>大众</option></select></div>
        <div className="field"><label>平台</label><select><option>抖音</option><option>视频号</option><option>B站</option><option>YouTube</option></select></div>
      </div>
      <button className="btn" onClick={async () => {
        const ta = document.getElementById('brief') as HTMLTextAreaElement | null;
        const brief = ta?.value?.trim() || '';
        if (!brief) { alert('请先填写需求'); return; }
        const id = await actions.createCreation(brief);
        if (id) await actions.genScript(id);
      }}>✨ 创建工程 + 自动写文案</button>
    </div>
  );
}

function ScriptView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  return (
    <div>
      <div className="sec-title">自动写文案（初稿）</div>
      <div className="script-box">{proj.script || state.cState.script || '尚未生成文案…'}</div>
      <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
        <button className="btn sm" disabled={!proj.script} onClick={() => actions.goHuman()}>下一步：去 AI 味 →</button>
        <button className="btn sm ghost" onClick={() => actions.genScript(proj.id)}>🔄 重新生成</button>
      </div>
    </div>
  );
}

function HumanView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { cState, settingsState } = state;
  const pt = settingsState.prompts;
  const selTpl = cState.humanPrompt || 'humanize';
  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
      <div>
        <div className="sec-title">去 AI 味前</div>
        <div className="script-box" style={{ color: 'var(--muted)' }}>{proj.script || state.cState.script}</div>
      </div>
      <div>
        <div className="sec-title">去 AI 味后（口语化）</div>
        <div className="script-box">{proj.humanizedScript || state.cState.human || <span className="muted">点击「去 AI 味」生成</span>}</div>
        <div className="field" style={{ marginTop: 10 }}>
          <label>提示词模板</label>
          <select value={selTpl} onChange={(e) => actions.pickHumanPrompt(e.target.value)}>
            {Object.keys(pt).map((k) => <option key={k} value={k}>{pt[k].name}</option>)}
          </select>
        </div>
        <div className="muted sm" style={{ margin: '4px 0 8px' }}>去 AI 味将使用「{pt[selTpl]?.name || 'humanize'}」模板对文案做口语化改写。</div>
        <button className="btn sm" onClick={() => actions.doHuman(proj.id)}>🪄 去 AI 味</button>
      </div>
      <div style={{ marginTop: 12, gridColumn: '1 / -1' }}>
        <button className="btn" disabled={!proj.humanizedScript} onClick={() => actions.goStory()}>下一步：生成分镜 →</button>
      </div>
    </div>
  );
}

function StoryView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { cState, creationSb } = state;
  const sr = cState.styleRef || '现实';
  const sp = stylePresets[sr] || stylePresets['现实'];
  // 优先用 storyboard.shots（DB 真实态），fallback cState.story
  const shots = (creationSb?.shots && creationSb.shots.length > 0) ? creationSb.shots : cState.story;
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
      {shots.length === 0 ? (
        <div className="empty-hint">还没生成分镜。<br /><button className="btn" style={{ marginTop: 12 }} onClick={() => actions.genStory(proj.id)}>✨ 生成分镜文案</button></div>
      ) : (
        <>
          <div className="sec-title">分镜文案（{shots.length} 镜 · 可直接修改）</div>
          {shots.map((s, i) => (
            <div key={i} className="shot"><div className="body">
              <div className="idx"><span className="tag spoken">镜头 {i + 1}</span>
                <input className="mini" value={s.cam} onChange={(e) => actions.editStory(i, 'cam', e.target.value)} style={{ width: 90 }} placeholder="运镜" />
              </div>
              <div className="ed-row"><label>画面</label><textarea className="ed" rows={2} value={s.desc} onChange={(e) => actions.editStory(i, 'desc', e.target.value)} /></div>
              <div className="ed-row"><label>台词</label><textarea className="ed" rows={2} value={s.dialogue} onChange={(e) => actions.editStory(i, 'dialogue', e.target.value)} /></div>
              <div className="meta"><span>⏱ <input className="mini" type="number" value={s.dur} onChange={(e) => actions.editStory(i, 'dur', e.target.value)} style={{ width: 46 }} />秒</span></div>
            </div></div>
          ))}
          <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
            <button className="btn sm" onClick={() => actions.goImage()}>下一步：生成图片 →</button>
            <button className="btn sm ghost" onClick={() => actions.genStory(proj.id)}>🔄 重新生成分镜</button>
            <button className="btn sm ghost" onClick={() => actions.persistStory(proj.id)}>💾 保存到分镜</button>
          </div>
        </>
      )}
    </div>
  );
}

function ImageView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { cState, creationSb, creationAssets } = state;
  // 优先用 storyboard.shots（DB 真实态），fallback cState.story（编辑态）
  const shots = (creationSb?.shots && creationSb.shots.length > 0) ? creationSb.shots : cState.story;
  const assetsByShot: Record<number, { path: string; size?: number }> = {};
  creationAssets.forEach((a) => { assetsByShot[a.shotId] = { path: a.path }; });
  creationAssets.forEach((a) => { assetsByShot[a.shotId] = { path: a.path }; });
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
  if (shots.length === 0) return <div className="empty-hint">请先生成分镜。</div>;
  return (
    <div>
      <div className="gen-grid">
        {shots.map((s, i) => {
          const refs = cState.refs[i] || [];
          const curCat = cState.refCat[i] || 'IP形象';
          const grouped = refCats.filter((c) => refs.some((r) => r.cat === c)).map((c) => ({ c, items: refs.map((r, ri) => ({ r, ri })).filter((o) => o.r.cat === c) }));
          const asset = assetsByShot[i];
          return (
            <div key={i} className="frame-card">
              <div className={'fr ' + (cState.imgs[i] || asset ? '' : 'empty')} style={(cState.imgs[i] || asset) ? { background: 'linear-gradient(135deg,#6366f1,#8b5cf6)' } : undefined}>
                {(cState.imgs[i] || asset) ? `✓ 已生成${asset ? ` · ${asset.path.split('/').pop()}` : ''}` : '点击生成'}
              </div>
              <div className="fb">
                <span className="muted sm">镜头 {i + 1}</span>
                <select className="mini" value={curCat} onChange={(e) => actions.setRefCat(i, e.target.value)}>
                  {refCats.map((c) => <option key={c} value={c}>{c}</option>)}
                </select>
                <button className="btn sm" onClick={() => actions.genImg(proj.id, i)}>⚡ 生成图片</button>
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
                )) : <span className="muted sm" style={{ padding: '2px 0' }}>暂无参考图</span>}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}