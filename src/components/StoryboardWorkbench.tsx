// 分镜工作台（参考解说猫第 7 步：分镜卡片列表 + 配音/风格/脚本/概览 4 面板）
// M2.5：分镜数据从 film_script_gen 生成的 6 段式文案解析
// 核心交互：单击单元格编辑 / 视频联动 / 配音状态标记 / 智能去重

import { useMemo } from 'react';
import { useApp } from '../state/AppContext';

interface Props {
  script: string;
  duration: number; // 秒
  videoName: string;
  onExport?: () => void;
}

interface Segment {
  index: number;
  section: string;
  start: number;
  end: number;
  text: string;
  hasDub?: boolean;
}

const SECTIONS = ['开端', '铺垫', '冲突', '高潮', '反转', '结局'];

/** 解析 [section] HH:MM-HH:MM text 格式的 6 段式文案 */
function parseScript(script: string, totalDuration: number): Segment[] {
  if (!script) return [];
  const lines = script.split('\n').map((l) => l.trim()).filter(Boolean);
  const segs: Segment[] = [];
  // 优先按 [section] 切
  let fallbackIdx = 0;
  for (const ln of lines) {
    let m = ln.match(/^\[(\S+?)\]\s*(\d{1,2}:\d{2})[-–~](\d{1,2}:\d{2})\s+(.+)$/);
    if (m) {
      const section = m[1];
      const start = toSec(m[2]);
      const end = toSec(m[3]);
      const text = m[4];
      segs.push({ index: segs.length, section, start, end, text });
      continue;
    }
    m = ln.match(/^\[(\S+?)\]\s+(.+)$/);
    if (m) {
      const section = m[1];
      const text = m[2];
      const start = (fallbackIdx / SECTIONS.length) * totalDuration;
      const end = ((fallbackIdx + 1) / SECTIONS.length) * totalDuration;
      segs.push({ index: segs.length, section, start, end, text });
      fallbackIdx++;
      continue;
    }
    // 兜底：无标记 → 自由文本段
    if (segs.length === 0 || segs[segs.length - 1].section) {
      segs.push({ index: segs.length, section: SECTIONS[Math.min(fallbackIdx, SECTIONS.length - 1)] || '开端', start: (fallbackIdx / 6) * totalDuration, end: ((fallbackIdx + 1) / 6) * totalDuration, text: ln, hasDub: false });
    } else {
      segs[segs.length - 1].text += ' ' + ln;
    }
    fallbackIdx++;
  }
  // 标 hasDub = false（初始未配）
  segs.forEach((s) => { s.hasDub = false; });
  return segs;
}

function toSec(s: string): number {
  const p = s.split(':');
  if (p.length !== 2) return 0;
  return +p[0] * 60 + +p[1];
}

function fmt(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

const STYLES = ['电影解说', '电视剧', '综艺', '动漫', '纪录片', '悬疑', '轻松', '激情', '知识', '通用'];

export default function StoryboardWorkbench({ script, duration, videoName, onExport }: Props) {
  const { state, actions } = useApp();
  const { cState } = state;
  const segments = useMemo(() => parseScript(script, duration || cState.human.length > 0 ? duration : 180), [script, duration, cState.human]);

  // 总字数
  const totalChars = segments.reduce((s, x) => s + x.text.length, 0);
  const dubCount = segments.filter((s) => s.hasDub).length;

  return (
    <div className="workbench">
      {/* 顶部工具栏 */}
      <div className="workbench__header">
        <div className="workbench__title">🎬 分镜工作台 · {videoName || '未命名'}</div>
        <div className="workbench__actions">
          <button className="btn sm" onClick={() => actions.task('AI 智能去重（M6 远期）', 100)}>🧹 去重</button>
          <button className="btn sm" onClick={() => actions.task('文案质检（M6 远期）', 100)}>📊 质检</button>
          <button className="btn sm" onClick={() => actions.task('已复制 SRT 字幕（占位）', 100)}>📋 复制字幕</button>
          <button className="btn sm" onClick={() => actions.task('SRT 字幕（占位）', 100)}>📝 导出 SRT</button>
          <button className="btn sm primary" onClick={onExport}>📤 导出</button>
        </div>
      </div>

      <div className="workbench__layout">
        {/* 左侧：分镜卡片列表 */}
        <div className="workbench__segments">
          {segments.length === 0 ? (
            <div className="empty-hint" style={{ padding: 40, textAlign: 'center' }}>
              暂无分镜数据。请先在解说猫弹窗中完成"生成解说文案"步骤。
            </div>
          ) : (
            segments.map((s) => (
              <div key={s.index} className="storyboard-item">
                <div className="storyboard-item__thumb">
                  <div style={{ fontSize: 32 }}>🎬</div>
                  <span className="time-badge">{fmt(s.start)}</span>
                </div>
                <div className="storyboard-item__info">
                  <div className="storyboard-item__time">{fmt(s.start)} - {fmt(s.end)}</div>
                  <div className="storyboard-item__section">[{s.section}]</div>
                  <div className="storyboard-item__text">{s.text}</div>
                </div>
                <div className="storyboard-item__actions">
                  <button className="storyboard-item__action" title={s.hasDub ? '已配音' : '配音'} onClick={() => actions.task('单条配音（M5 远期）', 100)}>🎙</button>
                  <button className="storyboard-item__action" title="编辑" onClick={() => {
                    const newText = prompt('编辑解说词', s.text);
                    if (newText != null) s.text = newText;
                  }}>✏️</button>
                  <button className="storyboard-item__action delete" title="删除" onClick={() => actions.task('删除分镜（M5 远期）', 100)}>🗑</button>
                </div>
              </div>
            ))
          )}
        </div>

        {/* 右侧：4 面板 */}
        <div className="workbench__right">
          {/* 配音管理 */}
          <div className="workbench__panel">
            <div className="workbench__panel-title">🎙 配音管理</div>
            <div className="workbench__panel-body">
              <div className="form-row">
                <label className="form-label">配音引擎</label>
                <select className="form-select">
                  <option>XiaomiMimo TTS（默认）</option>
                  <option>Edge TTS（M5）</option>
                  <option>CosyVoice（远期）</option>
                </select>
              </div>
              <div className="form-row">
                <label className="form-label">音色</label>
                <select className="form-select">
                  <option>晓晓-女（中文）</option>
                  <option>云扬-男（中文）</option>
                </select>
              </div>
              <div className="form-row">
                <label className="form-label">语速</label>
                <input type="range" className="form-input" min={0.5} max={2} step={0.1} defaultValue={1.0} />
              </div>
              <div className="form-row">
                <button className="btn primary" style={{ width: '100%' }} onClick={() => actions.task('批量配音（M5 远期）', 100)}>▶ 批量配音</button>
              </div>
            </div>
          </div>

          {/* 解说风格 */}
          <div className="workbench__panel">
            <div className="workbench__panel-title">🎨 解说风格</div>
            <div className="workbench__panel-body">
              <div className="style-pills">
                {STYLES.map((s) => (
                  <span key={s} className={'style-pill' + (cState.styleRef === s ? ' active' : '')} onClick={() => actions.pickStyleRef(s)}>
                    {s}
                  </span>
                ))}
              </div>
              <div className="muted" style={{ marginTop: 6, fontSize: 11 }}>
                切换风格后，导出时会用对应 prompt 重新生成解说
              </div>
            </div>
          </div>

          {/* 脚本编辑 */}
          <div className="workbench__panel">
            <div className="workbench__panel-title">📝 脚本编辑</div>
            <div className="workbench__panel-body">
              <textarea
                className="form-textarea"
                style={{ minHeight: 100, fontSize: 12 }}
                value={script}
                onChange={() => { /* 受控只读 */ }}
                readOnly
              />
              <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                <button className="btn sm" onClick={() => {
                  navigator.clipboard?.writeText(script);
                  actions.task('已复制到剪贴板 ✓', 100);
                }}>📋 复制</button>
                <button className="btn sm" onClick={() => actions.task('AI 润色（M6 远期）', 100)}>✨ AI 润色</button>
                <button className="btn sm primary" onClick={() => actions.task('匹配（M5 远期）', 100)}>🎯 匹配</button>
              </div>
            </div>
          </div>

          {/* 项目概览 */}
          <div className="workbench__panel">
            <div className="workbench__panel-title">📊 项目概览</div>
            <div className="workbench__panel-body">
              <OverviewRow label="分镜" value={`${segments.length} 段`} pct={segments.length / 6 * 100} color="primary" />
              <OverviewRow label="文案字数" value={`${totalChars}`} pct={Math.min(100, totalChars / 800 * 100)} color="accent" />
              <OverviewRow label="配音进度" value={`${dubCount}/${segments.length}`} pct={segments.length > 0 ? dubCount / segments.length * 100 : 0} color="success" />
              <div className="overview-row">
                <span className="muted">预计时长</span>
                <span className="bold">{Math.round(duration / 60)} 分 {(duration % 60).toFixed(0)} 秒</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function OverviewRow({ label, value, pct, color }: { label: string; value: string; pct: number; color: 'primary' | 'accent' | 'success' }) {
  return (
    <div className="overview-row">
      <div className="overview-row__head">
        <span className="muted">{label}</span>
        <span className="bold">{value}</span>
      </div>
      <div className="overview-row__bar">
        <div className={'overview-row__fill overview-row__fill--' + color} style={{ width: pct + '%' }} />
      </div>
    </div>
  );
}
