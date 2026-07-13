// 导出弹窗（参考解说猫：完整项目 / 剪映 / Premiere / SRT 4 格式）
// M2 已有剪映草稿导出（draft_content.json），本弹窗作为更完整的 UI 入口

import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { useApp } from '../state/AppContext';

interface Props {
  open: boolean;
  onClose: () => void;
  onConfirm: (format: string, outDir: string) => void;
}

const FORMATS = [
  { id: 'full',     icon: '📹', name: '完整项目', desc: '配音 + 字幕 + 视频合并（mp4）' },
  { id: 'jianying', icon: '🎬', name: '剪映草稿', desc: '导入剪映 Pro 继续编辑（draft_content.json）' },
  { id: 'pr',       icon: '🎞️', name: 'Premiere', desc: '导出 PR XML 项目文件' },
  { id: 'srt',      icon: '📝', name: 'SRT 字幕', desc: '仅导出字幕文件' },
];

export default function ExportModal({ open: isOpen, onClose, onConfirm }: Props) {
  const { state, actions } = useApp();
  const [selected, setSelected] = useState('full');
  const [outDir, setOutDir] = useState('D:\\VideosFlow\\output\\project_001');
  const [quality, setQuality] = useState('原画 (1080p)');

  if (!isOpen) return null;

  const pickDir = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: '选择输出目录' });
      if (selected) setOutDir(String(selected));
    } catch {
      // 浏览器模式兜底
    }
  };

  return (
    <div className="narrative-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="export-modal">
        <div className="narrative-modal__header">
          <h3>📤 导出项目</h3>
          <button className="narrative-modal__close" onClick={onClose}>×</button>
        </div>
        <div className="narrative-modal__body">
          <div className="form-row">
            <label className="form-label">选择导出格式</label>
            <div className="export-grid">
              {FORMATS.map((f) => (
                <div
                  key={f.id}
                  className={'export-card' + (selected === f.id ? ' active' : '')}
                  onClick={() => setSelected(f.id)}
                >
                  <div className="export-card__icon">{f.icon}</div>
                  <div className="export-card__name">{f.name}</div>
                  <div className="export-card__desc">{f.desc}</div>
                </div>
              ))}
            </div>
          </div>
          <div className="form-row">
            <label className="form-label">输出路径</label>
            <div style={{ display: 'flex', gap: 8 }}>
              <input className="form-input" value={outDir} onChange={(e) => setOutDir(e.target.value)} />
              <button className="btn" onClick={pickDir}>📂 浏览</button>
            </div>
          </div>
          <div className="form-row">
            <label className="form-label">视频质量</label>
            <select className="form-select" value={quality} onChange={(e) => setQuality(e.target.value)}>
              <option>原画 (1080p)</option>
              <option>高清 (720p)</option>
              <option>标准 (480p)</option>
            </select>
          </div>
          {state.editingProj && (
            <div className="export-summary">
              <div className="export-summary__row"><span>📹 当前工程</span><b>{state.editingProj.t}</b></div>
              <div className="export-summary__row"><span>📊 时长</span><b>{Math.round((state.editorState.script?.length || 0) / 270)} 分钟</b></div>
            </div>
          )}
        </div>
        <div className="narrative-modal__footer">
          <button className="btn" onClick={onClose}>取消</button>
          <button
            className="btn primary"
            onClick={() => {
              actions.task(`已选择导出：${FORMATS.find(f => f.id === selected)?.name}`, 100);
              onConfirm(selected, outDir);
            }}
          >
            📤 开始导出
          </button>
        </div>
      </div>
    </div>
  );
}
