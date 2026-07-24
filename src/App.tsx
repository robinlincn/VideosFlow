import { useState, useEffect, useCallback } from 'react';
import { useApp } from './state/AppContext';
import {
  FilmIcon, MicIcon, SparkIcon, GearIcon,
  MoonIcon, SunIcon, InspectorIcon, InspectorOpenIcon,
  MenuIcon, CloseIcon, DownloadIcon,
} from './components/icons';
import Film from './modules/Film';
import Spoken from './modules/Spoken';
import Creation from './modules/Creation';
import Settings from './modules/Settings';
import GenProgress from './components/GenProgress';
import ErrorBoundary from './components/ErrorBoundary';
import { ModuleKey } from './data/mock';

const MODULES = [
  { key: 'film', name: '影片', en: 'Film', Icon: FilmIcon },
  { key: 'spoken', name: '口播', en: 'Spoken', Icon: MicIcon },
  { key: 'creation', name: '创作', en: 'Create', Icon: SparkIcon },
  { key: 'settings', name: '设置', en: 'Config', Icon: GearIcon },
] as const;

const TITLE: Record<ModuleKey, string> = {
  film: '影片工作台',
  spoken: '口播精修',
  creation: '创作视频',
  settings: '系统配置',
};

const VIEW_HEADER: Record<ModuleKey, { eyebrow: string; title: string; em: string; issue: string }> = {
  film: {
    eyebrow: 'I · FILM',
    title: '类型化工程',
    em: '剪辑工作流',
    issue: 'No. 01',
  },
  spoken: {
    eyebrow: 'II · SPOKEN',
    title: '口播净化与',
    em: '花字字幕',
    issue: 'No. 02',
  },
  creation: {
    eyebrow: 'III · CREATE',
    title: '从灵感到',
    em: '完整成片',
    issue: 'No. 03',
  },
  settings: {
    eyebrow: 'IV · CONFIG',
    title: '模型与',
    em: '系统参数',
    issue: 'No. 04',
  },
};

function Inspector() {
  const { state, actions } = useApp();
  const task = state.task;
  const taskNav = state.taskNav;
  const idle = task.label === '空闲' || task.p <= 0;
  const failed = /失败|错误|error/i.test(task.label);
  const done = !idle && !failed && task.p >= 100;
  const goTask = () => {
    if (!taskNav) return;
    const patch: any = { module: taskNav.module };
    if (taskNav.module === 'creation') {
      patch.cStage = taskNav.stage ?? state.cStage;
      if (taskNav.sel) patch.creationSel = taskNav.sel;
    } else if (taskNav.module === 'film') {
      patch.filmStage = 'editor';
      if (taskNav.stage) patch.editorSub = taskNav.stage;
      if (taskNav.sel) patch.editingProj = { cat: state.editingProj?.cat ?? 'c1', id: taskNav.sel, t: state.editingProj?.t ?? '' };
    } else if (taskNav.module === 'spoken') {
      if (taskNav.stage) patch.spokenStage = taskNav.stage;
      if (taskNav.sel) patch.spokenSel = taskNav.sel;
    }
    actions.set(patch);
  };
  const taskCard = (
    <>
      <div className="insp-title">任务进度</div>
      <div className={'insp-card task-prog ' + (idle ? 'idle' : done ? 'done' : failed ? 'fail' : 'run')}>
        <div className="task-prog__row">
          <span className="task-prog__label">{task.label}</span>
          {taskNav && !idle && <button className="task-prog__go" onClick={goTask}>查看 →</button>}
        </div>
        {!idle && (
          <div className="task-prog__bar"><i style={{ width: Math.min(100, Math.max(0, task.p)) + '%' }} /></div>
        )}
        <div className="task-prog__meta">
          {idle ? '当前无进行中的任务' : (done ? '已完成' : failed ? '失败' : `${Math.round(task.p)}%`)}
        </div>
      </div>
    </>
  );
  if (state.module === 'film' && state.filmStage === 'editor') {
    return (
      <>
        {taskCard}
        <div className="insp-title">剪辑台进度</div>
        <div className="insp-card">
          <div className="ol">
            {state.editingProj && (
              <>
                {[
                  { id: 'gen', name: '生成解说文案' },
                  { id: 'align', name: '导入对齐' },
                  { id: 'voice', name: '解说配音' },
                  { id: 'cut', name: '自动切点' },
                  { id: 'time', name: '时间线精修' },
                  { id: 'out', name: '字幕花字导出' },
                ].map((s, idx, arr) => {
                  const cur = arr.findIndex((x) => x.id === state.editorSub);
                  return (
                    <div key={s.id} className={'it ' + (s.id === state.editorSub ? 'active' : idx < cur ? 'done' : '')}>
                      <span className="n">{String(idx + 1).padStart(2, '0')}</span>{s.name}
                    </div>
                  );
                })}
              </>
            )}
          </div>
        </div>
        <div className="insp-card muted sm">
          当前工程<br />
          <strong style={{ fontFamily: 'var(--font-display)', fontSize: 16 }}>{state.editingProj?.t || '—'}</strong>
          <br /><br />
          视频 · {state.editorState.videoName}
        </div>
      </>
    );
  }
  if (state.module === 'settings') {
    const p = state.settingsState.providers;
    const enabled = Object.values(p).filter((x) => x.enabled).length;
    const total = Object.keys(p).length;
    return (
      <>
        {taskCard}
        <div className="insp-title">配置概览</div>
        <div className="insp-card">已启用模型 <strong>{enabled}</strong> / {total}</div>
        <div className="insp-card">提示词模板 <strong>{Object.keys(state.settingsState.prompts).length}</strong> 个</div>
        <div className="insp-card muted sm">
          硬件加速 · {state.settingsState.other.hwAccel ? '开' : '关'}<br />
          默认导出 · {state.settingsState.other.exportResolution}<br />
          {state.settingsState.other.exportFormat}
        </div>
      </>
    );
  }
  return (
    <>
      {taskCard}
      <div className="insp-title">提示</div>
      <div className="insp-card muted sm">在左侧选择工作区。影片用于成片剪辑，口播用于净化，创作视频用于从需求生成完整视频。</div>
    </>
  );
}

function ViewHeader({ module }: { module: ModuleKey }) {
  const header = VIEW_HEADER[module];
  return (
    <div className="view-header">
      <div>
        <div className="number">{header.eyebrow}</div>
        <h1>{header.title} <em>{header.em}</em></h1>
      </div>
      <div className="meta">
        {header.issue}<br />
        <strong>{TITLE[module]}</strong><br />
        {new Date().getFullYear()}
      </div>
    </div>
  );
}

export default function App() {
  const { state, actions } = useApp();
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);

  useEffect(() => {
    const saved = localStorage.getItem('vf-theme') as 'light' | 'dark' | null;
    if (saved) {
      setTheme(saved);
    } else {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      setTheme(prefersDark ? 'dark' : 'light');
    }
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('vf-theme', theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((t) => t === 'light' ? 'dark' : 'light');
  }, []);

  const goModule = (m: ModuleKey) => {
    actions.goModule(m);
    setSidebarOpen(false);
  };

  return (
    <div className="app">
      {/* Mobile sidebar toggle */}
      <button className="sidebar-toggle" onClick={() => setSidebarOpen(!sidebarOpen)}>
        {sidebarOpen ? <CloseIcon /> : <MenuIcon />}
      </button>

      {/* Sidebar overlay (tablet) */}
      {sidebarOpen && (
        <div className="sidebar-overlay" onClick={() => setSidebarOpen(false)} />
      )}

      {/* Sidebar */}
      <aside className={'sidebar' + (sidebarOpen ? ' open' : '')}>
        <div className="brand">
          <span className="logo">V</span>
          <span className="name">VideosFlow</span>
        </div>
        <div className="side-label">Modules</div>
        {MODULES.map((m, idx) => (
          <button
            key={m.key}
            className={'side-item ' + (state.module === m.key ? 'active' : '')}
            onClick={() => goModule(m.key as ModuleKey)}
          >
            <span className="num">{String(idx + 1).padStart(2, '0')}</span>
            <span className="label">
              <span style={{ display: 'block', fontWeight: 600 }}>{m.name}</span>
              <span style={{ fontSize: 10, color: 'var(--ink-3)', fontFamily: 'var(--font-mono)', letterSpacing: 1, textTransform: 'uppercase' }}>{m.en}</span>
            </span>
            <span className="ico"><m.Icon /></span>
          </button>
        ))}
        <div className="side-spacer" />
        <div className="sidebar-footer">
          vol. 0.1 · {new Date().getFullYear()}
        </div>
      </aside>

      {/* Main */}
      <main className="main">
        <div className="topbar">
          <div className="crumb">
            <span>VideosFlow</span>
            <span className="sep" />
            <span className="current">{TITLE[state.module]}</span>
          </div>
          <div className="top-actions">
            <button
              className={'inspector-toggle' + (inspectorOpen ? ' active' : '')}
              onClick={() => setInspectorOpen(!inspectorOpen)}
              title="切换侧栏面板"
            >
              {inspectorOpen ? <InspectorOpenIcon /> : <InspectorIcon />}
            </button>
            <button
              className="theme-toggle"
              onClick={toggleTheme}
              title={theme === 'light' ? '切换到深色' : '切换到浅色'}
            >
              {theme === 'light' ? <MoonIcon /> : <SunIcon />}
            </button>
            <button
              className="btn sm ghost"
              onClick={() => { actions.goModule('creation'); actions.goExport(); }}
            >
              <DownloadIcon /> 导出
            </button>
          </div>
        </div>
        <div className="content">
          <div className="content-inner">
            <ViewHeader module={state.module} />
            <ErrorBoundary>
              {state.module === 'film' && <Film />}
              {state.module === 'spoken' && <Spoken />}
              {state.module === 'creation' && <Creation />}
              {state.module === 'settings' && <Settings />}
            </ErrorBoundary>
          </div>
        </div>
      </main>

      {/* Inspector */}
      <aside className={'inspector' + (inspectorOpen ? ' open' : '')}>
        <Inspector />
      </aside>

      {/* Taskbar */}
      <div className="taskbar">
        <span>STATUS · {state.task.label}</span>
        <span className="prog"><i style={{ width: state.task.p + '%' }} /></span>
      </div>

      {/* 生成进度弹窗（覆盖所有模块的生成任务） */}
      <GenProgress />

      {/* Mobile bottom navigation */}
      <nav className="mobile-nav">
        {MODULES.map((m) => (
          <button
            key={m.key}
            className={state.module === m.key ? 'active' : ''}
            onClick={() => goModule(m.key as ModuleKey)}
          >
            <span className="ico"><m.Icon size={20} /></span>
            <span>{m.name}</span>
          </button>
        ))}
      </nav>
    </div>
  );
}