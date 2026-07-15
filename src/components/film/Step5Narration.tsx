// 步骤 5：视频解说功能（核心工作台，顶部切换 + 3 模式 + 4 风格选择 + 视角/语言/时长/辅助/4 设置卡 + 开始生成按钮）
// v2.0 重构：替代 v1.0 的 6 步向导分散 + NarrationFlowModal 弹窗

import { useState, useEffect } from 'react';
import { submitFilmScriptGen, getFilmAnalysis, submitFilmVideoAnalysis } from '../../ipc/providers';
import { useApp } from '../../state/AppContext';
import type { ProgressMsg, FilmScriptGenOptions } from '../../ipc/types';
import VideoAnalysisModal from './VideoAnalysisModal';

interface Props {
  videoPath: string;
  videoName: string;
  videoDuration: number;
  rangeStart: number;
  rangeEnd: number;
  styleId: string;
  styleName: string;
  projectId: string;
  onGenerated: (script: string) => void;
  onBack: () => void;
  onGotoStoryboard: () => void;
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
  styleId, styleName, projectId, onGenerated, onBack, onGotoStoryboard,
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
  const [model, setModel] = useState('default');

  // M2.6：影片视频分析（在「开始生成」时触发，十步进度）
  const [analysisOpen, setAnalysisOpen] = useState(false);
  const [analysisStep, setAnalysisStep] = useState(0);
  const [analysisFailed, setAnalysisFailed] = useState(false);
  const [analysisFailReason, setAnalysisFailReason] = useState('');
  const [analysisReport, setAnalysisReport] = useState<string | null>(null);
  const [analysisHadError, setAnalysisHadError] = useState(false);

  // 影片理解报告：优先展示本次「开始生成」时分析的结果；重新进入解说工作台时从库回填
  const [analysisText, setAnalysisText] = useState<string | undefined>(undefined);
  useEffect(() => {
    if (projectId && !projectId.startsWith('local-')) {
      getFilmAnalysis(projectId)
        .then((r) => { if (r) setAnalysisText(r); })
        .catch(() => {});
    }
  }, [projectId]);

  // 顶部切换：解说工作台 ↔ 分镜工作台
  const [topTab, setTopTab] = useState<'narration' | 'storyboard'>('narration');
  const [result, setResult] = useState<string | null>(null);
  const [asrFailed, setAsrFailed] = useState(false);
  const [asrReason, setAsrReason] = useState('');

  // 阶段一：影片视频分析（十步进度），返回最终报告文本（失败则返回空串）
  const runAnalysisPhase = (): Promise<string> =>
    new Promise<string>((resolve) => {
      setAnalysisOpen(true);
      setAnalysisStep(0);
      setAnalysisFailed(false);
      setAnalysisFailReason('');
      setAnalysisReport(null);
      submitFilmVideoAnalysis(
        projectId,
        {
          videoPath: videoPath || '',
          title: videoName || styleName || '未命名视频',
          styleName,
          start: rangeStart,
          end: rangeEnd,
        },
        (m: ProgressMsg) => {
          const st = (m.payload as any)?.step;
          if (typeof st === 'number') setAnalysisStep(st);
          if (m.status === 'done') {
            const rep = (m.payload as any)?.report;
            const reportStr = typeof rep === 'string' ? rep : '';
            setAnalysisReport(reportStr);
            setAnalysisText(reportStr);
            setAnalysisOpen(false);
            resolve(reportStr);
          } else if (m.status === 'failed') {
            setAnalysisFailed(true);
            setAnalysisFailReason(m.message || '分析失败');
            setAnalysisHadError(true);
            setAnalysisOpen(false);
            resolve(''); // 分析失败时仍按所选参数 + ASR 生成解说
          }
        },
      ).catch((err: any) => {
        setAnalysisFailed(true);
        setAnalysisFailReason(String(err?.message || err));
        setAnalysisHadError(true);
        setAnalysisOpen(false);
        resolve('');
      });
    });

  // 阶段二：结合影片分析结果 + 所选参数，生成解说文案
  const runNarrationPhase = (report: string): Promise<void> =>
    new Promise<void>((resolve) => {
      const targetMin = DURATION_TO_MIN[duration] || 3;
      const seconds = Math.round(targetMin * 60);
      const langCode = language === 'CN' ? 'zh' : language === 'EN' ? 'en' : language === 'JA' ? 'ja' : 'zh';
      const opts: FilmScriptGenOptions = {
        videoPath: videoPath || '',
        title: videoName || styleName || '未命名视频',
        style,
        styleName,
        language: langCode,
        duration: seconds,
        hint,
        mode,
        view,
        model,
        analysisMode,
        voiceId,
        subtitleStyle,
        analysis: report,
      };
      setTaskPct(0);
      setTaskMsg('生成解说文案');
      submitFilmScriptGen(projectId, opts, (m: ProgressMsg) => {
        setTaskPct(m.progress);
        setTaskMsg(m.message || '');
        if (m.status === 'done') {
          const payload = m.payload as any;
          const script = payload?.script || '';
          setAsrFailed(!!payload?.asrFailed);
          setAsrReason(payload?.asrReason || '');
          setBusy(false);
          setResult(script);
          resolve();
        } else if (m.status === 'failed') {
          const fallback = buildFallbackScript(videoName, styleName, targetMin);
          setBusy(false);
          setResult(fallback);
          resolve();
        }
      });
    });

  const submitGen = async () => {
    setBusy(true);
    setAsrFailed(false);
    setAsrReason('');
    setResult(null);
    setAnalysisHadError(false);
    try {
      // 「我有文案」模式：直接使用用户文案，无需影片视频分析 / LLM
      if (mode === 'custom' && hint.trim()) {
        setBusy(false);
        setResult(hint.trim());
        return;
      }
      // 阶段一：影片视频分析（十步进度）→ 阶段二：结合分析结果 + 所选参数生成解说文案
      const report = await runAnalysisPhase();
      await runNarrationPhase(report);
    } catch (e) {
      setBusy(false);
      const fallback = buildFallbackScript(videoName, styleName, 3);
      setResult(fallback);
      console.error('[film-step5] generate failed:', e);
    }
  };

  // 供 VideoAnalysisModal 使用的总体进度（按步数折算百分比）
  const analysisProgress: ProgressMsg | null = analysisOpen
    ? {
        taskId: 'analysis',
        progress: (analysisStep / 10) * 100,
        status: analysisFailed ? 'failed' : 'running',
        message: analysisFailed ? analysisFailReason : undefined,
        payload: { step: analysisStep },
      }
    : null;

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
          onClick={() => onGotoStoryboard()}
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

          {/* M2.6：影片视频分析总结报告 */}
          {analysisText ? (
            <details className="film-step5__analysis" open>
              <summary>🎞 影片理解报告（点击「开始生成」时由多模态大模型分析）</summary>
              <pre className="film-step5__analysis-body">{analysisText}</pre>
            </details>
          ) : null}

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
              <select className="form-select" value={model} onChange={(e) => setModel(e.target.value)}>
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

          <button className="btn primary film-step5__start" onClick={submitGen} disabled={busy || !!result}>
            {busy ? '生成中...' : result ? '已生成 ✓' : '开始生成'}
          </button>

          {/* 生成结果展示面板 */}
          {result && !busy && (
            <div className="film-step5__result">
              <div className="film-step5__result-head">
                <span className="film-step5__result-title">解说文案已生成 ✓</span>
                <div className="film-step5__chips">
                  <span className="chip">{countSections(result)} 段</span>
                  <span className="chip">{result.replace(/\s/g, '').length} 字</span>
                  <span className="chip">约 {estMin(result)} 分钟</span>
                  <span className="chip">{styleName}</span>
                  <span className="chip">{language === 'CN' ? '中文' : language === 'EN' ? 'English' : language === 'JA' ? '日本語' : language}</span>
                </div>
              </div>
              {asrFailed && (
                <div className="film-step5__warn">
                  ⚠ ASR 转写未成功（视频无语音轨、未配置 ASR 服务或网络异常），以下文案为根据标题与所选风格<b>自由创作</b>，可能与视频实际内容不符。
                  {asrReason && (
                    <div className="film-step5__warn-reason">原因：{asrReason}</div>
                  )}
                  建议：到「设置」配置 XiaomiMimo ASR 密钥，或在上方补充「解说辅助」后再生成。
                </div>
              )}
              {!asrFailed && analysisHadError && (
                <div className="film-step5__warn">
                  ⚠ 影片视频分析未成功（原因：{analysisFailReason || '未知'}），已基于所选参数与视频语音（ASR）生成解说文案，可能与画面实际内容存在偏差。建议检查「设置 → 接口」中的多模态大模型（Ollama 等）配置后重试。
                </div>
              )}
              {/* 结构化解说文案展示（带时间节点的分段卡片） */}
              <div className="film-step5__script">
                {parseScriptSegments(result).map((seg, idx) => (
                  <div key={idx} className="film-step5__seg">
                    <div className="film-step5__seg-head">
                      {seg.section && <span className="film-step5__seg-tag">{seg.section}</span>}
                      {seg.start && seg.end && (
                        <span className="film-step5__seg-time">{seg.start} – {seg.end}</span>
                      )}
                    </div>
                    <div className="film-step5__seg-body">{seg.dialogue}</div>
                  </div>
                ))}
              </div>
              <div className="film-step5__result-actions">
                <button className="btn primary" onClick={() => onGenerated(result)}>进入分镜工作台 →</button>
                <button className="btn ghost" onClick={() => { setResult(null); setAsrFailed(false); setAsrReason(''); }}>重新生成</button>
              </div>
            </div>
          )}
        </>
      ) : (
        <div className="film-step5__placeholder">
          <div className="film-step5__hero-title">🎬 分镜工作台</div>
          <p className="muted">分镜工作台在 M5（创作下）实施后启用。当前先完成解说生成。</p>
          <button className="btn sm" onClick={() => setTopTab('narration')}>← 返回解说工作台</button>
        </div>
      )}

      {/* M2.6：影片视频分析弹窗（点击「开始生成」时触发，十步进度） */}
      {analysisOpen && (
        <VideoAnalysisModal
          open={analysisOpen}
          progress={analysisProgress}
          currentStep={analysisStep}
          failed={analysisFailed}
          failReason={analysisFailReason}
          report={analysisReport}
          onContinue={() => setAnalysisOpen(false)}
          onClose={() => setAnalysisOpen(false)}
        />
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

/** 解析解说文案为结构化分段：[section] start-end dialogue 或 start-end dialogue */
interface ScriptSegment {
  section: string;    // 段落标签（开端/铺垫/…），可能为空
  start: string;      // "0:00"
  end: string;        // "0:30"
  dialogue: string;   // 解说词正文
  raw: string;        // 原始行（用于回退）
}

function parseScriptSegments(script: string): ScriptSegment[] {
  const lines = script.split('\n').filter((l) => l.trim());
  const segments: ScriptSegment[] = [];
  // 匹配 [section] start-end dialogue 或 start-end dialogue
  const re = /^\[([^\]]+)\]\s+(\S+?)-(\S+?)\s+(.+)$/;   // 带段落标签
  const rePlain = /^(\S+?)-(\S+?)\s*(.+)$/;               // 无段落标签
  for (const line of lines) {
    const m = line.match(re);
    if (m) {
      segments.push({ section: m[1], start: m[2], end: m[3], dialogue: m[4].trim(), raw: line });
      continue;
    }
    const mp = line.match(rePlain);
    if (mp) {
      segments.push({ section: '', start: mp[1], end: mp[2], dialogue: mp[3].trim(), raw: line });
      continue;
    }
    // 无法解析的行作为纯文本段
    segments.push({ section: '', start: '', end: '', dialogue: line.trim(), raw: line });
  }
  return segments;
}

function countSections(s: string): number {
  return parseScriptSegments(s).length;
}

function estMin(s: string): number {
  const chars = s.replace(/\s/g, '').length;
  return Math.max(1, Math.round(chars / 4.5 / 60));
}
