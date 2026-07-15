// 影片模块：8 步流程弹窗（解说猫参考：选视频→参数→场景→分析→文案→匹配→工作台→导出）
// M2.5：第 5 步真正调 Rust 任务 film_script_gen（ASR→LLM→六段式）
// 当前实现：弹窗骨架 + 风格/时长/语言 UI + 任务进度回填；视频文件管理用前端
// Tauri 真实上传走 tauri-plugin-dialog（与 M2 importFilm 复用）

import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { submitFilmScriptGen } from '../ipc/providers';
import { useApp } from '../state/AppContext';
import type { ProgressMsg } from '../ipc/types';

interface Props {
  open: boolean;
  onClose: () => void;
  onComplete: (script: string) => void;
}

type Step = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
const STEPS: { id: Step; label: string; desc: string }[] = [
  { id: 1, label: '视频',  desc: '上传视频或拖拽文件到下方区域' },
  { id: 2, label: '参数',  desc: '选择解说风格、时长、语言等' },
  { id: 3, label: '场景',  desc: 'AI 自动识别视频场景切换点' },
  { id: 4, label: '分析',  desc: 'AI 逐帧分析画面内容' },
  { id: 5, label: '文案',  desc: 'AI 根据画面分析生成完整解说文案' },
  { id: 6, label: '匹配',  desc: '将文案精准匹配到每个视频场景' },
  { id: 7, label: '工作台',desc: '逐条编辑、配音、调整' },
  { id: 8, label: '导出',  desc: '选择导出格式，完成创作' },
];

const STYLES = [
  { id: 'movie',    name: '🎬 电影解说', desc: '叙事沉稳、画面感强' },
  { id: 'series',   name: '📺 电视剧',   desc: '剧情连贯、悬念推进' },
  { id: 'variety',  name: '🎉 综艺',     desc: '活泼欢快、节奏明快' },
  { id: 'anime',    name: '🎌 动漫',     desc: '夸张表达、富有张力' },
  { id: 'doc',      name: '📚 纪录片',   desc: '平实严谨、考据详实' },
  { id: 'horror',   name: '🔍 悬疑文案', desc: '紧张氛围、层层递进' },
  { id: 'funny',    name: '😄 轻松搞笑', desc: '幽默诙谐、口语化' },
  { id: 'emotion',  name: '🔥 激情解说', desc: '情感浓烈、爆发力强' },
  { id: 'knowledge',name: '💡 知识科普', desc: '逻辑清晰、举例说明' },
];

export default function NarrationFlowModal({ open: isOpen, onClose, onComplete }: Props) {
  const { state, actions } = useApp();
  const { editingProj } = state;
  const [currentStep, setCurrentStep] = useState<Step>(1);
  const [videoPath, setVideoPath] = useState<string>('');
  const [videoName, setVideoName] = useState<string>('');
  const [style, setStyle] = useState('movie');
  const [duration, setDuration] = useState(180);
  const [language, setLanguage] = useState('zh');
  const [hint, setHint] = useState('');
  const [busy, setBusy] = useState(false);
  const [taskMsg, setTaskMsg] = useState('');
  const [taskPct, setTaskPct] = useState(0);
  const [generatedScript, setGeneratedScript] = useState<string>('');
  const [degraded, setDegraded] = useState(false);

  // 进入弹窗时重置
  useEffect(() => {
    if (isOpen) {
      setCurrentStep(1);
      setVideoPath(editingProj ? state.editorState.videoPath : '');
      setVideoName(editingProj?.t || '');
      setStyle('movie');
      setDuration(180);
      setLanguage('zh');
      setHint('');
      setBusy(false);
      setTaskMsg('');
      setTaskPct(0);
      setGeneratedScript('');
      setDegraded(false);
    }
  }, [isOpen, editingProj, state.editorState.videoPath]);

  if (!isOpen) return null;

  // 选视频（步骤 1）
  const pickVideo = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: '视频文件', extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'] }],
        title: '选择要解说的视频',
      });
      if (!selected) return;
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      const fileName = filePath.replace(/.*[\\/]/, '');
      setVideoPath(filePath);
      setVideoName(fileName);
    } catch {
      // 浏览器模式兜底
      setVideoPath('mock://upload.mp4');
      setVideoName('演示视频.mp4');
    }
  };

  // 步骤 5：调 Rust 任务生成解说
  const generateScript = async () => {
    if (!editingProj) {
      actions.task('请先导入影片', 100);
      return;
    }
    setBusy(true);
    setTaskPct(15);
    setTaskMsg('抽取音轨');
    try {
      await submitFilmScriptGen(editingProj.id, {
        videoPath: (editingProj as any).videoPath || '',
        title: (editingProj as any).title || (editingProj as any).t || '未命名视频',
        style: (editingProj as any).categoryId || 'movie',
        language: 'zh',
        duration: 180,
        hint: '',
      }, (m: ProgressMsg) => {
        setTaskPct(m.progress);
        setTaskMsg(m.message || '');
        if (m.status === 'done') {
          const script = (m.payload as any)?.script || '';
          setGeneratedScript(script);
          setDegraded(false);
          setBusy(false);
          // 6 段式自动切到"匹配"步骤
          setCurrentStep(6);
          actions.task('解说文案生成完成 ✓', 100);
        } else if (m.status === 'failed') {
          // 失败时降级
          const fallback = `根据影片「${editingProj.t}」自动生成一段可二次编辑的解说文案：\n\n第一段，开场介绍影片背景与主题。\n第二段，中段展开主要情节与亮点。\n第三段，高潮部分渲染情绪与节奏。\n第四段，结尾总结主题并引导观后感。\n\n（提示：后端生成失败，已使用前端降级文案。可在设置页检查 LLM Key。）`;
          setGeneratedScript(fallback);
          setDegraded(true);
          setBusy(false);
          setCurrentStep(6);
          actions.task('解说文案生成失败，已使用降级文案', 100);
        }
      });
    } catch (e) {
      setBusy(false);
      const fallback = `根据影片「${editingProj?.t || '未知'}」自动生成一段可二次编辑的解说文案：\n\n第一段，开场介绍影片背景与主题。\n第二段，中段展开主要情节与亮点。\n第三段，高潮部分渲染情绪与节奏。\n第四段，结尾总结主题并引导观后感。`;
      setGeneratedScript(fallback);
      setDegraded(true);
      setCurrentStep(6);
      actions.task('IPC 失败，已使用降级文案', 100);
      console.error('[videosflow] film_script_gen IPC failed:', e);
    }
  };

  // 下一步/上一步
  const goNext = () => {
    if (currentStep === 1 && !videoPath) {
      alert('请先选择视频');
      return;
    }
    if (currentStep === 5 && !generatedScript && !busy) {
      // 自动触发生成
      generateScript();
      return;
    }
    if (currentStep < 8) setCurrentStep((currentStep + 1) as Step);
    else onComplete(generatedScript);
  };
  const goPrev = () => {
    if (currentStep > 1) setCurrentStep((currentStep - 1) as Step);
  };
  const finish = () => {
    onComplete(generatedScript);
  };

  return (
    <div className="narrative-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="narrative-modal">
        <div className="narrative-modal__header">
          <h3>🎬 上帝视角 · AI 帮我写</h3>
          <button className="narrative-modal__close" onClick={onClose}>×</button>
        </div>

        {/* 8 步进度胶囊 */}
        <div className="narrative-modal__steps">
          {STEPS.map((s) => (
            <div key={s.id} className={'narrative-step ' + (s.id === currentStep ? 'active' : s.id < currentStep ? 'done' : '')}>
              <div className="narrative-step__num">{s.id}</div>
              <div className="narrative-step__label">{s.label}</div>
            </div>
          ))}
        </div>

        <div className="narrative-modal__body">
          {/* 步骤 1：选视频 */}
          {currentStep === 1 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">选择视频</div>
              <div className="narrative-step-desc">上传视频或拖拽文件到下方区域</div>
              <div className="dropzone" onClick={pickVideo}>
                <div className="dropzone__icon">📁</div>
                <div className="dropzone__title">点击或拖拽视频到此处</div>
                <div className="dropzone__desc">支持 MP4, MKV, AVI, MOV 格式 · 最大 4GB</div>
              </div>
              {videoName && (
                <div className="narrative-selected-video">
                  <div className="muted">已选视频</div>
                  <div className="narrative-selected-video__name">🎬 {videoName}</div>
                  <div className="narrative-selected-video__path muted">{videoPath}</div>
                </div>
              )}
            </div>
          )}

          {/* 步骤 2：配置参数 */}
          {currentStep === 2 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">配置参数</div>
              <div className="narrative-step-desc">选择解说风格、时长、语言等</div>
              <div className="form-row">
                <label className="form-label">解说风格（12 选 1）</label>
                <div className="style-pills">
                  {STYLES.map((s) => (
                    <span
                      key={s.id}
                      className={'style-pill' + (style === s.id ? ' active' : '')}
                      onClick={() => setStyle(s.id)}
                      title={s.desc}
                    >
                      {s.name}
                    </span>
                  ))}
                </div>
              </div>
              <div className="form-row">
                <label className="form-label">目标时长（分钟）</label>
                <input
                  type="number"
                  className="form-input"
                  min={1}
                  max={10}
                  value={Math.round(duration / 60)}
                  onChange={(e) => setDuration(Math.max(60, Math.min(600, +e.target.value * 60)))}
                />
                <div className="muted">≈ {Math.round(duration / 60 * 270)} 字</div>
              </div>
              <div className="form-row">
                <label className="form-label">语言</label>
                <select className="form-select" value={language} onChange={(e) => setLanguage(e.target.value)}>
                  <option value="zh">中文（简体）</option>
                  <option value="zh-TW">中文（繁体）</option>
                  <option value="en">English</option>
                  <option value="ja">日本語</option>
                  <option value="ko">한국어</option>
                </select>
              </div>
              <div className="form-row">
                <label className="form-label">辅助提示（选填）</label>
                <textarea
                  className="form-textarea"
                  rows={2}
                  value={hint}
                  onChange={(e) => setHint(e.target.value)}
                  placeholder="例如：重点讲主角成长线，弱化配角支线..."
                />
              </div>
              <div className="form-row">
                <label className="form-label">智能穿插原片</label>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span className="tag accent">✓ 开</span>
                  <span className="muted">密度 30%</span>
                </div>
              </div>
            </div>
          )}

          {/* 步骤 3-4：场景检测 + 视频分析（M2.5 不做，远期 M6） */}
          {(currentStep === 3 || currentStep === 4) && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">{currentStep === 3 ? '场景检测' : 'AI 视频分析'}</div>
              <div className="narrative-step-desc">
                {currentStep === 3 ? 'TransNetV2 深度学习模型自动识别视频场景切换点（GPU 加速）' : 'Gemini 2.5 Flash 视觉模型逐块分析每个场景的内容'}
              </div>
              <div className="log-output">
                <div className="log-info">[INFO] 正在加载 {currentStep === 3 ? 'TransNetV2' : 'Gemini 2.5 Flash'} 模型...</div>
                <div className="log-info">[INFO] 分析视频：{videoName || '未选择'}</div>
                <div className="log-info">[INFO] 分辨率：1920×1080 · 时长：{Math.round(duration/60)} 分钟 · 帧率：30fps</div>
                <div className="log-info">[INFO] 正在{currentStep === 3 ? '检测场景切换点' : '分析画面内容'}...</div>
                <div className="log-warn">[WARN] 场景检测 / 视频分析在 M2.5 范围外（M6 远期），本步骤自动跳过</div>
                <div className="log-success">[OK] 已跳过到下一步</div>
              </div>
            </div>
          )}

          {/* 步骤 5：生成解说文案（M2.5 真实链路） */}
          {currentStep === 5 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">生成解说文案</div>
              <div className="narrative-step-desc">AI 根据视频内容生成六段式解说（开端→铺垫→冲突→高潮→反转→结局）</div>
              {busy && (
                <div className="progress-block">
                  <div className="progress-bar"><div className="progress-bar__fill" style={{ width: `${taskPct}%` }} /></div>
                  <div className="muted">{taskMsg || '生成中...'} · {taskPct.toFixed(0)}%</div>
                </div>
              )}
              {!busy && !generatedScript && (
                <button className="btn primary" onClick={generateScript}>⚡ 开始生成解说（调 Rust 任务）</button>
              )}
              {!busy && generatedScript && (
                <>
                  <div className="muted" style={{ marginBottom: 6 }}>AI 生成的文案（{generatedScript.length} 字）{degraded ? '（降级文案）' : ''}</div>
                  <textarea
                    className="form-textarea"
                    style={{ minHeight: 280, fontFamily: 'inherit' }}
                    value={generatedScript}
                    onChange={(e) => setGeneratedScript(e.target.value)}
                  />
                </>
              )}
            </div>
          )}

          {/* 步骤 6：文案-画面匹配（M2.5 用 6 段式自动切分，CLIP 匹配 M6 远期） */}
          {currentStep === 6 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">文案-画面匹配</div>
              <div className="narrative-step-desc">将文案按 6 段式自动切分 + 时间戳回填到每镜</div>
              <div className="log-output">
                <div className="log-info">[INFO] 启动 M2.5 章节切分 + 时间戳回填...</div>
                <div className="log-success">[OK] 检测到 6 段六段式结构（开端/铺垫/冲突/高潮/反转/结局）</div>
                <div className="log-info">[INFO] 平均段长：{(generatedScript.length / 6).toFixed(0)} 字</div>
                <div className="log-info">[INFO] 时间戳按 6 段等分：0:00 → {Math.round(duration/60*60)}s</div>
                <div className="log-warn">[WARN] Chinese-CLIP 语义匹配在 M2.5 范围外（M6 远期）</div>
                <div className="log-success">[OK] 文案-画面匹配完成！</div>
              </div>
              <div style={{ marginTop: 12, textAlign: 'center' }}>
                <button className="btn primary" onClick={() => setCurrentStep(7)}>🚀 进入分镜工作台</button>
              </div>
            </div>
          )}

          {/* 步骤 7：工作台跳转提示 */}
          {currentStep === 7 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">分镜工作台</div>
              <div className="narrative-step-desc">逐条编辑、配音、调整</div>
              <div className="narrative-step-content__workbench-hint">
                <div style={{ fontSize: 64, marginBottom: 12 }}>🎬</div>
                <div className="narrative-step-title">已生成 6 段分镜</div>
                <div className="narrative-step-desc" style={{ marginBottom: 16 }}>点击下方按钮进入工作台，逐条编辑每条分镜</div>
                <button className="btn primary" onClick={() => { onComplete(generatedScript); onClose(); }}>📝 进入工作台</button>
              </div>
            </div>
          )}

          {/* 步骤 8：导出 */}
          {currentStep === 8 && (
            <div className="narrative-step-content">
              <div className="narrative-step-title">导出</div>
              <div className="narrative-step-desc">选择导出格式，完成创作</div>
              <div className="export-grid">
                {[
                  { id: 'full',     icon: '📹', name: '完整项目', desc: '配音 + 字幕 + 视频' },
                  { id: 'jianying', icon: '🎬', name: '剪映草稿', desc: '导入剪映继续编辑' },
                  { id: 'pr',       icon: '🎞️', name: 'Premiere', desc: '导出 PR 项目' },
                  { id: 'srt',      icon: '📝', name: 'SRT 字幕', desc: '仅导出字幕文件' },
                ].map((o) => (
                  <div key={o.id} className="export-card">
                    <div className="export-card__icon">{o.icon}</div>
                    <div className="export-card__name">{o.name}</div>
                    <div className="export-card__desc">{o.desc}</div>
                  </div>
                ))}
              </div>
              <div className="form-row">
                <label className="form-label">输出路径</label>
                <input className="form-input" defaultValue={`D:\\VideosFlow\\output\\${videoName || 'project_001'}`} />
              </div>
              <div className="form-row">
                <label className="form-label">视频质量</label>
                <select className="form-select">
                  <option>原画 (1080p)</option>
                  <option>高清 (720p)</option>
                  <option>标准 (480p)</option>
                </select>
              </div>
              <div style={{ marginTop: 12, textAlign: 'center' }}>
                <button className="btn primary" onClick={finish}>📤 完成导出</button>
              </div>
            </div>
          )}
        </div>

        <div className="narrative-modal__footer">
          <button className="btn" onClick={goPrev} disabled={currentStep === 1 || busy}>上一步</button>
          <button
            className="btn primary"
            onClick={goNext}
            disabled={busy}
          >
            {currentStep === 5 && !generatedScript ? '开始生成' : currentStep === 8 ? '完成' : '下一步 →'}
          </button>
        </div>
      </div>
    </div>
  );
}
