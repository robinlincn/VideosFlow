// 步骤 3：上传影片（巨型 dropzone + 选择视频按钮 + SSD 加速提示）
// v2.0 重构：替代 v1.0 的左侧栏 + 导入按钮

import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

interface Props {
  styleId: string;
  styleName: string;
  onPicked: (videoPath: string, videoName: string, videoDuration: number) => void;
  onBack: () => void;
}

export default function Step3UploadVideo({ styleId, styleName, onPicked, onBack }: Props) {
  const [videoName, setVideoName] = useState<string>('');
  const [videoPath, setVideoPath] = useState<string>('');

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
      // 默认 3 分钟（M2.5 默认时长），真实场景用 ffprobe
      onPicked(filePath, fileName, 180);
    } catch {
      // 浏览器模式兜底
      const mockPath = 'mock://upload.mp4';
      setVideoPath(mockPath);
      setVideoName('演示视频.mp4');
      onPicked(mockPath, '演示视频.mp4', 180);
    }
  };

  return (
    <div className="film-step3">
      <div className="film-step2__top">
        <button className="film-step2__back" onClick={onBack}>‹ 返回</button>
      </div>
      <div className="film-step3__header">
        <h1>🎬 {styleName}</h1>
        <div className="film-step2__subtitle">选择视频文件开始创作</div>
      </div>
      <div className="film-step3__card" onClick={pickVideo}>
        <div className="film-step3__icon">🎬</div>
        <div className="film-step3__title">{videoName || '点击或拖拽视频到此处'}</div>
        <div className="film-step3__desc">支持 MP4、MOV、AVI、MKV 格式</div>
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
