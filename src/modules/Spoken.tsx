import { useState, useEffect } from 'react';
import { useApp } from '../state/AppContext';
import { spokenSteps, flowerTpls } from '../data/mock';
import { open } from '@tauri-apps/plugin-dialog';

const assetTypeLabel = (t: string) => ({ image: '🖼 图片', bgm: '🎵 BGM', sfx: '🔊 音效', clip: '🎬 片段' } as Record<string, string>)[t] || '文件';

const issueTypeLabel: Record<string, string> = { gap: '气口', mistake: '口误', repeat: '重复' };

/** 嗅探文件类型（与 Rust sniff_asset_kind 对齐）。 */
function sniffKind(fileName: string): 'image' | 'bgm' | 'sfx' | 'clip' {
  const lower = fileName.toLowerCase();
  const ext = lower.split('.').pop() || '';
  if (['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'].includes(ext)) return 'image';
  if (['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'].includes(ext)) return 'clip';
  if (ext === 'wav') return 'sfx';
  return 'bgm';
}

/** mm:ss → 秒。 */
function toSec(s: string): number {
  if (!s) return 0;
  const p = s.split(':');
  return p.length === 2 ? +p[0] * 60 + +p[1] : +p[0];
}

/** 秒 → mm:ss。 */
function fmtSec(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

export default function Spoken() {
  const { state, actions } = useApp();
  const { spokenVideosDb, spokenSel, spokenStage, spokenEdits, spokenAssets, spokenKeywords, spokenMatches } = state;
  const v = spokenVideosDb.find((x) => x.id === spokenSel) || spokenVideosDb[0];

  return (
    <div className="edit-grid" style={{ gridTemplateColumns: '220px 1fr' }}>
      <div>
        <div className="side-label">口播视频</div>
        <div className="lp">
          {spokenVideosDb.length === 0 && (
            <div className="muted sm" style={{ padding: 8 }}>暂无口播视频，请先上传。</div>
          )}
          {spokenVideosDb.map((x) => (
            <div key={x.id} className={'lp-cat ' + (x.id === spokenSel ? 'active' : '')} onClick={() => { actions.set({ spokenSel: x.id, spokenStage: 'tr' }); actions.refreshSpoken(x.id); }}>
              <span>{x.name}</span>
            </div>
          ))}
        </div>
        <UploadButton />
        <div className="muted sm" style={{ marginTop: 12 }}>
          上传 → 识别音频 → 提取文案；保留纠正（气口/口误/重复 采纳/忽略）与字幕重点/花字烧录。
        </div>
      </div>

      <div>
        {!v && <div className="empty-hint">请先上传口播视频。</div>}
        {v && (
          <>
            <StepPills current={spokenStage} onPick={(id) => actions.set({ spokenStage: id })} />
            {spokenStage === 'upload' && <UploadView v={v} />}
            {(spokenStage === 'tr' || spokenStage === 'upload') && <TrView v={v} />}
            {spokenStage === 'fix' && <FixView videoId={v.id} />}
            {spokenStage === 'match' && <MatchView videoId={v.id} />}
            {spokenStage === 'flw' && <FlwView videoId={v.id} />}
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

function UploadButton() {
  const { actions } = useApp();
  return (
    <button className="btn sm" style={{ marginTop: 14, width: '100%' }} onClick={async () => {
      try {
        const selected = await open({
          multiple: false,
          directory: false,
          filters: [{ name: '视频文件', extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'] }],
          title: '选择口播视频',
        });
        if (!selected) return;
        const filePath = Array.isArray(selected) ? selected[0] : selected;
        const fileName = filePath.replace(/.*[\\/]/, '');
        // 简化：duration 默认 60s（真实应该用 ffprobe 探测；M3 先用占位）
        const durationSec = 60;
        await actions.uploadSpoken(filePath, fileName, durationSec);
      } catch (e) {
        // tauri-plugin-dialog 可能在纯浏览器不可用，回退到 mock
        await actions.uploadSpoken('mock://upload.mp4', '口播视频.mp4', 60);
      }
    }}>⬆ 上传口播视频</button>
  );
}

function UploadView({ v }: { v: any }) {
  return (
    <div className="card" style={{ padding: 14 }}>
      <div className="sec-title">准备识别</div>
      <div className="muted sm">视频：{v.name} · 时长：{fmtSec(v.duration || 0)}</div>
      <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
        <button className="btn sm" onClick={() => /* go to tr */ window.location.reload()}>下一步：识别 →</button>
      </div>
    </div>
  );
}

function TrView({ v }: { v: any }) {
  const { actions } = useApp();
  const transcript = (() => {
    try { return JSON.parse(v.transcript || '[]') as { start: number; end: number; text: string }[]; } catch { return []; }
  })();
  return (
    <>
      <div className="kpis">
        <div className="kpi"><div className="v">{fmtSec(v.duration || 0)}</div><div className="l">时长</div></div>
        <div className="kpi"><div className="v">{transcript.length}</div><div className="l">转写句</div></div>
        <div className="kpi"><div className="v">{v.script ? '✓' : '—'}</div><div className="l">文案提取</div></div>
      </div>
      <div className="card" style={{ padding: 12, marginBottom: 14 }}>
        <div className="wave">{Array.from({ length: 48 }).map((_, i) => <i key={i} style={{ height: `${20 + Math.abs(Math.sin(i * 0.7)) * 70}%` }} />)}</div>
        <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
          <button className="btn sm" onClick={() => actions.transcribe(v.id)}>▶ 识别音频提取文案</button>
          <button className="btn sm ghost" disabled={!v.script} onClick={() => actions.set({ spokenStage: 'fix' })}>下一步：纠正 →</button>
        </div>
      </div>
      {transcript.length > 0 && (
        <div className="card" style={{ padding: 14, marginBottom: 14 }}>
          <div className="sec-title">转写结果</div>
          <div style={{ maxHeight: 220, overflow: 'auto' }}>
            {transcript.map((r, i) => (
              <div key={i} className="tr-line"><span className="t">{r.start ? fmtSec(r.start) : `0:0${i}`}</span><span className="x">{r.text}</span></div>
            ))}
          </div>
        </div>
      )}
      {v.script && (
        <div className="card" style={{ padding: 14 }}>
          <div className="sec-title">提取文案</div>
          <div className="script-box">{v.script}</div>
        </div>
      )}
    </>
  );
}

function FixView({ videoId }: { videoId: string }) {
  const { state, actions } = useApp();
  const { spokenEdits, spokenStage } = state;
  // M3 修复：进入 fix 步骤时主动拉一次最新 edits，确保统计实时刷新
  // （mock 路径下 doMatch 完成后 refreshSpoken 是异步的，FixView 首屏可能读到旧 state）
  useEffect(() => {
    if (spokenStage === 'fix' && videoId) {
      actions.refreshSpoken(videoId);
    }
  }, [spokenStage, videoId, actions]);
  const c: Record<string, number> = { gap: 0, mistake: 0, repeat: 0, acc: 0 };
  spokenEdits.filter((e) => e.videoId === videoId).forEach((i) => {
    c[i.issueType]++;
    if (i.accepted === 1) c.acc++;
  });
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
        <button className="btn sm" onClick={() => actions.doMatch(videoId)}>🤖 智能检测</button>
        <button className="btn sm ok" disabled={spokenEdits.length === 0} onClick={() => actions.acceptAllIssues(videoId)}>✓ 全部采纳</button>
        <button className="btn sm ghost" disabled={spokenEdits.length === 0} onClick={() => actions.ignoreAllIssues(videoId)}>⊘ 全部忽略</button>
        <button className="btn sm ghost" onClick={() => actions.cleanFromAccepted(videoId)}>🧹 生成干净文案</button>
        <button className="btn sm ghost" onClick={() => actions.set({ spokenStage: 'match' })}>下一步：匹配素材 →</button>
      </div>
      {spokenEdits.filter((e) => e.videoId === videoId).map((i) => (
        <div key={i.id} className={'iss-row ' + (i.accepted === 1 ? 'accepted' : i.accepted === -1 ? 'ignored' : '')}>
          <span className={'tag ' + i.issueType}>{issueTypeLabel[i.issueType]}</span>
          <span className="ti">{i.start ? fmtSec(i.start) : '—'}</span>
          <span className="tx">{i.text ? <><del>{i.text}</del> → <ins>{i.suggestion}</ins></> : <span className="muted">{i.suggestion}</span>}</span>
          <span className="acts">
            <button className="btn sm ok" disabled={i.accepted === 1} onClick={() => actions.setIssue(videoId, i.id, true)}>采纳</button>
            <button className="btn sm ghost" disabled={i.accepted === -1} onClick={() => actions.setIssue(videoId, i.id, false)}>忽略</button>
          </span>
        </div>
      ))}
      {state.spokenVideosDb.find((x) => x.id === videoId)?.cleanScript && (
        <div className="card" style={{ padding: 14, marginTop: 14 }}>
          <div className="sec-title">干净文案预览</div>
          <div className="script-box">{state.spokenVideosDb.find((x) => x.id === videoId)?.cleanScript}</div>
        </div>
      )}
    </div>
  );
}

function MatchView({ videoId }: { videoId: string }) {
  const { state, actions } = useApp();
  const { spokenAssets, spokenKeywords, spokenMatches } = state;
  const myAssets = spokenAssets.filter((a) => a.videoId === videoId);
  const myKw = spokenKeywords.filter((k) => k.videoId === videoId);
  const myMatches = spokenMatches.filter((m) => m.videoId === videoId);
  return (
    <div>
      <div className="muted sm" style={{ marginBottom: 10 }}>从口播文案抽取关键词 → 与素材库做最简贪心匹配；可在关键词列表和素材库编辑后重新匹配。</div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <button className="btn sm" onClick={() => actions.extractKeywords(videoId)}>🔑 抽取关键词</button>
        <button className="btn sm ghost" disabled={myKw.length === 0 || myAssets.length === 0} onClick={() => actions.matchAssets(videoId)}>🎯 智能匹配素材</button>
        <button className="btn sm ghost" disabled={myMatches.length === 0} onClick={() => actions.applyAllMatch(videoId)}>✓ 全部应用</button>
        <button className="btn sm ghost" onClick={() => actions.set({ spokenStage: 'flw' })}>下一步：花字字幕 →</button>
      </div>
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">素材库（{myAssets.length} 项）</div>
        <div className="asset-list">
          {myAssets.length === 0 && <span className="muted sm">无素材，请上传</span>}
          {myAssets.map((a) => (
            <div key={a.id} className="asset-chip">
              <span className={'at ' + a.type}>{assetTypeLabel(a.type)}</span>{a.name}
              <button className="x" style={{ marginLeft: 6, border: 'none', background: 'transparent', color: 'var(--muted)', cursor: 'pointer' }} onClick={() => actions.delAsset(videoId, a.id)}>×</button>
            </div>
          ))}
        </div>
        <UploadAssetButton videoId={videoId} />
      </div>
      <div className="card" style={{ padding: 14, marginTop: 14 }}>
        <div className="sec-title">关键词（{myKw.length} 个）</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {myKw.length === 0 && <span className="muted sm">尚未抽取</span>}
          {myKw.map((k) => (
            <span key={k.id} className="asset-chip"><span className="at keyword">关键词</span>{k.text} <span className="muted sm" style={{ marginLeft: 4 }}>·{Math.round(k.weight * 100)}%</span></span>
          ))}
        </div>
      </div>
      <div className="card" style={{ padding: 14, marginTop: 14 }}>
        <div className="sec-title">匹配结果{myMatches.length ? `（${myMatches.filter((m) => m.applied).length}/${myMatches.length} 已应用）` : ''}</div>
        {myMatches.length === 0 ? (
          <div className="muted sm">尚未匹配。先抽取关键词 + 上传素材，再点智能匹配。</div>
        ) : myMatches.map((m) => {
          const asset = myAssets.find((a) => a.id === m.assetId);
          return (
            <div key={m.id} className="m-row">
              <span className="m-seg">{m.segStart ? fmtSec(m.segStart) : '—'}</span>
              <span className="m-text">{m.segText}</span>
              <span className="m-kw">🔑 {m.keyword}</span>
              <span className="m-asset">{asset?.name || '(素材缺失)'}</span>
              <span className="acts"><button className={'btn sm ' + (m.applied ? 'ok' : '')} onClick={() => actions.toggleMatch(videoId, m.id)}>{m.applied ? '✓ 已应用' : '应用'}</button></span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function UploadAssetButton({ videoId }: { videoId: string }) {
  const { actions } = useApp();
  return (
    <button className="btn sm ghost" style={{ marginTop: 10 }} onClick={async () => {
      try {
        const selected = await open({
          multiple: false, directory: false,
          filters: [{ name: '素材', extensions: ['png', 'jpg', 'jpeg', 'webp', 'mp3', 'wav', 'mp4', 'mov'] }],
          title: '选择素材',
        });
        if (!selected) return;
        const filePath = Array.isArray(selected) ? selected[0] : selected;
        const fileName = filePath.replace(/.*[\\/]/, '');
        const kind = sniffKind(fileName);
        await actions.uploadAsset(videoId, fileName, kind, filePath);
      } catch {
        // 浏览器回退：mock
        await actions.uploadAsset(videoId, '素材.png', 'image', 'mock://asset.png');
      }
    }}>+ 上传素材</button>
  );
}

function FlwView({ videoId }: { videoId: string }) {
  const { state, actions } = useApp();
  const v = state.spokenVideosDb.find((x) => x.id === videoId);
  const tpl = flowerTpls.find((t: any) => t.id === state.editorState.flower) || flowerTpls[0];
  const myKw = state.spokenKeywords.filter((k) => k.videoId === videoId);
  const transcript = (() => {
    try { return JSON.parse(v?.transcript || '[]') as { start: number; end: number; text: string }[]; } catch { return []; }
  })();
  return (
    <div className="edit-grid">
      <div className="card" style={{ padding: 14 }}>
        <div className="sec-title">字幕重点 + 花字预览</div>
        <div style={{ maxHeight: 320, overflow: 'auto' }}>
          {transcript.map((r, i) => {
            const matchedKw = myKw.find((k) => r.text.includes(k.text));
            let html = r.text;
            if (matchedKw) {
              html = html.split(matchedKw.text).join(`<span class="flw ${tpl.cls}">${matchedKw.text}</span>`);
            }
            return <div key={i} className="tr-line"><span className="t">{r.start ? fmtSec(r.start) : `0:0${i}`}</span><span className="x" dangerouslySetInnerHTML={{ __html: html }} /></div>;
          })}
        </div>
        <div style={{ marginTop: 12, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn sm" disabled={myKw.length === 0} onClick={() => actions.burnFlower(videoId, state.editorState.flower)}>🔥 烧录到视频</button>
          <button className="btn sm ghost" onClick={() => actions.exportSpoken(videoId, true, state.editorState.flower)}>⬇ 导出干净片段（含花字）</button>
          <button className="btn sm ghost" onClick={() => actions.exportSpoken(videoId, false, state.editorState.flower)}>⬇ 导出干净片段（无花字）</button>
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