import { useEffect, useRef, useState } from 'react';
import { useApp } from '../state/AppContext';
import type { AppState, TaskNav } from '../data/mock';

type Toast = { id: number; type: 'ok' | 'fail'; msg: string; nav?: TaskNav | null };

/**
 * 后台生成进度：
 * - 进行中：右上角浮窗（非阻塞，可最小化），用户可继续操作其它功能；浮窗上可点击「前往当前步骤」直接跳到正在生成的页面；
 * - 完成 / 失败：浮窗消失，右侧居中弹出 toast 提示，点击可跳转到对应完成页面（再次点击 × 关闭）；
 * - 右侧 Inspector 面板的「任务进度」卡也会实时反映当前 / 最近一次生成的状态与进度，可点击「查看」跳转。
 * 所有创作 / 影片 / 口播的异步生成任务都经 AppContext 的 task(label, p, nav) 统一上报跳转目标。
 */
export default function GenProgress() {
  const { state, set } = useApp();
  const { label, p } = state.task;
  const nav = state.taskNav;
  const idle = label === '空闲' || p <= 0;
  const failed = /失败|错误|error/i.test(label);
  const done = !idle && !failed && p >= 100;
  const active = !idle && !done && !failed;

  const [minimized, setMinimized] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const startRef = useRef(0);
  const [elapsed, setElapsed] = useState(0);
  const seen = useRef('');
  const toastId = useRef(0);

  useEffect(() => {
    if (idle) { startRef.current = 0; setElapsed(0); return; }
    if (!startRef.current) startRef.current = Date.now();
    const t = setInterval(() => setElapsed(Math.floor((Date.now() - startRef.current) / 1000)), 250);
    return () => clearInterval(t);
  }, [idle]);

  const pushToast = (type: 'ok' | 'fail', msg: string, n?: TaskNav | null) => {
    const id = ++toastId.current;
    setToasts((ts) => [...ts, { id, type, msg, nav: n ?? null }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), 8000);
  };

  // 完成 / 失败各只弹一次（按 label+进度去重）；携带当前跳转目标。
  useEffect(() => {
    if (idle) { seen.current = ''; return; }
    const key = `${label}::${Math.round(p)}`;
    if (key === seen.current) return;
    if (done) { seen.current = key; pushToast('ok', label, nav); }
    else if (failed) { seen.current = key; pushToast('fail', label, nav); }
    else { seen.current = key; }
  }, [label, p, done, failed, idle, nav]);

  /** 点击消息 / 浮窗「前往」：跳到生成任务对应的页面。 */
  const goTo = (n?: TaskNav | null) => {
    if (!n) return;
    set((s) => {
      const patch: Partial<AppState> = { module: n.module };
      if (n.module === 'creation') {
        patch.cStage = n.stage ?? s.cStage;
        if (n.sel) patch.creationSel = n.sel;
      } else if (n.module === 'film') {
        patch.filmStage = 'editor';
        if (n.stage) patch.editorSub = n.stage;
        if (n.sel) patch.editingProj = { cat: s.editingProj?.cat ?? 'c1', id: n.sel, t: s.editingProj?.t ?? '' };
      } else if (n.module === 'spoken') {
        if (n.stage) patch.spokenStage = n.stage;
        if (n.sel) patch.spokenSel = n.sel;
      }
      return { ...s, ...patch };
    });
  };

  const closeToast = (id: number) => setToasts((ts) => ts.filter((x) => x.id !== id));
  const onToastClick = (t: Toast) => { goTo(t.nav); closeToast(t.id); };

  const pct = Math.min(100, Math.max(4, p));

  return (
    <>
      {active && (
        <div className={'gen-pop' + (minimized ? ' min' : '')}>
          {minimized ? (
            <button className="gen-pop__min" onClick={() => setMinimized(false)}>
              <span className="spin" /> {Math.round(p)}% · {label.slice(0, 14)}
            </button>
          ) : (
            <div className="gen-pop__card">
              <div className="gen-pop__head">
                <span className="gen-pop__dot" />
                <span className="gen-pop__title">正在生成…</span>
                <button className="gen-pop__minbtn" onClick={() => setMinimized(true)} title="最小化到角落">—</button>
              </div>
              <div className="gen-pop__msg">{label}</div>
              <div className="gen-bar">
                <div className="gen-bar__fill" style={{ width: pct + '%' }} />
              </div>
              <div className="gen-pop__meta">
                <span>{Math.round(p)}%</span>
                <span>已用时 {elapsed}s</span>
              </div>
              {nav && (
                <button className="gen-pop__go" onClick={() => goTo(nav)} title="跳转到当前生成的页面">
                  前往当前步骤 →
                </button>
              )}
            </div>
          )}
        </div>
      )}

      <div className="toast-stack">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={'toast ' + t.type + (t.nav ? ' nav' : '')}
            onClick={() => onToastClick(t)}
            title={t.nav ? '点击跳转到完成的页面' : '点击关闭'}
          >
            <span className="toast__ico">{t.type === 'ok' ? '✓' : '⚠'}</span>
            <span className="toast__msg">{t.msg}</span>
            {t.nav && <span className="toast__go">查看 →</span>}
            <span className="toast__close" onClick={(e) => { e.stopPropagation(); closeToast(t.id); }}>×</span>
          </div>
        ))}
      </div>
    </>
  );
}
