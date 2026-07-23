import { useEffect, useState } from 'react';
import { useApp } from '../state/AppContext';
import { cSteps, stylePresets, refCats } from '../data/mock';
import type { CreationProject } from '../ipc/types';
import { getVideoServerBase, initVideoServer } from '../ipc/client';
import { listVoices } from '../ipc/providers';
import { open as openDialog } from '@tauri-apps/plugin-dialog';

const STEP_DESC: Record<string, string> = {
  req: '描述大体需求，自动写文案',
  script: 'AI 自动写文案初稿',
  human: '口语化去 AI 味',
  story: '生成可编辑的分镜文案',
  image: '按分镜生成图片（可加分类参考图）',
  frames: '按首帧图生成首尾帧视频（可加尾帧做过渡）',
  voice: '多声音配音 + 字幕',
  export: '合成与导出成片',
};

/** 本地媒体预览源：桌面版经 fileserver（127.0.0.1）按需 Range 加载，浏览器态原样返回。 */
function toSrc(p: string): string {
  if (!p) return '';
  if (/^[a-zA-Z]:\\/.test(p) || p.startsWith('file://')) {
    const base = getVideoServerBase();
    return base ? `${base}/file?path=${encodeURIComponent(p)}` : '';
  }
  if (p.startsWith('blob:') || p.startsWith('http://') || p.startsWith('https://')) return p;
  return p;
}

/** 把分镜源统一为「DB 态优先、编辑态回退」，并附带数组下标 i。 */
type ViewShot = { index?: number; desc: string; dialogue: string; dur: number; cam: string; start?: number; end?: number; style?: string; _i: number };
function useShots(): { shots: ViewShot[]; ready: boolean } {
  const { state } = useApp();
  const { creationSb, cState } = state;
  const [ready, setReady] = useState(false);
  useEffect(() => { initVideoServer().then(() => setReady(true)); }, []);
  const raw = (creationSb?.shots && creationSb.shots.length > 0) ? creationSb.shots : cState.story;
  const shots = raw.map((s, i) => ({ ...(s as object), _i: i }) as ViewShot);
  return { shots, ready };
}

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
          {cStage === 'frames' && <FramesView proj={cur} />}
          {cStage === 'voice' && <VoiceView proj={cur} />}
          {cStage === 'export' && <ExportView proj={cur} />}
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
  const shots = (creationSb?.shots && creationSb.shots.length > 0) ? creationSb.shots : cState.story;
  const assetsByShot: Record<number, { path: string; size?: number }> = {};
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
      <div style={{ marginTop: 14, display: 'flex', gap: 8 }}>
        <button className="btn" disabled={!creationAssets.some((a) => a.kind === 'image')} onClick={() => actions.goFrames()}>下一步：首尾帧视频 →</button>
      </div>
    </div>
  );
}

// ===========================================================================
// M5-① 首尾帧视频
// ===========================================================================
function FramesView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { creationAssets, creationManifest, cState } = state;
  const { shots, ready } = useShots();
  const assetsByShot: Record<number, string> = {};
  creationAssets.forEach((a) => { if (a.kind === 'image') assetsByShot[a.shotId] = a.path; });
  const man = creationManifest;

  const onPickTail = (shotIdx: number) => {
    openDialog({ multiple: false, filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }] }).then((p) => {
      if (p && !Array.isArray(p)) actions.setCreationTail(shotIdx, p as string);
    });
  };

  if (shots.length === 0) return <div className="empty-hint">请先在「分镜」与「图片」步准备好镜头与首帧图。</div>;

  const generatedCount = shots.filter((s) => man && man.clips[String(s.index ?? s._i)]).length;
  const allHaveImg = shots.every((s) => assetsByShot[s.index ?? s._i]);

  return (
    <div>
      <div className="card" style={{ marginBottom: 14, padding: 12, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <div className="muted sm">共 {shots.length} 镜 · 已生成片段 {generatedCount}/{shots.length}</div>
        <div style={{ flex: 1 }} />
        <button className="btn" disabled={!allHaveImg} onClick={() => actions.genFrames(proj.id)}>🎬 生成首尾帧视频</button>
        <button className="btn sm ghost" disabled={generatedCount === 0} onClick={() => actions.goVoice()}>下一步：配音+字幕 →</button>
      </div>
      {!allHaveImg && (
        <div className="muted sm" style={{ marginBottom: 10, color: 'var(--warn, #b45309)' }}>⚠ 还有镜头未生成首帧图，请回到「图片」步生成后再生成视频。</div>
      )}

      <div className="gen-grid">
        {shots.map((s) => {
          const shotIdx = s.index ?? s._i;
          const img = assetsByShot[shotIdx];
          const clip = man?.clips[String(shotIdx)];
          const tail = cState.tails[shotIdx];
          return (
            <div key={s._i} className="frame-card">
              <div className={'fr ' + (img ? '' : 'empty')} style={img ? { backgroundImage: `url(${ready ? toSrc(img) : ''})`, backgroundSize: 'cover', backgroundPosition: 'center' } : undefined}>
                {!img && '缺少首帧图'}
                {img && <span className="fr-badge">镜头 {s._i + 1} 首帧</span>}
              </div>
              <div className="fb" style={{ flexWrap: 'wrap' }}>
                <span className="muted sm">镜头 {s._i + 1} · {s.dur || 4}s</span>
                <button className="btn sm ghost" onClick={() => onPickTail(shotIdx)}>🖼 尾帧{tail ? '✓' : ''}</button>
                {tail && <button className="btn sm ghost" onClick={() => actions.clearCreationTail(shotIdx)}>✕ 取消尾帧</button>}
              </div>
              {clip ? (
                <video key={clip} className="fr-video" controls src={ready ? toSrc(clip) : ''} style={{ width: '100%', borderRadius: 8, marginTop: 8 }} />
              ) : (
                <div className="muted sm" style={{ padding: '6px 0', textAlign: 'center' }}>未生成片段</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ===========================================================================
// M5-② 配音 + 字幕
// ===========================================================================
const VOICE_FALLBACK = [
  { id: 'mimo_default', name: '默认音色（mimo_default）' },
  { id: 'mimo_zh_female', name: '知性女声（如失败用默认）' },
  { id: 'mimo_zh_male', name: '磁性男声（如失败用默认）' },
  { id: 'mimo_warm_male', name: '温暖男声（如失败用默认）' },
];

function VoiceView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { creationManifest, cState } = state;
  const { shots, ready } = useShots();
  const [voices, setVoices] = useState<{ id: string; name: string }[]>(VOICE_FALLBACK);
  useEffect(() => {
    listVoices().then((v) => { if (v && v.length) setVoices(v.map((x) => ({ id: x.id, name: x.name }))); }).catch(() => undefined);
  }, []);
  const man = creationManifest;

  if (shots.length === 0) return <div className="empty-hint">请先生成分镜与首尾帧视频。</div>;

  const dubbedCount = shots.filter((s) => man && man.audios[String(s.index ?? s._i)]).length;

  return (
    <div>
      <div className="card" style={{ marginBottom: 14, padding: 12, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <div className="field" style={{ margin: 0, minWidth: 240 }}>
          <label>配音音色</label>
          <select value={cState.voiceName} onChange={(e) => actions.setCreationVoice(e.target.value)}>
            {voices.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
        </div>
        <div className="muted sm">共 {shots.length} 镜 · 已配音 {dubbedCount}/{shots.length}</div>
        <div style={{ flex: 1 }} />
        <button className="btn" onClick={() => actions.genVoice(proj.id)}>🔊 生成配音</button>
        <button className="btn sm ghost" disabled={shots.length === 0} onClick={() => actions.downloadCreationSrt(proj.id)}>⬇ 下载字幕 SRT</button>
        <button className="btn sm ghost" disabled={dubbedCount === 0} onClick={() => actions.goExport()}>下一步：导出 →</button>
      </div>

      <div className="gen-grid">
        {shots.map((s) => {
          const shotIdx = s.index ?? s._i;
          const wav = man?.audios[String(shotIdx)];
          return (
            <div key={s._i} className="frame-card">
              <div className="fb" style={{ alignItems: 'flex-start', flexDirection: 'column', gap: 6 }}>
                <span className="tag spoken">镜头 {s._i + 1} · 台词</span>
                <div className="script-box" style={{ fontSize: 13, margin: 0, maxHeight: 96, overflow: 'auto' }}>{s.dialogue || <span className="muted">（无台词，跳过）</span>}</div>
              </div>
              {wav ? (
                <div style={{ padding: '8px 0' }}>
                  <audio key={wav} controls src={ready ? toSrc(wav) : ''} style={{ width: '100%' }} />
                </div>
              ) : (
                <div className="muted sm" style={{ padding: '8px 0', textAlign: 'center' }}>未生成配音</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ===========================================================================
// M5-③ 导出成片
// ===========================================================================
const SUB_STYLES = [
  { id: 'standard', name: '标准白字（黑描边）' },
  { id: 'highlight', name: '重点强调（黄底）' },
  { id: 'big', name: '大字标题' },
];

function ExportView({ proj }: { proj: CreationProject }) {
  const { state, actions } = useApp();
  const { creationManifest, cState } = state;
  const { shots, ready } = useShots();
  const [subStyle, setSubStyle] = useState('standard');
  const man = creationManifest;
  const final = man?.exported || cState.exportedPath;

  if (shots.length === 0) return <div className="empty-hint">请先完成分镜、首尾帧视频与配音。</div>;

  const clipCount = shots.filter((s) => man && man.clips[String(s.index ?? s._i)]).length;
  const audioCount = shots.filter((s) => man && man.audios[String(s.index ?? s._i)]).length;

  return (
    <div>
      <div className="card" style={{ marginBottom: 14, padding: 14 }}>
        <div className="sec-title">合成设置</div>
        <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', alignItems: 'end' }}>
          <div className="field">
            <label>字幕样式</label>
            <select value={subStyle} onChange={(e) => setSubStyle(e.target.value)}>
              {SUB_STYLES.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
            </select>
          </div>
          <div className="muted sm">
            片段 {clipCount}/{shots.length} · 配音 {audioCount}/{shots.length}
          </div>
        </div>
        <div style={{ marginTop: 12, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn" disabled={clipCount === 0} onClick={() => actions.exportCreation(proj.id, subStyle)}>📦 导出成片 MP4</button>
          <button className="btn sm ghost" disabled={shots.length === 0} onClick={() => actions.downloadCreationSrt(proj.id)}>⬇ 下载字幕 SRT</button>
          <button className="btn sm ghost" onClick={() => actions.goVoice()}>← 返回配音</button>
        </div>
      </div>

      {final ? (
        <div className="card" style={{ padding: 14 }}>
          <div className="sec-title">成片预览</div>
          <video key={final} controls src={ready ? toSrc(final) : ''} style={{ width: '100%', maxHeight: 520, borderRadius: 10, background: '#000' }} />
          <div className="muted sm" style={{ marginTop: 8 }}>路径：{final}</div>
          <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
            <a className="btn sm" href={ready ? toSrc(final) : '#'} download onClick={(e) => { if (!ready) e.preventDefault(); }}>⬇ 下载成片</a>
            <button className="btn sm ghost" onClick={() => actions.downloadCreationSrt(proj.id)}>⬇ 下载字幕 SRT</button>
          </div>
        </div>
      ) : (
        <div className="empty-hint">尚未导出成片。点击「导出成片 MP4」开始合成（拼接镜头 + 混入配音 + 烧录字幕）。</div>
      )}
    </div>
  );
}
