// 步骤 1：开始界面（紫渐变背景 + AI 影视解说生成器 hero + 开始创作按钮）
// v2.0 重构：替代 v1.0 的影片库入口

interface Props {
  onStart: () => void;
}

export default function Step1Splash({ onStart }: Props) {
  return (
    <div className="film-step1-splash">
      <div className="film-step1-splash__bg" />
      <div className="film-step1-splash__content">
        <div className="film-step1-splash__title">AI 影视解说生成器</div>
        <div className="film-step1-splash__subtitle">自动分析视频内容，一键生成解说词和配音</div>
        <button className="film-step1-splash__btn" onClick={onStart}>
          <span className="film-step1-splash__btn-icon">✨</span>
          <span>开始创作</span>
        </button>
      </div>
    </div>
  );
}
