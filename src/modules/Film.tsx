// 影片模块（v2.0 重构版 · 2026-07-13）
// 按 docs/film-module-v2-design.md 文档：
// 6 步线性流程：开始 → 选择类型 → 上传影片 → 设置视频范围 → 视频解说功能 → 视频配音剪辑
// 顶部切换"解说工作台 ↔ 分镜工作台"（步骤 5 / 6 共享）
// 风格保留原 v1.0 设计语言（不修改设计风格），仅替换结构

import { useState } from 'react';
import { useApp } from '../state/AppContext';
import Step1Splash from '../components/film/Step1Splash';
import Step2PickStyle from '../components/film/Step2PickStyle';
import Step3UploadVideo from '../components/film/Step3UploadVideo';
import Step4VideoRange from '../components/film/Step4VideoRange';
import Step5Narration from '../components/film/Step5Narration';
import Step6DubAndCut from '../components/film/Step6DubAndCut';

type Step = 1 | 2 | 3 | 4 | 5 | 6;

interface DraftCtx {
  styleId: string;
  styleName: string;
  videoPath: string;
  videoName: string;
  videoDuration: number; // 秒
  rangeStart: number;    // 秒
  rangeEnd: number;      // 秒
  script: string;
}

const EMPTY_DRAFT: DraftCtx = {
  styleId: '',
  styleName: '',
  videoPath: '',
  videoName: '',
  videoDuration: 180,
  rangeStart: 0,
  rangeEnd: 180,
  script: '',
};

export default function Film() {
  const { actions } = useApp();
  const [step, setStep] = useState<Step>(1);
  const [draft, setDraft] = useState<DraftCtx>(EMPTY_DRAFT);
  const [showRangeModal, setShowRangeModal] = useState(false);

  // 5 → 6 切换
  const [tab, setTab] = useState<'narration' | 'storyboard'>('narration');

  const goStart = () => { setStep(1); setDraft(EMPTY_DRAFT); setTab('narration'); };
  const goStep2 = () => setStep(2);
  const goStep3 = (styleId: string, styleName: string) => {
    setDraft((d) => ({ ...d, styleId, styleName }));
    setStep(3);
  };
  const goStep4 = (videoPath: string, videoName: string, videoDuration: number) => {
    setDraft((d) => ({ ...d, videoPath, videoName, videoDuration, rangeStart: 0, rangeEnd: videoDuration }));
    setShowRangeModal(true);
  };
  const goStep5 = (start: number, end: number) => {
    setDraft((d) => ({ ...d, rangeStart: start, rangeEnd: end }));
    setShowRangeModal(false);
    setStep(5);
  };
  const goStep6 = (script: string) => {
    setDraft((d) => ({ ...d, script }));
    setTab('storyboard');
    setStep(6);
  };
  const backFromStep5 = () => setStep(3);
  const backFromStep6 = () => { setTab('narration'); setStep(5); };
  const backFromStep4 = () => setStep(2);
  const backFromStep3 = () => setStep(1);
  const backFromStep2 = () => setStep(1);

  // 步骤 1
  if (step === 1) {
    return <Step1Splash onStart={goStep2} />;
  }
  // 步骤 2
  if (step === 2) {
    return <Step2PickStyle onPick={goStep3} onBack={backFromStep2} />;
  }
  // 步骤 3
  if (step === 3) {
    return (
      <>
        <Step3UploadVideo
          styleId={draft.styleId}
          styleName={draft.styleName}
          onPicked={goStep4}
          onBack={backFromStep3}
        />
        {/* 步骤 4 模态 */}
        <Step4VideoRange
          open={showRangeModal}
          totalDuration={draft.videoDuration || 180}
          initialStart={draft.rangeStart}
          initialEnd={draft.rangeEnd}
          onConfirm={goStep5}
          onCancel={() => setShowRangeModal(false)}
        />
      </>
    );
  }
  // 步骤 5
  if (step === 5) {
    if (tab === 'narration') {
      return (
        <Step5Narration
          videoPath={draft.videoPath}
          videoName={draft.videoName}
          videoDuration={draft.videoDuration}
          rangeStart={draft.rangeStart}
          rangeEnd={draft.rangeEnd}
          styleId={draft.styleId}
          styleName={draft.styleName}
          onGenerated={goStep6}
          onBack={backFromStep5}
        />
      );
    } else {
      // 分镜工作台（步骤 6）
      return (
        <Step6DubAndCut
          script={draft.script}
          videoPath={draft.videoPath}
          videoName={draft.videoName}
          totalDuration={draft.rangeEnd - draft.rangeStart}
          rangeStart={draft.rangeStart}
          rangeEnd={draft.rangeEnd}
          onBack={backFromStep6}
          onSwitchToNarration={() => setTab('narration')}
        />
      );
    }
  }
  // 步骤 6
  if (step === 6) {
    return (
      <Step6DubAndCut
        script={draft.script}
        videoPath={draft.videoPath}
        videoName={draft.videoName}
        totalDuration={draft.rangeEnd - draft.rangeStart}
        rangeStart={draft.rangeStart}
        rangeEnd={draft.rangeEnd}
        onBack={() => { setTab('narration'); setStep(5); }}
        onSwitchToNarration={() => { setTab('narration'); setStep(5); }}
      />
    );
  }
  return null;
}
