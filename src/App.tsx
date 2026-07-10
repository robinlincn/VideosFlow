import { useApp } from './state/AppContext';
import { FilmIcon, MicIcon, SparkIcon, GearIcon } from './components/icons';
import Film from './modules/Film';
import Spoken from './modules/Spoken';
import Creation from './modules/Creation';
import Settings from './modules/Settings';
import { filmSteps, spokenSteps, cSteps, settingsSteps, ModuleKey } from './data/mock';

const MODULES = [
  { key: 'film', name: '影片', Icon: FilmIcon },
  { key: 'spoken', name: '口播', Icon: MicIcon },
  { key: 'creation', name: '创作视频', Icon: SparkIcon },
  { key: 'settings', name: '设置', Icon: GearIcon },
] as const;

const TITLE: Record<ModuleKey, string> = {
  film: '影片', spoken: '口播', creation: '创作视频', settings: '系统配置',
};

function StepPills({ steps, current, onPick }: { steps: { id: string; name: string }[]; current: string; onPick: (id: string) => void; }) {
  return (
    <div className="steps">
      {steps.map((s) => {
        const idx = steps.findIndex((x) => x.id === s.id);
        const cur = steps.findIndex((x) => x.id === current);
        const cls = s.id === current ? 'active' : idx < cur ? 'done' : '';
        return <div key={s.id} className={'step ' + cls} onClick={() => onPick(s.id)}>{s.name}</div>;
      })}
    </div>
  );
}

function Inspector() {
  const { state, actions } = useApp();
  if (state.module === 'film' && state.filmStage === 'editor') {
    return (
      <>
        <div className="insp-title">剪辑台进度</div>
        <div className="insp-card">
          {filmSteps.map((s) => {
            const idx = filmSteps.findIndex((x) => x.id === s.id);
            const cur = filmSteps.findIndex((x) => x.id === state.editorSub);
            return (
              <div key={s.id} className="ol" style={{ marginBottom: 2 }}>
                <div className={'it ' + (s.id === state.editorSub ? 'active' : idx < cur ? 'done' : '')}>
                  <span className="n">{idx + 1}</span>{s.name}{idx < cur ? ' ✓' : ''}
                </div>
              </div>
            );
          })}
        </div>
        <div className="insp-card muted sm">当前工程：{state.editingProj?.t || '—'}<br />视频：{state.editorState.videoName}</div>
      </>
    );
  }
  if (state.module === 'settings') {
    const p = state.settingsState.providers;
    const enabled = Object.values(p).filter((x) => x.enabled).length;
    const total = Object.keys(p).length;
    return (
      <>
        <div className="insp-title">配置概览</div>
        <div className="insp-card">已启用模型 <b>{enabled}/{total}</b></div>
        <div className="insp-card">提示词模板 <b>{Object.keys(state.settingsState.prompts).length}</b> 个</div>
        <div className="insp-card muted sm">硬件加速：{state.settingsState.other.hwAccel ? '开' : '关'}<br />默认导出：{state.settingsState.other.exportResolution} · {state.settingsState.other.exportFormat}</div>
      </>
    );
  }
  return (
    <>
      <div className="insp-title">提示</div>
      <div className="insp-card muted sm">在左侧选择工作区。影片用于成片剪辑，口播用于净化，创作视频用于从需求生成完整视频。</div>
    </>
  );
}

export default function App() {
  const { state, actions } = useApp();
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand"><div className="logo">▶</div><div className="name">VideosFlow</div></div>
        <div className="side-label">工作区</div>
        {MODULES.map((m) => (
          <button key={m.key} className={'side-item ' + (state.module === m.key ? 'active' : '')} onClick={() => actions.goModule(m.key as ModuleKey)}>
            <span className="ico"><m.Icon /></span>{m.name}
          </button>
        ))}
        <div className="side-spacer" />
        <div className="side-label">v0.1.0 · 工程骨架</div>
      </aside>

      <main className="main">
        <div className="topbar">
          <div className="traffic"><i className="r" /><i className="y" /><i className="g" /></div>
          <div className="title">{TITLE[state.module]}</div>
          <div className="top-actions">
            <button className="btn sm ghost" onClick={() => { actions.goModule('creation'); actions.goExport(); }}>⬇ 导出</button>
          </div>
        </div>
        <div className="content">
          <div className="content-inner">
            {state.module === 'film' && <Film />}
            {state.module === 'spoken' && <Spoken />}
            {state.module === 'creation' && <Creation />}
            {state.module === 'settings' && <Settings />}
          </div>
        </div>
      </main>

      <aside className="inspector"><Inspector /></aside>

      <div className="taskbar">
        <span>{state.task.label}</span>
        <span className="prog"><i style={{ width: state.task.p + '%' }} /></span>
      </div>
    </div>
  );
}
