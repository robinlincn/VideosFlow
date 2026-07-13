// 步骤 5：视频解说功能（核心工作台，顶部切换 + 3 模式 + 4 风格选择 + 视角/语言/时长/辅助/4 设置卡 + 开始生成按钮）
// v2.0 重构：替代 v1.0 的 6 步向导分散 + NarrationFlowModal 弹窗

import { useState } from 'react';
import { submitFilmScriptGen } from '../../ipc/providers';
import { useApp } from '../../state/AppContext';
import type { ProgressMsg } from '../../ipc/types';

interface Props {
  videoPath: string;
  videoName: string;
  videoDuration: number;
  rangeStart: number;
  rangeEnd: number;
  styleId: string;
  styleName: string;
  onGenerated: (script: string) => void;
  onBack: () => void;
}

const DURATIONS = [
  { id: 'custom',     label: '自定义',     desc: '' },
  { id: 'brief',      label: '精简版',     desc: '约 1 分钟' },
  { id: 'standard',   label: '标准版',     desc: '约 3 分钟' },
  { id: 'detailed',   label: '详细版',     desc: '约 5 分钟' },
  { id: 'long',       label: '长篇版',     desc: '约 10 分钟' },
  { id: 'epic',       label: '超长版',     desc: '约 20 分钟' },
  { id: 'saga',       label: '史诗版',     desc: '约 30 分钟' },
];

const DURATION_TO_MIN: Record<string, number> = {
  brief: 1, standard: 3, detailed: 5, long: 10, epic: 20, saga: 30, custom: 5,
};

const STYLES = [
  { id: 'sarcastic-suspense', name: '毒舌悬疑', desc: '毒舌剖析师·悬疑案件，伏笔回收逻辑闭环' },
  { id: 'sarcastic-action',  name: '毒舌动作', desc: '毒舌剖析师·动作犯罪，战术拆解+反差套路' },
  { id: 'sarcastic-drama',   name: '毒舌短剧', desc: '毒舌剖析师·下沉短剧，打脸爽点+人性算计' },
  { id: 'custom',              name: '自定义风格', desc: '按您的要求生成' },
];

export default function Step5Narration({
  videoPath, videoName, videoDuration, rangeStart, rangeEnd,
  styleId, styleName, onGenerated, onBack,
}: Props) {
  const { state, actions } = useApp();
  const { settingsState } = state;
  const [mode, setMode] = useState<'ai' | 'custom' | 'imitate'>('ai');
  const [style, setStyle] = useState(styleId); // 默认从步骤 2 带来的风格
  const [view, setView] = useState<'first' | 'third'>('third');
  const [language, setLanguage] = useState('CN');
  const [duration, setDuration] = useState('brief'); // 1 分钟
  const [hint, setHint] = useState('');
  const [busy, setBusy] = useState(false);
  const [taskPct, setTaskPct] = useState(0);
  const [taskMsg, setTaskMsg] = useState('');
  const [analysisMode, setAnalysisMode] = useState(0.1);
  const [voiceId, setVoiceId] = useState('知性女声');
  const [subtitleStyle, setSubtitleStyle] = useState('经典-白字黑边');

  // 顶部切换：解说工作台 ↔ 分镜工作台
  const [topTab, setTopTab] = useState<'narration' | 'storyboard'>('narration');

  const submitGen = async () => {
    setBusy(true);
    setTaskPct(15);
    setTaskMsg('抽取音轨');
    try {
      // 找当前 editingProj（步骤 3 已创建，但步骤 4 之前先要落到 film_projects）
      const proj = state.editingProj;
      if (!proj) {
        // 兜底：没工程时降级为占位
        const fallback = buildFallbackScript(videoName, styleName, DURATION_TO_MIN[duration] || 3);
        setTimeout(() => {
          setBusy(false);
          onGenerated(fallback);
        }, 1000);
        return;
      }
      const targetMin = DURATION_TO_MIN[duration] || 3;
      const minutes = Math.min(10, Math.max(1, targetMin));
      actions.task(`开始生成解说 · ${videoName} · 风格：${styleName} · ${minutes} 分钟`, 30);
      await submitFilmScriptGen(proj.id, (m: ProgressMsg) => {
        setTaskPct(m.progress);
        setTaskMsg(m.message || '');
        if (m.status === 'done') {
          const script = (m.payload as any)?.script || '';
          setBusy(false);
          actions.task('解说生成完成 ✓', 100);
          onGenerated(script);
        } else if (m.status === 'failed') {
          // 降级占位
          const fallback = buildFallbackScript(videoName, styleName, minutes);
          setBusy(false);
          onGenerated(fallback);
          actions.task('生成失败，使用降级文案：' + (m.message || ''), 100);
        }
      });
    } catch (e) {
      setBusy(false);
      const fallback = buildFallbackScript(videoName, styleName, 3);
      onGenerated(fallback);
      console.error('[film-step5] generate failed:', e);
    }
  };

  return (
    <div className="film-step5">
      {/* 顶部切换：解说工作台 ↔ 分镜工作台 */}
      <div className="film-step5__tabs">
        <button
          className={'film-step5__tab' + (topTab === 'narration' ? ' active' : '')}
          onClick={() => setTopTab('narration')}
        >📝 解说工作台</button>
        <button
          className={'film-step5__tab' + (topTab === 'storyboard' ? ' active' : '')}
          onClick={() => { actions.task('分镜工作台在 M5 实现后启用', 100); setTopTab('storyboard'); }}
        >🎬 分镜工作台</button>
      </div>

      {topTab === 'narration' ? (
        <>
          <div className="film-step5__hero">
            <div className="film-step5__hero-title">AI 影视解说生成器</div>
          </div>

          {/* 已选文件卡片 */}
          <div className="film-step5__file-card">
            <div className="film-step5__file-info">
              <span className="film-step5__file-icon">🎬</span>
              <div>
                <div className="film-step5__file-name">{videoName}</div>
                <div className="film-step5__file-duration">完整视频 {fmtDuration(videoDuration)}</div>
              </div>
            </div>
            <button className="btn sm ghost" onClick={onBack}>重新选择</button>
          </div>

          {/* 3 模式 tab */}
          <div className="film-step5__mode-tabs">
            <button
              className={'film-step5__mode-tab' + (mode === 'ai' ? ' active' : '')}
              onClick={() => setMode('ai')}
            >AI 帮我写</button>
            <button
              className={'film-step5__mode-tab' + (mode === 'custom' ? ' active' : '')}
              onClick={() => setMode('custom')}
            >我有文案</button>
            <button
              className={'film-step5__mode-tab' + (mode === 'imitate' ? ' active' : '')}
              onClick={() => setMode('imitate')}
            >AI 仿写</button>
            <span className="film-step5__mode-tag">精彩提取（M5）</span>
          </div>

          {/* 写作风格 */}
          <div className="form-row">
            <label className="form-label">写作风格</label>
            <div className="film-step5__styles">
              {STYLES.map((s) => (
                <div
                  key={s.id}
                  className={'film-step5__style' + (style === s.id ? ' active' : '')}
                  onClick={() => setStyle(s.id)}
                  title={s.desc}
                >
                  <div className="film-step5__style-name">{s.name}</div>
                  {style === s.id && <div className="film-step5__style-desc">{s.desc}</div>}
                </div>
              ))}
              <div className="film-step5__style film-step5__style--custom" onClick={() => actions.task('自定义风格（M5）', 100)}>
                <div className="film-step5__style-name">查看更多 ›</div>
              </div>
            </div>
          </div>

          {/* 视角 */}
          <div className="form-row">
            <label className="form-label">解说视角</label>
            <div className="film-step5__view">
              <button className={'pill' + (view === 'first' ? ' active' : '')} onClick={() => setView('first')}>第一人称</button>
              <button className={'pill' + (view === 'third' ? ' active' : '')} onClick={() => setView('third')}>第三人称</button>
            </div>
          </div>

          {/* 语言 */}
          <div className="form-row">
            <label className="form-label">解说语言</label>
            <div className="film-step5__lang">
              <select className="form-select" value={language} onChange={(e) => setLanguage(e.target.value)} style={{ maxWidth: 200 }}>
                <option value="CN">CN 中文</option>
                <option value="EN">EN English</option>
                <option value="JA">JA 日本語</option>
              </select>
              <span className="muted">语速基准 4.5 字/秒</span>
            </div>
          </div>

          {/* 时长 */}
          <div className="form-row">
            <label className="form-label">解说时长</label>
            <div className="film-step5__durations">
              {DURATIONS.map((d) => (
                <button
                  key={d.id}
                  className={'pill' + (duration === d.id ? ' active' : '')}
                  onClick={() => setDuration(d.id)}
                >
                  {d.label} {d.desc && <span className="muted" style={{ marginLeft: 4 }}>({d.desc})</span>}
                </button>
              ))}
            </div>
          </div>

          {/* 辅助 */}
          <div className="form-row">
            <label className="form-label">解说辅助（选填）</label>
            <textarea
              className="form-textarea"
              rows={2}
              value={hint}
              onChange={(e) => setHint(e.target.value)}
              placeholder="可选，如：结尾加上【这里是XX，关注我带你看更多精彩】"
            />
          </div>

          {/* 进度条 */}
          {busy && (
            <div className="progress-block">
              <div className="progress-bar"><div className="progress-bar__fill" style={{ width: `${taskPct}%` }} /></div>
              <div className="muted">{taskMsg} · {taskPct.toFixed(0)}%</div>
            </div>
          )}

          {/* 底部 4 设置卡 */}
          <div className="film-step5__settings">
            <div className="film-step5__setting-card">
              <div className="film-step5__setting-title">解说模型</div>
              <select className="form-select" defaultValue="default">
                <option value="default">默认（新手指荐）</option>
                <option value="god">上帝视角模式（推）</option>
              </select>
              <button className="btn sm ghost" style={{ marginTop: 6 }} onClick={() => actions.task('测试连通功能在 M5 实现', 100)}>测试连通</button>
            </div>
            <div className="film-step5__setting-card">
              <div className="film-step5__setting-title">分析模式</div>
              <div className="film-step5__setting-row">
                <span>穿插原片</span>
                <input type="range" min={0} max={1} step={0.05} value={analysisMode} onChange={(e) => setAnalysisMode(+e.target.value)} style={{ flex: 1 }} />
                <span className="muted">{Math.round(analysisMode * 100)}%</span>
              </div>
            </div>
            <div className="film-step5__setting-card">
              <div className="film-step5__setting-title">语音克隆</div>
              <select className="form-select" value={voiceId} onChange={(e) => setVoiceId(e.target.value)}>
                <option>知性女声</option>
                <option>磁性男声</option>
                <option>温暖男声</option>
              </select>
            </div>
            <div className="film-step5__setting-card">
              <div className="film-step5__setting-title">字幕样式</div>
              <select className="form-select" value={subtitleStyle} onChange={(e) => setSubtitleStyle(e.target.value)}>
                <option>经典-白字黑边</option>
                <option>简约-无边框</option>
                <option>阴影-黑字阴影</option>
              </select>
            </div>
          </div>

          <button className="btn primary film-step5__start" onClick={submitGen} disabled={busy}>
            {busy ? '生成中...' : '开始生成'}
          </button>
        </>
      ) : (
        <div className="film-step5__placeholder">
          <div className="film-step5__hero-title">🎬 分镜工作台</div>
          <p className="muted">分镜工作台在 M5（创作下）实施后启用。当前先完成解说生成。</p>
          <button className="btn sm" onClick={() => setTopTab('narration')}>← 返回解说工作台</button>
        </div>
      )}

      <div style={{ marginTop: 16 }}>
        <button className="btn sm ghost" onClick={onBack}>‹ 返回</button>
      </div>
    </div>
  );
}

function fmtDuration(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function buildFallbackScript(videoName: string, styleName: string, minutes: number): string {
  const targetChars = minutes * 270;
  const sections = ['开端', '铺垫', '冲突', '高潮', '反转', '结局'];
  const avgChars = Math.floor(targetChars / 6);
  return sections.map((s, i) => {
    const ratio = (i + 1) / 6;
    const start = Math.floor((minutes * 60) * (i / 6));
    const end = Math.floor((minutes * 60) * ((i + 1) / 6));
    return `[${s}] ${fmtSec(start)}-${fmtSec(end)} 【${styleName}】这是【${videoName}】的第 ${i + 1} 段「${s}」占位文案（约 ${avgChars} 字），真实生成时会调用 Agnes LLM 根据 ASR 转写和选定风格生成 6 段式解说。`;
  }).join('\n');
}

function fmtSec(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}
