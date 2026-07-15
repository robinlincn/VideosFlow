// 步骤 3：上传影片（巨型 dropzone + 选择视频按钮 + SSD 加速提示）
// v2.0 重构：替代 v1.0 的左侧栏 + 导入按钮
// 双模式选择本地视频文件：
//   - Tauri 桌面版：原生文件对话框（返回真实磁盘路径）
//   - 普通浏览器：<input type="file"> + 拖拽，读取真实时长
// 选完后由父级 onPicked → 弹出"设置视频范围"弹框

import { useRef, useState } from 'react';

const VIDEO_EXTS = ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'];

const hasTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

interface Props {
  styleId: string;
  styleName: string;
  onPicked: (videoPath: string, videoName: string, videoDuration: number) => void;
  onBack: () => void;
}

export default function Step3UploadVideo({ styleId, styleName, onPicked, onBack }: Props) {
  const [videoName, setVideoName] = useState<string>('');
  const [, setVideoPath] = useState<string>('');
  const [dragOver, setDragOver] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // 从本地文件读取真实时长（浏览器）
  const readDuration = (file: File): Promise<number> =>
    new Promise((resolve) => {
      try {
        const url = URL.createObjectURL(file);
        const v = document.createElement('video');
        v.preload = 'metadata';
        v.onloadedmetadata = () => {
          URL.revokeObjectURL(url);
          const d = isFinite(v.duration) && v.duration > 0 ? Math.round(v.duration) : 180;
          resolve(d);
        };
        v.onerror = () => {
          URL.revokeObjectURL(url);
          resolve(180);
        };
        v.src = url;
      } catch {
        resolve(180);
      }
    });

  const isVideoFile = (name: string) =>
    VIDEO_EXTS.some((ext) => name.toLowerCase().endsWith('.' + ext));

  // 浏览器：处理选中的 File 对象
  const handleBrowserFile = async (file: File) => {
    if (!isVideoFile(file.name)) {
      alert('请选择视频文件（MP4 / MOV / MKV / AVI / WEBM / M4V）');
      return;
    }
    const duration = await readDuration(file);
    setVideoPath(file.name);
    setVideoName(file.name);
    onPicked(file.name, file.name, duration);
  };

  // Tauri 桌面：原生文件对话框
  const pickViaTauri = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: '视频文件', extensions: VIDEO_EXTS }],
        title: '选择要解说的视频',
      });
      if (!selected) return;
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      const fileName = String(filePath).replace(/.*[\\/]/, '');
      setVideoPath(String(filePath));
      setVideoName(fileName);
      // 桌面版真实时长由后端 ffprobe 获取，这里给默认 180s
      onPicked(String(filePath), fileName, 180);
    } catch (e) {
      console.error('[film-step3] Tauri 文件选择失败:', e);
    }
  };

  // 统一入口：点击选择视频
  const pickVideo = () => {
    if (hasTauri) {
      pickViaTauri();
    } else {
      fileInputRef.current?.click();
    }
  };

  const onInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) handleBrowserFile(file);
    // 允许重复选择同一文件
    e.target.value = '';
  };

  // 拖拽（浏览器模式支持；Tauri 走原生拖放另议）
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    if (hasTauri) return;
    const file = e.dataTransfer.files?.[0];
    if (file) handleBrowserFile(file);
  };

  return (
    <div className="film-step3">
      <div className="film-step2__top">
        <button className="film-step2__back" onClick={onBack}>‹ 返回</button>
      </div>
      <div className="film-step3__header">
        <h1>🎬 {styleName}</h1>
        <div className="film-step2__subtitle">选择本地视频文件开始创作</div>
      </div>

      {/* 隐藏的本地文件选择器（浏览器模式） */}
      <input
        ref={fileInputRef}
        type="file"
        accept="video/*,.mp4,.mov,.mkv,.avi,.webm,.m4v"
        style={{ display: 'none' }}
        onChange={onInputChange}
      />

      <div
        className={'film-step3__card' + (dragOver ? ' film-step3__card--dragover' : '')}
        onClick={pickVideo}
        onDragOver={(e) => { e.preventDefault(); if (!hasTauri) setDragOver(true); }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
      >
        <div className="film-step3__icon">🎬</div>
        <div className="film-step3__title">{videoName || '点击或拖拽视频到此处'}</div>
        <div className="film-step3__desc">支持 MP4、MOV、AVI、MKV、WEBM、M4V 格式</div>
        <button className="btn primary film-step3__btn" onClick={(e) => { e.stopPropagation(); pickVideo(); }}>
          选择视频
        </button>
      </div>
      <div className="film-step3__tip">
        💡 将视频放在固态硬盘（SSD）可提速 3~5 倍，用时从 5 分钟缩短到 1 分钟内
      </div>
    </div>
  );
}
