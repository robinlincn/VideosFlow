import { useApp } from '../state/AppContext';
import { spokenSteps, flowerTpls } from '../data/mock';

const assetTypeLabel = (t: string) => ({ image: '🖼 图片', bgm: '🎵 BGM', sfx: '🔊 音效', clip: '🎬 片段' } as Record<string, string>)[t] || '文件';

export default function Spoken() {
  const { state, actions } = useApp();
  const { spokenVideos, spokenSel, spokenStage } = state;
  const v = spokenVideos.find((x) => x.id === spokenSel) || spokenVideos[0];

  return (
    <div className="edit-grid" style={{ gridTemplateColumns: '220px 1fr' }}>
      <div>
        <div className="side-label">口播视频</div>
        <div className="lp">
          {spokenVideos.map((x) => (
            <div key={x.id} className={'lp-cat ' + (x.id === spokenSel ? 'active' : '')} onClick={() => actions.set({ spokenSel: x.id, spokenStage: 'upload' })}>
              <span>{x.name}</span>
            </div>
          ))}
        </div>
        <button className="btn sm" style={{ marginTop: 14, width: '100%' }} onClick={actions.uploadSpoken}>⬆ 上传口播视频</button>
        <div className="muted sm" style={{ marginTop: 12 }}>
          上传 → 识别音频 → 提取文案；保留纠正（气口/口误/重复 采纳/忽略）与字幕重点/花字烧录。
        </div>
      </div>

      <div>
        {!v && <div className="empty-hint">请先上传口播视频。</div>}
        {v && (
          <>
            <StepPills current={spokenStage} onPick={(id) => actions.set({ spokenStage: id })} />
            {(spokenStage === 'upload' || spokenStage === 'tr') && <UploadView v={v} />}
            {spokenStage === 'fix' && <FixView v={v} />}
            {spokenStage === 'match' && <MatchView v={v} />}
            {spokenStage === 'flw' && <FlwView v={v} />}
          </>
        )}
      </div>
    </div>
  );
}

function StepPills({ current, onPick }: { current: string; onPick: (id: string) => void; }) {
  return (
    <div className="steps">
      {spokenSteps.map((s, i) => {
        const cur = spokenSteps.findIndex((x) => x.id === current);
        const cls = s.id === current ? 'active' : i < cur ? 'done' : '';
        return <div key={s.id} className={'step ' + cls} onClick={() => onPick(s.id)}>{(i + 1)} {s.name}</div>;
      })}
    </div>
  );
}

function UploadView({ v }: { v: any }) {
  const { actions } = useApp();
  return (
    <>
      <div className="kpis">
        <div className="kpi"><div className="v">{v.dur}</div><div className="l">时长</div></div>
        <div className="kpi"><div className="v">{v.tr.length}</div><div className="l">转写句</div></div>
        <div className="kpi"><div className="v">{v.script ? '✓' : '—'}</div><div className="l">文案提取</div></div>
      </div>
      <div className="card" style={{ padding: 12, marginBottom: 14 }}>
        <div className="wave">{Array.from({ length: 48 }).map((_, i) => <i key={i} style={{ height: `${20 + Math.abs(Math.sin(i * 0.7)) * 70}%` }} />)}</div>
        <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
          <button className="btn sm" onClick={() => actions.transcribe(v.id)}>▶ 识别音频提取文案</button>
          <button className="btn sm ghost" disabled={!v.script} onClick={() => actions.set({ spokenStage: 'fix' })}>下一步：纠正 →</button>
        </div>
      </div>
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">素材库（可选）</div>
        <div className="asset-list">
          {(v.assets && v.assets.length) ? v.assets.map((a: any) => (
            <div key={a.name} className="asset-chip"><span className={'at ' + a.type}>{assetTypeLabel(a.type)}</span>{a.name}
              <button className="x" style={{ marginLeft: 6, border: 'none', background: 'transparent', color: 'var(--muted)', cursor: 'pointer' }} onClick={() => actions.delAsset(v.id, a.name)}>×</button>
            </div>
          )) : <span className="muted sm">无素材，可上传</span>}
        </div>
        <button className="btn sm ghost" style={{ marginTop: 10 }} onClick={() => actions.uploadAsset(v.id)}>+ 上传素材</button>
      </div>
      {v.script && <div className="card" style={{ padding: 14, marginTop: 14 }}><div className="sec-title">提取文案</div><div className="script-box">{v.script}</div></div>}
    </>
  );
}

function FixView({ v }: { v: any }) {
  const { actions } = useApp();
  const c: Record<string, number> = { gap: 0, mistake: 0, repeat: 0, acc: 0 };
  v.issues.forEach((i: any) => { c[i.type]++; if (i.accepted) c.acc++; });
  const typeLabel: Record<string, string> = { gap: '气口', mistake: '口误', repeat: '重复' };
  return (
    <div>
      <div className="muted sm" style={{ marginBottom: 10 }}>检测建议默认"采纳/忽略"双选，标记可回溯，<b>不自动破坏原片</b>。</div>
      <div className="iss-stat">
        <div className="stat gap"><div className="v">{c.gap}</div><div className="l">气口</div></div>
        <div className="stat mis"><div className="v">{c.mistake}</div><div className="l">口误/卡顿</div></div>
        <div className="stat rep"><div className="v">{c.repeat}</div><div className="l">重复/啰嗦</div></div>
        <div className="stat acc"><div className="v">{c.acc}</div><div className="l">已采纳</div></div>
      </div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <button className="btn sm ok" onClick={actions.acceptAllIssues}>✓ 全部采纳</button>
        <button className="btn sm ghost" onClick={actions.ignoreAllIssues}>⊘ 全部忽略</button>
        <button className="btn sm ghost" onClick={actions.cleanFromAccepted}>🧹 生成干净文案</button>
        <button className="btn sm ghost" onClick={() => actions.set({ spokenStage: 'match' })}>下一步：匹配素材 →</button>
      </div>
      {v.issues.map((i: any) => (
        <div key={i.id} className={'iss-row ' + (i.accepted == null ? '' : i.accepted ? 'accepted' : 'ignored')}>
          <span className={'tag ' + i.type}>{typeLabel[i.type]}</span>
          <span className="ti">{i.ti}</span>
          <span className="tx">{i.tx ? <><del>{i.tx}</del> → <ins>{i.suggestion.replace(/^删除|^合并|^保留|，.*$/g, '')}</ins></> : <span className="muted">{i.suggestion}</span>}</span>
          <span className="acts">
            <button className="btn sm ok" onClick={() => actions.setIssue(v.id, i.id, true)}>采纳</button>
            <button className="btn sm ghost" onClick={() => actions.setIssue(v.id, i.id, false)}>忽略</button>
          </span>
        </div>
      ))}
      {v.cleanScript && <div className="card" style={{ padding: 14, marginTop: 14 }}><div className="sec-title">干净文案预览</div><div className="script-box">{v.cleanScript}</div></div>}
    </div>
  );
}

function MatchView({ v }: { v: any }) {
  const { actions } = useApp();
  const matched = v.matchResults;
  return (
    <div>
      <div className="muted sm" style={{ marginBottom: 10 }}>根据口播文案与关键词，从素材库智能匹配最合适的素材，并应用到对应片段。</div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <button className="btn sm" onClick={() => actions.doMatch(v.id)}>🤖 智能匹配素材</button>
        <button className="btn sm ghost" disabled={!matched} onClick={() => actions.applyAllMatch(v.id)}>✓ 全部应用</button>
        <button className="btn sm ghost" onClick={() => actions.set({ spokenStage: 'flw' })}>下一步：花字字幕 →</button>
      </div>
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">素材库（{(v.assets || []).length} 项）</div>
        <div className="asset-list">{(v.assets && v.assets.length) ? v.assets.map((a: any) => <div key={a.name} className="asset-chip"><span className={'at ' + a.type}>{assetTypeLabel(a.type)}</span>{a.name}</div>) : <span className="muted sm">无素材</span>}</div>
      </div>
      <div className="card" style={{ padding: 14, marginTop: 14 }}>
        <div className="sec-title">匹配结果{matched ? `（${matched.filter((m: any) => m.applied).length}/${matched.length} 已应用）` : ''}</div>
        {matched ? matched.map((m: any) => (
          <div key={m.seg} className="m-row">
            <span className="m-seg">{m.seg}</span>
            <span className="m-text">{m.text}{m.text.length >= 12 ? '…' : ''}</span>
            <span className="m-kw">{m.kw ? '🔑 ' + m.kw : ''}</span>
            <span className="m-asset">{m.asset}</span>
            <span className="acts"><button className={'btn sm ' + (m.applied ? 'ok' : '')} onClick={() => actions.toggleMatch(v.id, m.seg)}>{m.applied ? '✓ 已应用' : '应用'}</button></span>
          </div>
        )) : <div className="muted sm">尚未匹配。点击「🤖 智能匹配素材」。</div>}
      </div>
    </div>
  );
}

function FlwView({ v }: { v: any }) {
  const { state, actions } = useApp();
  const tpl = flowerTpls.find((t: any) => t.id === state.editorState.flower) || flowerTpls[0];
  return (
    <div className="edit-grid">
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">字幕重点 + 花字预览</div>
        <div style={{ maxHeight: 320, overflow: 'auto' }}>
          {v.tr.map((r: any) => {
            const kws = v.keywords || [];
            let html = r.x;
            kws.forEach((k: string) => { html = html.split(k).join(`<span class="kw">${k}</span>`); });
            const safe = kws[0] ? kws[0].replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : null;
            if (tpl && safe) html = html.replace(new RegExp(safe), `<span class="flw ${tpl.cls}">${safe}</span>`);
            return <div key={r.t} className="tr-line"><span className="t">{r.t}</span><span className="x" dangerouslySetInnerHTML={{ __html: html }} /></div>;
          })}
        </div>
        <div style={{ marginTop: 12, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn sm" onClick={actions.burnFlower}>🔥 烧录到视频</button>
          <button className="btn sm ghost" onClick={actions.exportSpoken}>⬇ 导出干净口播片段</button>
          <button className="btn sm ghost" onClick={actions.exportSpokenJianYing}>🎬 导出剪映工程</button>
        </div>
        <div className="muted sm" style={{ marginTop: 6 }}>导出为剪映草稿（draft_content.json：视频轨 + 字幕/花字轨），导入剪映后可继续精修。</div>
      </div>
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">花字模板</div>
        <div className="flow-grid">
          {flowerTpls.map((t: any) => (
            <div key={t.id} className={'flow-card ' + (state.editorState.flower === t.id ? 'active' : '')} onClick={() => actions.pickSpokenFlower(t.id)}>
              <div className="pt">{t.name}</div>
              <div><span className={'pp ' + t.cls}>{t.demo}</span></div>
              <div className="ds">{t.desc}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
