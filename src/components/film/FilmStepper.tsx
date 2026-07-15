// 影视模块 · 常驻 5 阶段流程指示器
// 1.开始 → 2.视频类型 → 3.视频选择 → 4.解说工作台 → 5.分镜工作台
// 与内部 6 步向导（Splash/PickStyle/UploadVideo/VideoRange/Narration/DubAndCut）映射

import { PlayCircle, Clapperboard, Film, Mic, LayoutGrid } from 'lucide-react';

export interface FilmStage {
  n: number;
  label: string;
  icon: typeof PlayCircle;
}

export const FILM_STAGES: FilmStage[] = [
  { n: 1, label: '开始', icon: PlayCircle },
  { n: 2, label: '视频类型', icon: Clapperboard },
  { n: 3, label: '视频选择', icon: Film },
  { n: 4, label: '解说工作台', icon: Mic },
  { n: 5, label: '分镜工作台', icon: LayoutGrid },
];

export default function FilmStepper({ stage }: { stage: number }) {
  return (
    <div className="film-stepper" role="list" aria-label="影视创作流程">
      {FILM_STAGES.map((s, i) => {
        const state = i < stage ? 'done' : i === stage ? 'active' : 'todo';
        const Icon = s.icon;
        return (
          <div className={`film-stepper__item ${state}`} role="listitem" key={s.n}>
            <div className="film-stepper__node">
              <span className="film-stepper__num">{i < stage ? '✓' : s.n}</span>
            </div>
            <div className="film-stepper__label">
              <Icon size={14} className="film-stepper__ic" aria-hidden />
              <span>{s.label}</span>
            </div>
            {i < FILM_STAGES.length - 1 && <div className="film-stepper__line" aria-hidden />}
          </div>
        );
      })}
    </div>
  );
}
