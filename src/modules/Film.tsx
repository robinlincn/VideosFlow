// 影片模块（v2.0 重构版 · 2026-07-13）
// 按 docs/film-module-v2-design.md 文档：
// 6 步线性流程：开始 → 选择类型 → 上传影片 → 设置视频范围 → 视频解说功能 → 视频配音剪辑
// 顶部常驻 5 阶段流程指示器（开始 / 视频类型 / 视频选择 / 解说工作台 / 分镜工作台）
// 顶部切换"解说工作台 ↔ 分镜工作台"（步骤 5 / 6 共享）

import { useState, type ReactNode } from 'react';
import { useApp } from '../state/AppContext';
import { createFilmProject } from '../ipc/providers';
import Step1Splash from '../components/film/Step1Splash';
import Step2PickStyle from '../components/film/Step2PickStyle';
import Step3UploadVideo from '../components/film/Step3UploadVideo';
import Step4VideoRange from '../components/film/Step4VideoRange';
import Step5Narration from '../components/film/Step5Narration';
import Step6DubAndCut from '../components/film/Step6DubAndCut';
import FilmStepper from '../components/film/FilmStepper';

type Step = 1 | 2 | 3 | 4 | 5 | 6;

// 内部 6 步 → 对外显示的 5 阶段（视频范围归属"视频选择"）
const STAGE_OF_STEP: Record<Step, number> = {
  1: 0, // 开始
  2: 1, // 视频类型
  3: 2, // 视频选择
  4: 2, // 视频选择（范围模态）
  5: 3, // 解说工作台
  6: 4, // 分镜工作台
};

interface DraftCtx {
  styleId: string;
  styleName: string;
  videoPath: string;
  videoName: string;
  videoDuration: number; // 秒
  rangeStart: number;    // 秒
  rangeEnd: number;      // 秒
  script: string;
  projectId: string;
  categoryId: string;
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
  projectId: '',
  categoryId: '',
};

// Step2 解说风格 → 影片分类（与前后端 seed c1..c5 对齐：电影/故事/电视剧/动画片/记录片）
const STYLE_TO_CAT: Record<string, string> = {
  movie: 'c1', series: 'c3', variety: 'c3', anime: 'c4',
  shortdrama: 'c2', sports: 'c5', documentary: 'c5', general: 'c1',
};

export default function Film() {
  const { actions } = useApp();
  const [step, setStep] = useState<Step>(1);
  const [draft, setDraft] = useState<DraftCtx>(EMPTY_DRAFT);
  const [showRangeModal, setShowRangeModal] = useState(false);

  // 5 → 6 切换
  const [tab, setTab] = useState<'narration' | 'storyboard'>('narration');

  const stage = STAGE_OF_STEP[step];

  const goStart = () => { setStep(1); setDraft(EMPTY_DRAFT); setTab('narration'); };
  const goStep2 = () => setStep(2);
  const goStep3 = (styleId: string, styleName: string) => {
    setDraft((d) => ({ ...d, styleId, styleName }));
    setStep(3);
  };
  const goStep4 = async (videoPath: string, videoName: string, videoDuration: number) => {
    let projectId = draft.projectId;
    let categoryId = draft.categoryId;
    if (!projectId) {
      try {
        categoryId = STYLE_TO_CAT[draft.styleId] || 'c1';
        projectId = await createFilmProject(categoryId, videoName);
      } catch {
        // 后端不可用时退回本地 id，保证流程可继续（主要用于纯浏览器预览）
        projectId = 'local-' + Date.now().toString(36);
        categoryId = STYLE_TO_CAT[draft.styleId] || 'c1';
      }
    }
    setDraft((d) => ({ ...d, videoPath, videoName, videoDuration, projectId, categoryId, rangeStart: 0, rangeEnd: videoDuration }));
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

  let body: ReactNode = null;

  // 步骤 1
  if (step === 1) {
    body = <Step1Splash onStart={goStep2} />;
  }
  // 步骤 2
  else if (step === 2) {
    body = <Step2PickStyle onPick={goStep3} onBack={backFromStep2} />;
  }
  // 步骤 3
  else if (step === 3) {
    body = (
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
          videoPath={draft.videoPath}
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
  else if (step === 5) {
    if (tab === 'narration') {
      body = (
        <Step5Narration
          videoPath={draft.videoPath}
          videoName={draft.videoName}
          videoDuration={draft.videoDuration}
          rangeStart={draft.rangeStart}
          rangeEnd={draft.rangeEnd}
          styleId={draft.styleId}
          styleName={draft.styleName}
          projectId={draft.projectId}
          onGenerated={goStep6}
          onBack={backFromStep5}
          onGotoStoryboard={() => { setTab('storyboard'); setStep(6); }}
        />
      );
    } else {
      // 分镜工作台（步骤 6）
      body = (
        <Step6DubAndCut
          script={draft.script}
          videoName={draft.videoName}
          projectId={draft.projectId}
          totalDuration={draft.rangeEnd - draft.rangeStart}
          rangeStart={draft.rangeStart}
          rangeEnd={draft.rangeEnd}
          onBack={backFromStep6}
          onSwitchToNarration={() => setTab('narration')}
          onScriptChange={(s) => setDraft((d) => ({ ...d, script: s }))}
        />
      );
    }
  }
  // 步骤 6
  else if (step === 6) {
      body = (
        <Step6DubAndCut
          script={draft.script}
          videoName={draft.videoName}
          projectId={draft.projectId}
          totalDuration={draft.rangeEnd - draft.rangeStart}
          rangeStart={draft.rangeStart}
          rangeEnd={draft.rangeEnd}
          onBack={() => { setTab('narration'); setStep(5); }}
          onSwitchToNarration={() => { setTab('narration'); setStep(5); }}
          onScriptChange={(s) => setDraft((d) => ({ ...d, script: s }))}
        />
      );
  }

  return (
    <div className="film-flow">
      <FilmStepper stage={stage} />
      <div className="film-flow__body">{body}</div>
    </div>
  );
}
