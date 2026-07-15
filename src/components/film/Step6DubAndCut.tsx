// 步骤 6：视频配音剪辑 = 分镜工作台（剪辑脚本表 + 工具栏 + 视频预览 + 导出）
// v2.0 重构：替代 v1.0 的 StoryboardWorkbench。分镜可编辑/删除/排序并持久化回草稿。

import { useEffect, useState } from 'react';
import { useApp } from '../../state/AppContext';
import { submitFilmExport } from '../../ipc/providers';
import type { ProgressMsg } from '../../ipc/types';

interface Props {
  script: string;
  videoName: string;
  projectId: string;
  totalDuration: number;
  rangeStart: number;
  rangeEnd: number;
  onBack: () => void;
  onSwitchToNarration: () => void;
  onScriptChange?: (script: string) => void;
}

interface Segment {
  index: number;
  section: string;
  start: number;
  end: number;
  text: string;
  role: string;
}

const SECTIONS = ['开端', '铺垫', '冲突', '高潮', '反转', '结局'];

function parseScript(script: string, totalDuration: number, startOffset: number): Segment[] {
  if (!script) return [];
  const lines = script.split('\n').map((l) => l.trim()).filter(Boolean);
  const segs: Segment[] = [];
  let i = 0;
  for (const ln of lines) {
    let m = ln.match(/^\[(\S+?)\]\s*(\d{1,2}):(\d{2})[-–~](\d{1,2}):(\d{2})\s+(.+)$/);
    if (m) {
      const section = m[1];
      const start = +m[2] * 60 + +m[3];
      const end = +m[4] * 60 + +m[5];
      const text = m[6].replace(/^【[^】]+】/, '').trim();
      segs.push({ index: i++, section, start: start + startOffset, end: end + startOffset, text, role: '主角' });
      continue;
    }
    m = ln.match(/^\[(\S+?)\]\s+(.+)$/);
    if (m) {
      const section = m[1];
      const text = m[2].replace(/^【[^】]+】/, '').trim();
      const dur = totalDuration || 180;
      const start = (segs.length / 6) * dur + startOffset;
      const end = ((segs.length + 1) / 6) * dur + startOffset;
      segs.push({ index: i++, section, start, end, text, role: '主角' });
      continue;
    }
    if (segs.length > 0) {
      segs[segs.length - 1].text += ' ' + ln;
    } else {
      const dur = totalDuration || 180;
      segs.push({ index: i++, section: SECTIONS[0] || '开端', start: startOffset, end: startOffset + dur / 6, text: ln, role: '主角' });
    }
  }
  return segs;
}

function fmt(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}.${String(Math.floor((sec % 1) * 100)).padStart(2, '0')}`;
}
function fmtShort(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}
function fmtDur(sec: number): string {
  if (sec >= 60) {
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m}分${s.toFixed(1)}秒`;
  }
  return `${sec.toFixed(1)}秒`;
}
function segsToScript(segs: Segment[]): string {
  return segs.map((s) => `[${s.section}] ${fmtShort(s.start)}-${fmtShort(s.end)} ${s.text}`).join('\n');
}

const TOOLBAR = [
  { id: 'voice',      icon: '🎵', label: '音色库',     primary: false },
  { id: 'batch_dub',  icon: '🎬', label: '批量配音',   primary: true  },
  { id: 'translate',  icon: '🌐', label: '翻译至',     primary: false },
  { id: 'do_trans',   icon: '⏵', label: '开始翻译',   primary: false },
  { id: 'history',    icon: '📚', label: '历史版本',   primary: false },
  { id: 'role_swap',  icon: '👤', label: '角色替换',   primary: false },
  { id: 'copy_sub',   icon: '📋', label: '复制字幕',   primary: false },
  { id: 'import_sub', icon: '📥', label: '导入字幕',   primary: false },
  { id: 'export_srt', icon: '💾', label: '导出 SRT',   primary: false },
  { id: 'import_dub', icon: '🎧', label: '导入整段配音', primary: false },
  { id: 'regen',      icon: '🔄', label: '重新生成',   primary: false },
  { id: 'dedup',      icon: '🧹', label: 'AI 智能去重', primary: false },
  { id: 'qa',         icon: '✅', label: '文案质检',   primary: false },
];

export default function Step6DubAndCut({
  script, videoName, projectId, totalDuration, rangeStart, rangeEnd, onBack, onSwitchToNarration, onScriptChange,
}: Props) {
  const { actions } = useApp();
  const [segs, setSegs] = useState<Segment[]>(() => parseScript(script, totalDuration, rangeStart));
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [editingText, setEditingText] = useState('');
  const [subtitleStyle, setSubtitleStyle] = useState('经典-白字黑边');
  const [exportBusy, setExportBusy] = useState(false);
  const [exportMsg, setExportMsg] = useState('');

  // 从解说工作台带入新脚本时重新解析
  useEffect(() => {
    setSegs(parseScript(script, totalDuration, rangeStart));
    setEditingIdx(null);
  }, [script, totalDuration, rangeStart]);

  const persist = (next: Segment[]) => {
    setSegs(next);
    onScriptChange?.(segsToScript(next));
  };

  const handleEdit = (i: number, text: string) => {
    setEditingIdx(i);
    setEditingText(text);
  };
  const commitEdit = () => {
    if (editingIdx === null) return;
    const next = segs.map((s, idx) => (idx === editingIdx ? { ...s, text: editingText } : s));
    persist(next);
    setEditingIdx(null);
  };
  const removeSegment = (i: number) => {
    const next = segs.filter((_, idx) => idx !== i).map((s, idx) => ({ ...s, index: idx }));
    persist(next);
  };
  const moveSegment = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= segs.length) return;
    const next = [...segs];
    [next[i], next[j]] = [next[j], next[i]];
    next.forEach((s, idx) => { s.index = idx; });
    persist(next);
  };
  const generateSrt = () => {
    const srt = segs.map((s, i) => `${i + 1}\n${fmt(s.start)} --> ${fmt(s.end)}\n${s.text}\n`).join('\n');
    const blob = new Blob([srt], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url; a.download = 'subtitles.srt';
    a.click();
    URL.revokeObjectURL(url);
    actions.task('SRT 字幕已下载', 100);
  };
  const exportJianying = async () => {
    setExportBusy(true);
    setExportMsg('准备导出');
    try {
      await submitFilmExport(projectId || 'local', {
        hw: false,
        resolution: '1080p',
        burnSub: true,
        mixVoice: false,
        voiceMix: 1,
        script: segsToScript(segs),
      }, (m: ProgressMsg) => {
        setExportMsg(m.message || '');
        if (m.status === 'done') {
          setExportBusy(false);
          const out = (m.payload as any)?.outPath || '';
          actions.task('剪映草稿已导出：' + out, 100);
          setExportMsg('导出完成：' + out);
        } else if (m.status === 'failed') {
          setExportBusy(false);
          actions.task('导出失败：' + (m.message || ''), 100);
          setExportMsg('导出失败');
        }
      });
    } catch (e) {
      setExportBusy(false);
      actions.task('导出失败：' + String(e), 100);
      setExportMsg('导出失败');
    }
  };
  const onTool = (id: string) => {
    switch (id) {
      case 'export_srt':
        generateSrt();
        break;
      case 'regen':
        onSwitchToNarration();
        break;
      case 'copy_sub': {
        const txt = segs.map((s) => s.text).join('\n');
        if (navigator.clipboard?.writeText) {
          navigator.clipboard.writeText(txt).then(
            () => actions.task('字幕已复制到剪贴板', 100),
            () => actions.task('复制失败', 100),
          );
        } else {
          actions.task('当前环境不支持剪贴板', 100);
        }
        break;
      }
      default:
        actions.task(`${TOOLBAR.find((t) => t.id === id)?.label || id}（M5 实现）`, 100);
    }
  };

  return (
    <div className="film-step6">
      {/* 顶部切换 */}
      <div className="film-step5__tabs">
        <button className="film-step5__tab" onClick={onSwitchToNarration}>📝 解说工作台</button>
        <button className="film-step5__tab active">🎬 分镜工作台</button>
      </div>

      {/* 工具栏 */}
      <div className="film-step6__toolbar">
        {TOOLBAR.map((t) => (
          <button
            key={t.id}
            className={'film-step6__tool' + (t.primary ? ' film-step6__tool--primary' : '')}
            onClick={() => onTool(t.id)}
            title={t.label}
          >
            <span className="film-step6__tool-icon">{t.icon}</span>
            <span className="film-step6__tool-label">{t.label}</span>
          </button>
        ))}
      </div>

      {/* 进度 / 导出提示 */}
      <div className="film-step6__progress">
        <span className="muted">
          💡 解说生成完成！可单击解说词编辑、调整顺序或删除；点击『导出 SRT』下载字幕，或『导出剪映草稿』生成工程文件。
        </span>
      </div>

      <div className="film-step6__main">
        {/* 左侧：视频预览 */}
        <div className="film-step6__left">
          <div className="film-step6__video">
            <div className="film-step6__video-frame">▶ 视频预览</div>
            <div className="film-step6__timeline">
              <div className="film-step6__timeline-handle" />
            </div>
            <div className="film-step6__video-meta">
              <div>视频文件：<span className="muted">{videoName || '-'}</span></div>
              <div>分镜数：<span className="muted">{segs.length}</span></div>
              <div>成片约：<span className="muted">{fmtDur(rangeEnd - rangeStart)}</span></div>
              <div>状态：<span className="muted">就位</span></div>
            </div>
          </div>
        </div>

        {/* 右侧：剪辑脚本表 */}
        <div className="film-step6__right">
          <div className="film-step6__script-header">
            <span>剪辑脚本</span>
            <span className="muted">共 <b style={{ color: 'var(--accent)' }}>{segs.length}</b> 条 ✓ {segs.length} | ⚠ 0 | 原片 0 | 成片约 {fmtDur(rangeEnd - rangeStart)}</span>
          </div>
          <div className="film-step6__table-wrap">
            <table className="film-step6__table">
              <thead>
                <tr>
                  <th style={{ width: 36 }}>#</th>
                  <th style={{ width: 72 }}>开始时间</th>
                  <th style={{ width: 72 }}>结束时间</th>
                  <th style={{ width: 56 }}>时长</th>
                  <th style={{ width: 70 }}>角色</th>
                  <th style={{ width: 110 }}>原始字幕</th>
                  <th>解说词（单击编辑）</th>
                  <th style={{ width: 70 }}>配音</th>
                  <th style={{ width: 90 }}>操作</th>
                </tr>
              </thead>
              <tbody>
                {segs.length === 0 && (
                  <tr><td colSpan={9} className="muted" style={{ textAlign: 'center', padding: 20 }}>暂无分镜数据。请先在解说工作台生成解说。</td></tr>
                )}
                {segs.slice(0, 15).map((s) => (
                  <tr key={s.index}>
                    <td>{s.index + 1}</td>
                    <td className="film-step6__time">{fmt(s.start)}</td>
                    <td className="film-step6__time">{fmt(s.end)}</td>
                    <td className="film-step6__time">{fmtDur(s.end - s.start)}</td>
                    <td>{s.role}</td>
                    <td className="muted" style={{ fontSize: 11 }}>—</td>
                    <td>
                      {editingIdx === s.index ? (
                        <input
                          className="mini"
                          autoFocus
                          value={editingText}
                          onChange={(e) => setEditingText(e.target.value)}
                          onBlur={commitEdit}
                          onKeyDown={(e) => { if (e.key === 'Enter') commitEdit(); }}
                        />
                      ) : (
                        <span onClick={() => handleEdit(s.index, s.text)} style={{ cursor: 'text' }}>
                          {s.text}
                        </span>
                      )}
                    </td>
                    <td>
                      <button className="film-step6__mini-btn" onClick={() => actions.task('单条配音（M5）', 100)}>单独配音</button>
                    </td>
                    <td>
                      <button className="film-step6__icon-btn" title="复制" onClick={() => actions.task('复制分镜（M5）', 100)}>📋</button>
                      <button className="film-step6__icon-btn" title="上移" onClick={() => moveSegment(s.index, -1)}>↑</button>
                      <button className="film-step6__icon-btn" title="下移" onClick={() => moveSegment(s.index, 1)}>↓</button>
                      <button className="film-step6__icon-btn" title="删除" onClick={() => removeSegment(s.index)}>🗑</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* 底部 4 选项 + 3 导出按钮 */}
      <div className="film-step6__bottom">
        <div className="film-step6__bottom-opts">
          <span className="muted">比例：</span>
          <select className="form-select" style={{ maxWidth: 120 }}><option>原比例</option><option>16:9</option><option>9:16</option></select>
          <span className="muted" style={{ marginLeft: 12 }}>字幕：</span>
          <select className="form-select" style={{ maxWidth: 140 }} value={subtitleStyle} onChange={(e) => setSubtitleStyle(e.target.value)}>
            <option>经典-白字黑边</option>
            <option>简约-无边框</option>
            <option>阴影-黑字阴影</option>
          </select>
          <label className="film-step6__check"><input type="checkbox" onChange={() => actions.task('遵循原字幕（M5）', 100)} /> 遵循原字幕（仅支持剪映6.0以下版本）</label>
          <label className="film-step6__check"><input type="checkbox" onChange={() => actions.task('花字（M5）', 100)} /> 花字</label>
          <label className="film-step6__check"><input type="checkbox" onChange={() => actions.task('音频强制对齐（M5）', 100)} /> 音频强制对齐</label>
        </div>
        <div className="film-step6__bottom-exports">
          <button className="btn primary film-step6__export-btn" onClick={exportJianying} disabled={exportBusy}>
            {exportBusy ? (exportMsg || '导出中...') : '📤 导出剪映草稿'}
          </button>
          <button className="btn sm" onClick={() => actions.task('导出 Premiere（M5 远期）', 100)}>📤 导出 Premiere</button>
          <button className="btn sm" onClick={() => actions.task('导出国际剪映（M5 远期）', 100)}>📤 导出国际剪映</button>
          <button className="btn sm ghost" onClick={generateSrt}>💾 导出 SRT</button>
        </div>
      </div>

      <div style={{ marginTop: 12 }}>
        <button className="btn sm ghost" onClick={onBack}>‹ 返回解说工作台</button>
      </div>
    </div>
  );
}
