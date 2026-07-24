import React, { createContext, useContext, useRef, useState, useEffect } from 'react';
import {
  AppState, ModuleKey, ProviderCfg, TaskNav,
} from '../data/mock';
import type { ProviderRow, ProgressMsg, FilmCategory, FilmProject, TimelineEnvelope, TimelineClip, CreationManifest } from '../ipc/types';
import {
  initialFilmCats, initialFilmProjects, initialEditorState, initialSpokenVideos,
  initialSettings, initialCreation, defaultSubs, type EditorState as EditorStateType,
} from '../data/mock';
import { downloadJson, toSec } from '../lib/jianying';
import { open } from '@tauri-apps/plugin-dialog';
import {
  loadFilmCats, createFilmCategory, renameFilmCategory, reorderFilmCategory, deleteFilmCategory,
  loadFilmProjects, createFilmProject, updateFilmProject, deleteFilmProject,
  loadTimeline, saveTimeline as persistTimeline, submitFilmImport, submitFilmSmartCut, submitFilmExport,
  loadSpokenVideos, createSpokenVideo, deleteSpokenVideo, getSpokenVideo,
  extractSpokenScript, loadSpokenEdits, setSpokenEditAccepted, applySpokenEdits,
  loadSpokenAssets, createSpokenAsset, deleteSpokenAsset,
  loadSpokenKeywords, loadSpokenMatches, toggleSpokenMatch, matchSpokenAssets,
  submitSpokenAsr, submitSpokenDetect, submitSpokenKeyword, submitSpokenBurn, submitSpokenExport,
  loadCreationProjects, getCreationProject, createCreationProject, updateCreationProject, deleteCreationProject,
  loadStoryboard, saveStoryboard, loadGeneratedAssets,
  submitScriptWrite, submitScriptHumanize, submitStoryboardGen, submitImageGen,
  getCreationManifest, submitCreationFrames, submitCreationVoice, submitCreationExport,
  submitFilmScriptGen,
} from '../ipc/providers';
import type { SpokenVideo as SpokenVideoDb, SpokenEdit, SpokenAsset, CreationProject, Shot, GeneratedAsset } from '../ipc/types';

const initialState: AppState = {
  module: 'film',
  task: { label: '空闲', p: 0 },
  taskNav: null,
  filmCat: 'c1', filmStage: 'library', editorSub: 'gen',
  editingProj: null, selectedClip: null,
  filmCats: initialFilmCats, filmProjects: initialFilmProjects, editorState: initialEditorState,
  spokenSel: 'v1', spokenStage: 'upload',
  spokenVideos: initialSpokenVideos,
  // M3：DB 形态默认空，AppProvider 启动 effect 会拉取
  spokenVideosDb: [],
  spokenEdits: [],
  spokenAssets: [],
  spokenKeywords: [],
  spokenMatches: [],
  cStage: 'req', cState: initialCreation,
  settingsSub: 'api', settingsState: initialSettings,
  // M4：DB 形态默认空，AppProvider 启动 effect 会拉取
  creationProjects: [],
  creationSel: null,
  creationSb: null,
  creationAssets: [],
  creationManifest: null,
};

type SetState = React.Dispatch<React.SetStateAction<AppState>>;

interface AppCtx {
  state: AppState;
  set: SetState;
  task: (label: string, p?: number, nav?: TaskNav | null) => void;
  actions: ReturnType<typeof buildActions>;
}

const Ctx = createContext<AppCtx | null>(null);

export function useApp() {
  const c = useContext(Ctx);
  if (!c) throw new Error('useApp must be used within AppProvider');
  return c;
}

/** 把一个秒数格式化为 m:ss（配音字幕时间轴展示用）。 */
function fmtSec(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

/** 构建一个新的工程级 EditorState（清空导入/对齐/时间线状态）。 */
function freshEditorState(projectId: string, videoName: string, videoPath = ''): EditorStateType {
  return {
    projectId,
    videoName,
    videoPath,
    script: initialEditorState.script,
    imported: false,
    aligned: false,
    alignedPct: 0,
    asr: [],
    timeline: null,
    voiceMix: initialEditorState.voiceMix,
    flower: initialEditorState.flower,
    selectedClipId: null,
    exportOpts: { ...initialEditorState.exportOpts },
    voiceLines: null,
  };
}

/** 把 TimelineEnvelope 扁平化出所有 clip（导出与时间线保存用）。 */
function flattenClips(env: TimelineEnvelope): TimelineClip[] {
  const out: TimelineClip[] = [];
  for (const tr of env.tracks) for (const c of tr.clips) out.push(c);
  return out;
}

function buildActions(set: SetState, task: (l: string, p?: number, nav?: TaskNav | null) => void, get: () => AppState) {
  const sim = (label: string, ms: number, fn: () => void) => {
    task(label, 40);
    window.setTimeout(() => {
      fn();
      task(label + ' ✓', 100);
      window.setTimeout(() => task('空闲', 0), 1500);
    }, ms);
  };
  const patch = (p: Partial<AppState>) => set((s) => ({ ...s, ...p }));

  // ---------- 通用导航 ----------
  const goModule = (m: ModuleKey) => patch({ module: m });
  const goSettingsSub = (id: string) => patch({ settingsSub: id });

  // ---------- 影片：数据加载 ----------
  const loadProjects = async (catId: string) => {
    try {
      const list = await loadFilmProjects(catId);
      set((s) => ({ ...s, filmProjects: { ...s.filmProjects, [catId]: list } }));
    } catch {
      /* 保持已有回退数据 */
    }
  };

  const initFilm = async () => {
    try {
      const cats = await loadFilmCats();
      const projects: Record<string, FilmProject[]> = {};
      await Promise.all(cats.map(async (c) => {
        try { projects[c.id] = await loadFilmProjects(c.id); } catch { projects[c.id] = []; }
      }));
      set((s) => ({ ...s, filmCats: cats, filmProjects: projects }));
    } catch {
      /* 保持初始 mock 回退 */
    }
  };

  // ---------- 影片：类型树 CRUD ----------
  const switchCat = async (catId: string) => {
    patch({ filmCat: catId });
    await loadProjects(catId);
  };

  const createCat = async (name: string) => {
    const cats = get().filmCats;
    const order = cats.reduce((m, c) => Math.max(m, c.order), 0) + 1;
    try {
      await createFilmCategory(name, order);
      const next = await loadFilmCats();
      set((s) => ({ ...s, filmCats: next }));
      task('已新建类型 ✓', 100);
    } catch (e) {
      task('新建类型失败：' + String(e), 100);
    }
  };

  const renameCat = async (id: string, name: string) => {
    try {
      await renameFilmCategory(id, name);
      const next = await loadFilmCats();
      set((s) => ({ ...s, filmCats: next }));
    } catch (e) { task('重命名失败：' + String(e), 100); }
  };

  const reorderCat = async (id: string, order: number) => {
    try {
      await reorderFilmCategory(id, order);
      const next = await loadFilmCats();
      set((s) => ({ ...s, filmCats: next }));
    } catch (e) { task('排序失败：' + String(e), 100); }
  };

  const moveCat = async (id: string, dir: -1 | 1) => {
    const cats = [...get().filmCats].sort((a, b) => a.order - b.order);
    const idx = cats.findIndex((c) => c.id === id);
    const swap = idx + dir;
    if (idx < 0 || swap < 0 || swap >= cats.length) return;
    const a = cats[idx];
    const b = cats[swap];
    try {
      await reorderFilmCategory(a.id, b.order);
      await reorderFilmCategory(b.id, a.order);
      const next = await loadFilmCats();
      set((s) => ({ ...s, filmCats: next }));
    } catch (e) { task('排序失败：' + String(e), 100); }
  };

  const deleteCat = async (id: string, strategy: string, targetId?: string) => {
    try {
      await deleteFilmCategory(id, strategy, targetId);
      const next = await loadFilmCats();
      set((s) => ({ ...s, filmCats: next }));
      const cur = get().filmCat === id ? (next[0]?.id ?? 'c1') : get().filmCat;
      patch({ filmCat: cur });
      await loadProjects(cur);
      task('已删除类型 ✓', 100);
    } catch (e) { task('删除类型失败：' + String(e), 100); }
  };

  // ---------- 影片：工程库 CRUD ----------
  const importFilm = async () => {
    const cat = get().filmCat;
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{
          name: '视频文件',
          extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'],
        }],
        title: '选择要导入的视频',
      });
      if (!selected) return;
      const videoPath = Array.isArray(selected) ? selected[0] : selected;
      const fileName = videoPath.replace(/.*[\\/]/, '');
      const list = get().filmProjects[cat] || [];
      const title = fileName.replace(/\.[^.]+$/, '') || ('新素材 ' + (list.length + 1));
      const id = await createFilmProject(cat, title, null);
      set((s) => {
        const projects = {
          ...s.filmProjects,
          [cat]: [...(s.filmProjects[cat] || []), {
            id, categoryId: cat, title, cover: null, status: '草稿', tags: '', createdAt: Date.now(),
          } as FilmProject],
        };
        return {
          ...s,
          filmProjects: projects,
          editingProj: { cat, id, t: title },
          filmStage: 'editor',
          editorSub: 'gen',
          editorState: freshEditorState(id, fileName, videoPath),
        };
      });
      task('已导入工程 ✓', 100);
    } catch (e) {
      task('导入失败：' + String(e), 100);
    }
  };

  const openEditor = async (cat: string, id: string, t: string) => {
    patch({ editingProj: { cat, id, t }, filmStage: 'editor', editorSub: 'gen', filmCat: cat });
    try {
      const row = await loadTimeline(id);
      if (row) {
        const env = JSON.parse(row.tracks) as TimelineEnvelope;
        set((s) => ({ ...s, editorState: { ...s.editorState, projectId: id, timeline: env } }));
      } else {
        set((s) => ({ ...s, editorState: { ...s.editorState, projectId: id } }));
      }
    } catch {
      set((s) => ({ ...s, editorState: { ...s.editorState, projectId: id } }));
    }
  };

  const updateProject = async (id: string, p: { title?: string; cover?: string | null; status?: string; tags?: string }) => {
    try {
      await updateFilmProject(id, p);
      await loadProjects(get().filmCat);
    } catch (e) { task('更新工程失败：' + String(e), 100); }
  };

  const deleteProject = async (id: string) => {
    try {
      await deleteFilmProject(id);
      if (get().editingProj?.id === id) patch({ filmStage: 'library', editingProj: null });
      await loadProjects(get().editingProj?.cat ?? get().filmCat);
      task('已删除工程 ✓', 100);
    } catch (e) { task('删除工程失败：' + String(e), 100); }
  };

  const goEditorSub = (id: string) => patch({ editorSub: id });

  // ---------- 影片：解说文案 ----------
  const filmScriptMap: Record<string, string> = {
    '城市之光': '第一段，老街清晨，阳光洒在青石板上；第二段，转角面馆热气升腾；第三段，一碗面下肚，满足上扬；第四段，窗边静坐，写一段给城市的话。',
    '外婆的菜园': '第一段，晨雾里的菜畦；第二段，外婆弯腰摘豆；第三段，灶台烟火气；第四段，一碗时蔬汤，是童年的味。',
    '候鸟': '第一段，秋风起，翅影掠过湖面；第二段，南飞编队穿云；第三段，湿地歇脚；第四段，春归，生命轮回。',
  };
  /** 生成解说文案：先尝试调 Rust 任务走 Agnes LLM，失败时降级到 sim（含 console 日志便于诊断）。 */
  const genFilmScript = () => {
    const proj = get().editingProj;
    console.log('[videosflow-debug] genFilmScript click: proj=', proj);
    if (!proj) {
      console.warn('[videosflow-debug] genFilmScript: no editingProj');
      task('请先导入影片', 100);
      return;
    }
    // M2.5：调真实 Rust 任务（film_script_gen）走 ASR→LLM→六段式
    task('生成解说文案中…', 10, { module: 'film', stage: 'gen', sel: proj.id });
    submitFilmScriptGen(proj.id, {
      videoPath: (proj as any).videoPath || '',
      title: (proj as any).title || (proj as any).t || '未命名视频',
      style: (proj as any).categoryId || 'movie',
      language: 'zh',
      duration: 180,
      hint: '',
    }, (m) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        const script = (m.payload as any)?.script || '';
        console.log('[videosflow-debug] film_script_gen done: scriptLen=', script.length);
        // 同步写回 React state（UI 立即可见）+ 触发后端更新 film_projects.script
        set((s) => ({
          ...s,
          editorState: { ...s.editorState, script },
        }));
        task('解说文案生成完成 ✓', 100);
        window.setTimeout(() => task('空闲', 0), 3000);
      } else if (m.status === 'failed') {
        // 失败时降级到 sim 占位
        const title = proj.t;
        const fallback = `根据影片「${title}」自动生成一段可二次编辑的解说文案：\n\n第一段，开场介绍影片背景与主题。\n第二段，中段展开主要情节与亮点。\n第三段，高潮部分渲染情绪与节奏。\n第四段，结尾总结主题并引导观后感。\n\n（提示：解说生成失败，已降级使用占位文案。可在设置页检查 LLM Key。）`;
        console.warn('[videosflow-debug] film_script_gen failed, fallback to sim');
        set((s) => ({ ...s, editorState: { ...s.editorState, script: fallback } }));
        task('解说文案生成失败，已使用降级文案：' + (m.message || ''), 100);
        window.setTimeout(() => task('空闲', 0), 5000);
      }
    }).catch((e) => {
      // 整个 IPC 链路异常，降级 sim
      console.error('[videosflow-debug] genFilmScript IPC failed:', String(e));
      const title = proj.t;
      const fallback = `根据影片「${title}」自动生成一段可二次编辑的解说文案：\n\n第一段，开场介绍影片背景与主题。\n第二段，中段展开主要情节与亮点。\n第三段，高潮部分渲染情绪与节奏。\n第四段，结尾总结主题并引导观后感。`;
      set((s) => ({ ...s, editorState: { ...s.editorState, script: fallback } }));
      task('解说文案已生成（前端兜底） ✓', 100);
      window.setTimeout(() => task('空闲', 0), 3000);
    });
  };

  // ---------- 影片：导入对齐（film_import 任务） ----------
  const alignFilm = () => {
    const proj = get().editingProj;
    if (!proj) return;
    const pid = proj.id;
    const videoPath = get().editorState.videoPath;
    const script = get().editorState.script;
    task('导入并抽取音轨…', 10, { module: 'film', stage: 'align', sel: proj.id });
    submitFilmImport(pid, videoPath, script, (m: ProgressMsg) => {
      task(m.message || '导入对齐中…', m.progress);
      if (m.status === 'done') {
        const payload = (m.payload || {}) as { alignedPct?: number; degraded?: boolean };
        loadTimeline(pid).then((row) => {
          if (row) {
            const env = JSON.parse(row.tracks) as TimelineEnvelope;
            set((s) => ({
              ...s,
              editorState: {
                ...s.editorState,
                projectId: pid,
                timeline: env,
                imported: true,
                aligned: true,
                alignedPct: payload.alignedPct ?? 0,
                asr: env.asr || [],
              },
            }));
          } else {
            set((s) => ({ ...s, editorState: { ...s.editorState, imported: true, aligned: true, alignedPct: payload.alignedPct ?? 0 } }));
          }
          task(payload.degraded ? '导入完成（ASR 降级）' : '导入对齐完成 ✓', 100);
          window.setTimeout(() => task('空闲', 0), 1800);
        }).catch(() => {
          task('时间线载入失败', 100);
        });
      } else if (m.status === 'failed') {
        task('导入失败：' + (m.message || '未知错误'), 100);
      }
    }).catch((e) => task('导入失败：' + String(e), 100));
  };

  // ---------- 影片：解说配音（本地预览；真实 TTS 混音在导出时执行） ----------
  const buildVoiceLines = (script: string) =>
    script.split('\n').filter(Boolean).map((x, i) => ({ id: i, t: fmtSec(i * 4), x }));

  const genVoiceForFilm = () => {
    const script = get().editorState.script;
    const lines = buildVoiceLines(script);
    task('智能配音 + 生成字幕…', 40, { module: 'film', stage: 'voice', sel: get().editingProj?.id });
    window.setTimeout(() => {
      set((s) => ({ ...s, editorState: { ...s.editorState, voiceLines: lines, aligned: true } }));
      task('配音生成 ✓', 100);
      window.setTimeout(() => task('空闲', 0), 1200);
    }, 1000);
  };
  const reVoiceForFilm = () => genVoiceForFilm();
  const editVoiceLine = (i: number, v: string) => set((s) => ({
    ...s, editorState: { ...s.editorState, voiceLines: (s.editorState.voiceLines || []).map((l) => l.id === i ? { ...l, x: v } : l) },
  }));
  const setVoiceMix = (v: number) => set((s) => ({ ...s, editorState: { ...s.editorState, voiceMix: v } }));
  const previewMix = () => task('预览混音（原声 ' + Math.round(get().editorState.voiceMix * 100) + '%）', 60);

  // ---------- 影片：智能粗剪（film_smart_cut 任务） ----------
  const autoCut = () => {
    const proj = get().editingProj;
    if (!proj) return;
    const pid = proj.id;
    const script = get().editorState.script;
    task('载入时间线并切点…', 10, { module: 'film', stage: 'cut', sel: proj.id });
    submitFilmSmartCut(pid, script, (m: ProgressMsg) => {
      task(m.message || '自动切点中…', m.progress);
      if (m.status === 'done') {
        loadTimeline(pid).then((row) => {
          if (row) {
            const env = JSON.parse(row.tracks) as TimelineEnvelope;
            set((s) => ({ ...s, editorState: { ...s.editorState, projectId: pid, timeline: env } }));
          }
          task('自动切点完成 ✓', 100);
          window.setTimeout(() => task('空闲', 0), 1500);
        }).catch(() => task('时间线载入失败', 100));
      } else if (m.status === 'failed') {
        task('切点失败：' + (m.message || '未知错误'), 100);
      }
    }).catch((e) => task('切点失败：' + String(e), 100));
  };

  // ---------- 影片：时间线保存 / 归档 ----------
  const saveTimeline = async () => {
    const proj = get().editingProj;
    const env = get().editorState.timeline;
    if (!proj || !env) return;
    try {
      await persistTimeline(proj.id, env, flattenClips(env));
      task('时间线已保存 ✓', 100);
    } catch (e) {
      task('保存时间线失败：' + String(e), 100);
    }
  };

  const archiveToFilm = async () => {
    await saveTimeline();
    const cat = get().editingProj?.cat ?? get().filmCat;
    patch({ filmStage: 'library', editingProj: null });
    await loadProjects(cat);
  };

  const goLibrary = () => {
    patch({ filmStage: 'library', editingProj: null });
  };

  // ---------- 影片：花字选择（与口播共用 editorState.flower） ----------
  const pickFlower = (id: string) => set((s) => ({ ...s, editorState: { ...s.editorState, flower: id } }));

  // ---------- 影片：导出（film_export 任务） ----------
  const setExportOpt = (p: Partial<EditorStateType['exportOpts']>) =>
    set((s) => ({ ...s, editorState: { ...s.editorState, exportOpts: { ...s.editorState.exportOpts, ...p } } }));

  const exportFilm = () => {
    const proj = get().editingProj;
    if (!proj) return;
    const opts = get().editorState.exportOpts;
    task('准备导出…', 5, { module: 'film', stage: 'out', sel: proj.id });
    submitFilmExport(proj.id, { ...opts, script: get().editorState.script }, (m: ProgressMsg) => {
      task(m.message || '导出中…', m.progress);
      if (m.status === 'done') {
        task('导出 MP4 完成 ✓', 100);
        window.setTimeout(() => task('空闲', 0), 1800);
      } else if (m.status === 'failed') {
        task('导出失败：' + (m.message || '未知错误'), 100);
      }
    }).catch((e) => task('导出失败：' + String(e), 100));
  };

  // ---------- 口播 ----------
  /** 启动时拉取 spokenVideos + 第一个视频的 edits/assets/keywords/matches */
  const loadSpoken = async () => {
    try {
      const list = await loadSpokenVideos();
      const sel = list[0]?.id ?? null;
      set((s) => ({ ...s, spokenVideosDb: list, spokenSel: sel }));
      if (sel) await refreshSpoken(sel);
    } catch { /* dev fallback：保留 initialSpokenVideos */ }
  };

  const refreshSpoken = async (videoId: string) => {
    try {
      const [edits, assets, kws, matches] = await Promise.all([
        loadSpokenEdits(videoId),
        loadSpokenAssets(videoId),
        loadSpokenKeywords(videoId),
        loadSpokenMatches(videoId),
      ]);
      set((s) => ({ ...s, spokenEdits: edits, spokenAssets: assets, spokenKeywords: kws, spokenMatches: matches }));
    } catch { /* 静默：UI 仍展示 mock 数据 */ }
  };

  /** 真实上传（前端先调 tauri-plugin-dialog 选文件，再调 create） */
  const uploadSpoken = async (filePath: string, fileName: string, durationSec: number) => {
    try {
      const id = await createSpokenVideo(fileName, filePath, durationSec);
      const list = await loadSpokenVideos();
      set((s) => ({ ...s, spokenVideosDb: list, spokenSel: id, spokenStage: 'tr', spokenEdits: [], spokenAssets: [], spokenKeywords: [], spokenMatches: [] }));
      task('已上传口播视频 ✓', 100);
      return id;
    } catch (e) {
      task('上传失败：' + String(e), 100);
      return null;
    }
  };

  /** 异步识别：抽音轨 → XiaomiMimo ASR → 写 transcript + script */
  const transcribe = async (videoId: string) => {
    task('识别音频中…', 10, { module: 'spoken', stage: 'tr', sel: videoId });
    submitSpokenAsr(videoId, (m) => {
      task(m.message || '识别中…', m.progress);
      if (m.status === 'done') {
        refreshSpoken(videoId);
        task((m.payload as any)?.degraded ? '识别完成（ASR 仅返回整段） ✓' : '识别完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('识别失败：' + (m.message || '未知错误'), 100);
      }
    }).catch((e) => task('识别失败：' + String(e), 100));
  };

  /** 单条采纳/忽略 */
  const setIssue = async (videoId: string, editId: string, val: boolean) => {
    try {
      await setSpokenEditAccepted(editId, val ? 1 : -1);
      const edits = await loadSpokenEdits(videoId);
      set((s) => ({ ...s, spokenEdits: edits }));
    } catch (e) { task('操作失败：' + String(e), 100); }
  };

  const acceptAllIssues = async (videoId: string) => {
    const list = get().spokenEdits.filter((e) => e.videoId === videoId);
    for (const e of list) {
      await setSpokenEditAccepted(e.id, 1);
    }
    const edits = await loadSpokenEdits(videoId);
    set((s) => ({ ...s, spokenEdits: edits }));
  };

  const ignoreAllIssues = async (videoId: string) => {
    const list = get().spokenEdits.filter((e) => e.videoId === videoId);
    for (const e of list) {
      await setSpokenEditAccepted(e.id, -1);
    }
    const edits = await loadSpokenEdits(videoId);
    set((s) => ({ ...s, spokenEdits: edits }));
  };

  const cleanFromAccepted = async (videoId: string) => {
    try {
      const clean = await applySpokenEdits(videoId);
      task('干净文案已生成 ✓', 100);
      const v = await getSpokenVideo(videoId);
      const list = await loadSpokenVideos();
      set((s) => ({ ...s, spokenVideosDb: list }));
      void clean; void v;
    } catch (e) { task('生成失败：' + String(e), 100); }
  };

  /** 上传素材（filePath 来自 tauri-plugin-dialog） */
  const uploadAsset = async (videoId: string, fileName: string, kind: string, filePath: string) => {
    try {
      await createSpokenAsset(videoId, fileName, kind, filePath);
      const assets = await loadSpokenAssets(videoId);
      set((s) => ({ ...s, spokenAssets: assets }));
    } catch (e) { task('素材上传失败：' + String(e), 100); }
  };

  const delAsset = async (videoId: string, assetId: string) => {
    try {
      await deleteSpokenAsset(assetId);
      const assets = await loadSpokenAssets(videoId);
      set((s) => ({ ...s, spokenAssets: assets }));
    } catch { /* noop */ }
  };

  /** 异步检测：gap/repeat/mistake → 写 spoken_edits */
  const doMatch = async (videoId: string) => {
    task('检测中…', 10, { module: 'spoken', stage: 'match', sel: videoId });
    submitSpokenDetect(videoId, (m) => {
      task(m.message || '检测中…', m.progress);
      if (m.status === 'done') {
        refreshSpoken(videoId);
        task('检测完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('检测失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('检测失败：' + String(e), 100));
  };

  const toggleMatch = async (videoId: string, matchId: string) => {
    try {
      await toggleSpokenMatch(matchId);
      const matches = await loadSpokenMatches(videoId);
      set((s) => ({ ...s, spokenMatches: matches }));
    } catch { /* noop */ }
  };

  const applyAllMatch = async (videoId: string) => {
    const list = get().spokenMatches.filter((m) => m.videoId === videoId);
    for (const m of list) {
      if (!m.applied) await toggleSpokenMatch(m.id);
    }
    const matches = await loadSpokenMatches(videoId);
    set((s) => ({ ...s, spokenMatches: matches }));
  };

  /** 关键词抽取（LLM → TF-IDF 降级） */
  const extractKeywords = async (videoId: string) => {
    task('抽取关键词中…', 10, { module: 'spoken', stage: 'match', sel: videoId });
    submitSpokenKeyword(videoId, (m) => {
      task(m.message || '抽取中…', m.progress);
      if (m.status === 'done') {
        refreshSpoken(videoId);
        task('关键词抽取完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('抽取失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('抽取失败：' + String(e), 100));
  };

  /** 同步：贪心匹配 spoken_keywords ↔ spoken_assets */
  const matchAssets = async (videoId: string) => {
    try {
      const matches = await matchSpokenAssets(videoId);
      set((s) => ({ ...s, spokenMatches: matches }));
      task('匹配完成 ✓', 100);
    } catch (e) { task('匹配失败：' + String(e), 100); }
  };

  const pickSpokenFlower = (id: string) => set((s) => ({ ...s, editorState: { ...s.editorState, flower: id } }));

  /** 异步：FFmpeg 烧录花字 */
  const burnFlower = (videoId: string, flower: string) => {
    task('烧录花字中…', 10, { module: 'spoken', stage: 'flw', sel: videoId });
    submitSpokenBurn(videoId, flower, (m) => {
      task(m.message || '烧录中…', m.progress);
      if (m.status === 'done') {
        task('烧录完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('烧录失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('烧录失败：' + String(e), 100));
  };

  /** 异步：基于 accepted edits 切片段 → 拼接 → 可选烧录 → 导出 */
  const exportSpoken = (videoId: string, burnFlowerFlag: boolean, flower: string) => {
    task('准备导出…', 5, { module: 'spoken', stage: 'flw', sel: videoId });
    submitSpokenExport(videoId, { burnFlower: burnFlowerFlag, flower }, (m) => {
      task(m.message || '导出中…', m.progress);
      if (m.status === 'done') {
        task('干净片段导出完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1800);
      } else if (m.status === 'failed') {
        task('导出失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('导出失败：' + String(e), 100));
  };

  /** 同步：构造并下载剪映工程 JSON（前端实现，Rust 端不参与） */
  const exportSpokenJianYing = () => {
    const v = get().spokenVideosDb.find((x) => x.id === get().spokenSel);
    if (!v) { task('请先选择口播视频', 100); return; }
    try {
      const transcript = JSON.parse(v.transcript || '[]') as { start: number; end: number; text: string }[];
      const total = v.duration || (transcript[transcript.length - 1]?.end ?? 30);
      const flower = get().editorState.flower;
      const colorMap: Record<string, string> = {
        emphasis: '#f59e0b', emotion: '#a855f7', shout: '#ef4444',
        keyword: '#e5e7eb', title: '#1f2430', signature: '#9ca3af',
      };
      const texts = transcript.map((r, i) => {
        const start = r.start ?? 0;
        const end = r.end ?? (transcript[i + 1]?.start ?? total);
        const kw = get().spokenKeywords.find((k) => k.text && r.text.includes(k.text));
        return {
          id: 'text_' + i,
          content: r.text,
          start: +start.toFixed(3),
          end: +end.toFixed(3),
          flower: !!kw,
          template: flower,
          color: kw ? (colorMap[flower] || '#fff') : '#ffffff',
        };
      });
      const draft = {
        app_version: '5.x',
        fps: 30,
        canvas: { w: 1920, h: 1080 },
        duration: +total.toFixed(3),
        materials: {
          videos: [{ id: 'vid_0', file_name: v.name, type: 'video', duration: +total.toFixed(3) }],
          texts,
        },
        tracks: [
          { type: 'video', segments: [{ material_id: 'vid_0', source: 'local', start: 0, end: +total.toFixed(3) }] },
          { type: 'text', segments: texts.map((t, i) => ({ material_id: 'text_' + i, start: t.start, end: t.end })) },
        ],
      };
      downloadJson('draft_content.json', draft);
      task('剪映工程已下载 ✓', 100);
    } catch (e) {
      task('剪映导出失败：' + String(e), 100);
    }
  };

  // ---------- 创作 ----------
  /** 启动时拉取所有创作工程 */
  const loadCreation = async () => {
    try {
      const list = await loadCreationProjects();
      const sel = list[0]?.id ?? null;
      set((s) => ({ ...s, creationProjects: list, creationSel: sel }));
      if (sel) await refreshCreation(sel);
    } catch { /* 静默：dev fallback 保留 */ }
  };

  const refreshCreation = async (projectId: string) => {
    try {
      const [proj, sb, assets, manifest] = await Promise.all([
        getCreationProject(projectId),
        loadStoryboard(projectId),
        loadGeneratedAssets(projectId),
        getCreationManifest(projectId),
      ]);
      set((s) => ({
        ...s,
        creationProjects: s.creationProjects.map((p) => (p.id === projectId ? proj : p)),
        creationSb: sb,
        creationAssets: assets,
        creationManifest: manifest,
      }));
    } catch { /* 静默 */ }
  };

  /** 创建新创作工程 */
  const createCreation = async (brief: string) => {
    try {
      const id = await createCreationProject(brief);
      const list = await loadCreationProjects();
      set((s) => ({
        ...s,
        creationProjects: list,
        creationSel: id,
        cStage: 'script',
        creationSb: null,
        creationAssets: [],
      }));
      await refreshCreation(id);
      task('创作工程已创建 ✓', 100);
      return id;
    } catch (e) { task('创建失败：' + String(e), 100); return null; }
  };

  const deleteCreation = async (projectId: string) => {
    try {
      await deleteCreationProject(projectId);
      const list = await loadCreationProjects();
      const newSel = list[0]?.id ?? null;
      set((s) => ({
        ...s,
        creationProjects: list,
        creationSel: newSel,
        creationSb: null,
        creationAssets: [],
      }));
      if (newSel) await refreshCreation(newSel);
      task('已删除创作工程 ✓', 100);
    } catch (e) { task('删除失败：' + String(e), 100); }
  };

  /** 异步：自动写文案 */
  const genScript = async (projectId: string) => {
    task('生成文案中…', 10, { module: 'creation', stage: 'script', sel: projectId });
    submitScriptWrite(projectId, (m) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        const script = (m.payload as any)?.script || '';
        // 直接刷新 React state，避免 mock 路径下 list→proj 的二次更新时序问题
        set((s) => ({
          ...s,
          creationSel: projectId,
          cStage: 'script',
          creationProjects: s.creationProjects.map((p) => (p.id === projectId ? { ...p, script, status: 'writing' as const } : p)),
          cState: { ...s.cState, script },
        }));
        // 生成文案后自动去 AI 味（用户无感），去味完成由 doHuman 内部跳到分镜
        task('文案已生成，正在自动去 AI 味…', 60);
        doHuman(projectId).catch(() => undefined);
      } else if (m.status === 'failed') {
        task('文案生成失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('文案生成失败：' + String(e), 100));
  };

  /** 异步：去 AI 味 */
  const doHuman = async (projectId: string) => {
    task('去 AI 味中…', 10);
    submitScriptHumanize(projectId, (m) => {
      task(m.message || '去 AI 味中…', m.progress);
      if (m.status === 'done') {
        const human = (m.payload as any)?.human || '';
        // 停留在「文案」页，让用户查看 / 编辑 / 保存；不再自动跳到分镜
        set((s) => ({
          ...s,
          creationProjects: s.creationProjects.map((p) => (p.id === projectId ? { ...p, humanizedScript: human, status: 'humanized' as const } : p)),
          cState: { ...s.cState, human },
        }));
        task('去 AI 味完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('去 AI 味失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('去 AI 味失败：' + String(e), 100));
  };

  /** 同步：跳步（无异步任务） */
  const goStory = () => patch({ cStage: 'story' });
  const goImage = () => patch({ cStage: 'image' });
  const goFrames = () => patch({ cStage: 'frames' });
  const goVoice = () => patch({ cStage: 'voice' });
  const goExport = () => patch({ cStage: 'export' });

  /** 异步：生成分镜 */
  const genStory = async (projectId: string) => {
    task('生成分镜中…', 10, { module: 'creation', stage: 'story', sel: projectId });
    submitStoryboardGen(projectId, (m) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        let shots: any[] = [];
        try {
          const raw = (m.payload as any)?.shots;
          shots = typeof raw === 'string' ? JSON.parse(raw) : raw;
        } catch { shots = []; }
        // 同步三处：creationProjects 列表项、cState.story（编辑态）、creationSb（DB 态，ImageView 用）
        set((s) => ({
          ...s,
          creationProjects: s.creationProjects.map((p) => p.id === projectId ? { ...p, status: 'storyboard' as const } : p),
          creationSb: { id: 'sb-' + Date.now().toString(36), projectId, shots: Array.isArray(shots) ? shots : [], styleRef: '现实', updatedAt: Date.now() },
          cState: { ...s.cState, story: Array.isArray(shots) ? shots : s.cState.story },
        }));
        task('分镜生成完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('分镜生成失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('分镜生成失败：' + String(e), 100));
  };

  /** 编辑单个分镜字段（实时写 state，同步 cState.story 与 creationSb.shots） */
  const editStory = (i: number, f: string, v: string) => set((s) => {
    const val = f === 'dur' ? +v : v;
    const story = s.cState.story.map((sh, k) => k === i ? { ...sh, [f]: val } : sh);
    const creationSb = s.creationSb
      ? { ...s.creationSb, shots: s.creationSb.shots.map((sh, k) => k === i ? { ...sh, [f]: val } : sh) }
      : s.creationSb;
    return { ...s, cState: { ...s.cState, story }, creationSb };
  });

  /** 保存分镜到 storyboards 表（前端编辑后） */
  const persistStory = async (projectId: string) => {
    const shots = get().cState.story;
    const styleRef = get().cState.styleRef || '现实';
    if (shots.length === 0) { task('分镜为空', 100); return; }
    try {
      // 映射补齐 ipc/types.Shot 必填字段 index
      const normalized = shots.map((s, i) => ({ index: s.index ?? i, desc: s.desc, dialogue: s.dialogue, dur: s.dur, cam: s.cam }));
      await saveStoryboard(projectId, normalized, styleRef);
      task('分镜已保存 ✓', 100);
      await refreshCreation(projectId);
    } catch (e) { task('保存失败：' + String(e), 100); }
  };

  /** 编辑并保存文案（去 AI 味后的终稿）到 creation_projects.script/humanized_script。 */
  const saveScript = async (projectId: string, text: string) => {
    const t = text.trim();
    if (!t) { task('文案为空，无法保存', 100); return; }
    try {
      await updateCreationProject(projectId, { humanizedScript: t, status: 'humanized' });
      set((s) => ({
        ...s,
        creationProjects: s.creationProjects.map((p) => p.id === projectId ? { ...p, humanizedScript: t } : p),
        cState: { ...s.cState, human: t },
      }));
      task('文案已保存 ✓', 100);
      setTimeout(() => task('空闲', 0), 1200);
    } catch (e) { task('保存失败：' + String(e), 100); }
  };

  const pickHumanPrompt = (v: string) => set((s) => ({ ...s, cState: { ...s.cState, humanPrompt: v } }));
  const pickStyleRef = (v: string) => set((s) => ({ ...s, cState: { ...s.cState, styleRef: v } }));

  /** 参考图分类管理（前端 state，不入 DB） */
  const addRef = (i: number, files: { name: string; dataUrl: string }[]) => set((s) => {
    const cat = s.cState.refCat[i] || 'IP形象';
    const arr = s.cState.refs[i] || [];
    const next = [...arr, ...files.map((f) => ({ name: f.name, dataUrl: f.dataUrl, cat }))];
    return { ...s, cState: { ...s.cState, refs: { ...s.cState.refs, [i]: next } } };
  });
  const setRefCat = (i: number, v: string) => set((s) => ({ ...s, cState: { ...s.cState, refCat: { ...s.cState.refCat, [i]: v } } }));
  const setRefCatItem = (i: number, ri: number, v: string) => set((s) => ({
    ...s, cState: { ...s.cState, refs: { ...s.cState.refs, [i]: (s.cState.refs[i] || []).map((r, k) => k === ri ? { ...r, cat: v } : r) } },
  }));
  const delRef = (i: number, ri: number) => set((s) => ({
    ...s, cState: { ...s.cState, refs: { ...s.cState.refs, [i]: (s.cState.refs[i] || []).filter((_, k) => k !== ri) } },
  }));

  /** 异步：单镜图片生成 */
  const genImg = (projectId: string, i: number) => {
    const styleRef = get().cState.styleRef || '现实';
    task('生成图片 ' + (i + 1) + '…', 10, { module: 'creation', stage: 'image', sel: projectId });
    submitImageGen(projectId, i, styleRef, (m) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        set((s) => ({ ...s, cState: { ...s.cState, imgs: { ...s.cState.imgs, [i]: true } } }));
        refreshCreation(projectId);
        task('图片生成完成 ✓', 100);
        setTimeout(() => task('空闲', 0), 1500);
      } else if (m.status === 'failed') {
        task('图片生成失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('图片生成失败：' + String(e), 100));
  };

  /** M5-① 首尾帧视频：把各镜首帧图（图片步产物）生成运镜片段，可选尾帧做 crossfade。 */
  const genFrames = (projectId: string) => {
    const tails = get().cState.tails || {};
    task('生成首尾帧视频中…', 10, { module: 'creation', stage: 'frames', sel: projectId });
    submitCreationFrames(projectId, tails, (m: ProgressMsg) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        refreshCreation(projectId).then(() => {
          const man = get().creationManifest;
          const shots = get().cState.story;
          const frames: Record<number, boolean> = {};
          shots.forEach((sh, i) => {
            const idx = (sh.index ?? i);
            frames[idx] = !!(man && man.clips[String(idx)]);
          });
          set((s) => ({ ...s, cState: { ...s.cState, frames } }));
          task('首尾帧视频生成完成 ✓', 100);
          window.setTimeout(() => task('空闲', 0), 1800);
        });
      } else if (m.status === 'failed') {
        task('生成失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('生成失败：' + String(e), 100));
  };

  /** M5-② 配音：逐镜台词走 TTS 生成 wav（统一音色 voiceName）。 */
  const genVoice = (projectId: string) => {
    const voice = get().cState.voiceName || 'mimo_default';
    task('生成配音中…', 10, { module: 'creation', stage: 'voice', sel: projectId });
    submitCreationVoice(projectId, voice, (m: ProgressMsg) => {
      task(m.message || '生成中…', m.progress);
      if (m.status === 'done') {
        refreshCreation(projectId).then(() => {
          const man = get().creationManifest;
          const shots = get().cState.story;
          const ok = shots.some((sh, i) => {
            const idx = (sh.index ?? i);
            return !!(man && man.audios[String(idx)]);
          });
          set((s) => ({ ...s, cState: { ...s.cState, voice: { ok } } }));
          task('配音生成完成 ✓', 100);
          window.setTimeout(() => task('空闲', 0), 1800);
        });
      } else if (m.status === 'failed') {
        task('配音失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('配音失败：' + String(e), 100));
  };

  /** 上传单镜尾帧图（绝对路径），用于首尾帧视频的 crossfade 过渡。 */
  const setCreationTail = (i: number, path: string) => set((s) => ({
    ...s, cState: { ...s.cState, tails: { ...s.cState.tails, [i]: path } },
  }));
  const clearCreationTail = (i: number) => set((s) => {
    const next = { ...s.cState.tails };
    delete next[i];
    return { ...s, cState: { ...s.cState, tails: next } };
  });

  /** 选择配音音色（全局统一用于本工程）。 */
  const setCreationVoice = (v: string) => set((s) => ({ ...s, cState: { ...s.cState, voiceName: v } }));

  /** M5-③ 导出成片：拼接 + 混音 + 烧字幕 → 最终 MP4。 */
  const exportCreation = (projectId: string, subtitleStyle: string) => {
    task('导出成片中…', 5, { module: 'creation', stage: 'export', sel: projectId });
    submitCreationExport(projectId, subtitleStyle, (m: ProgressMsg) => {
      task(m.message || '导出中…', m.progress);
      if (m.status === 'done') {
        const outPath = (m.payload as any)?.outPath || '';
        refreshCreation(projectId).then(() => {
          set((s) => ({ ...s, cState: { ...s.cState, exportedPath: outPath } }));
          if (outPath) task('成片导出完成 ✓', 100);
          else task('成片导出完成（预览见各片段）', 100);
          window.setTimeout(() => task('空闲', 0), 2000);
        });
      } else if (m.status === 'failed') {
        task('导出失败：' + (m.message || ''), 100);
      }
    }).catch((e) => task('导出失败：' + String(e), 100));
  };

  /** 下载创作字幕 SRT：按 shots 台词 + 时长累计时间轴生成（与成片分段对齐）。 */
  const downloadCreationSrt = (projectId: string) => {
    const shots = get().cState.story;
    if (shots.length === 0) { task('分镜为空', 100); return; }
    const fmt = (sec: number) => {
      const h = Math.floor(sec / 3600);
      const m = Math.floor((sec % 3600) / 60);
      const s = Math.floor(sec % 60);
      const ms = Math.floor((sec - Math.floor(sec)) * 1000);
      return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')},${String(ms).padStart(3, '0')}`;
    };
    let cur = 0;
    const blocks: string[] = [];
    shots.forEach((sh, i) => {
      const dur = Number(sh.dur) || 4;
      const text = (sh.dialogue || '').trim();
      if (!text) { cur += dur; return; }
      const start = cur;
      const end = cur + dur;
      blocks.push(`${i + 1}\n${fmt(start)} --> ${fmt(end)}\n${text}\n`);
      cur = end;
    });
    const blob = new Blob([blocks.join('\n')], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${projectId}-字幕.srt`;
    a.click();
    URL.revokeObjectURL(url);
    task('字幕 SRT 已下载 ✓', 100);
  };

  // ---------- 设置 ----------
  const testProvider = (k: string) => set((s) => ({
    ...s, settingsState: { ...s.settingsState, providers: { ...s.settingsState.providers, [k]: { ...s.settingsState.providers[k], test: 'ok' } } },
  }));
  const savePrompt = () => task('提示词已保存 ✓', 100);
  const saveSettings = () => sim('保存全部配置…', 700, () => { });
  const resetSettings = () => patch({ settingsState: initialSettings });
  const updOther = (k: string, v: unknown) => set((s) => ({
    ...s, settingsState: { ...s.settingsState, other: { ...s.settingsState.other, [k]: v } },
  }));

  const hydrateProviders = (rows: ProviderRow[]) => set((s) => {
    const providers: Record<string, ProviderCfg> = {};
    for (const r of rows) {
      const mode = (r.mode === 'local' ? 'local' : 'cloud') as 'cloud' | 'local';
      providers[r.kind] = {
        name: r.name, provider: r.provider, baseUrl: r.baseUrl,
        apiKey: '', model: r.model, enabled: r.enabled,
        hasKey: r.hasKey,
        mode,
        test: mode === 'local' ? 'local' : (r.hasKey ? 'ok' : 'idle'),
      };
    }
    return { ...s, settingsState: { ...s.settingsState, providers } };
  });
  const setProviderTest = (kind: string, status: string) => set((s) => ({
    ...s,
    settingsState: {
      ...s.settingsState,
      providers: {
        ...s.settingsState.providers,
        [kind]: { ...s.settingsState.providers[kind], test: status },
      },
    },
  }));

  return {
    set: patch, task,
    goModule, goSettingsSub,
    initFilm, loadProjects, switchCat,
    createCat, renameCat, reorderCat, moveCat, deleteCat,
    importFilm, openEditor, updateProject, deleteProject, goEditorSub, genFilmScript, alignFilm,
    genVoiceForFilm, reVoiceForFilm, editVoiceLine, setVoiceMix, previewMix, autoCut,
    saveTimeline, archiveToFilm, goLibrary, pickFlower, setExportOpt, exportFilm,
    // M2.5：影片解说生成（async 真链路）
    submitFilmScriptGen: genFilmScript,
    // M3：口播
    loadSpoken, refreshSpoken, uploadSpoken, transcribe,
    setIssue, acceptAllIssues, ignoreAllIssues, cleanFromAccepted,
    uploadAsset, delAsset, doMatch, toggleMatch, applyAllMatch,
    extractKeywords, matchAssets,
    pickSpokenFlower, burnFlower, exportSpoken, exportSpokenJianYing,
    // 创作
    loadCreation, refreshCreation, createCreation, deleteCreation,
    genScript, doHuman, goStory, genStory, editStory, persistStory, saveScript,
    pickHumanPrompt, pickStyleRef,
    goImage, genImg, addRef, setRefCat, setRefCatItem, delRef, goFrames, genFrames,
    goVoice, genVoice, setCreationVoice, setCreationTail, clearCreationTail, goExport, exportCreation, downloadCreationSrt,
    testProvider, savePrompt, saveSettings, resetSettings, updOther,
    hydrateProviders, setProviderTest,
  };
}

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<AppState>(initialState);
  const stateRef = useRef<AppState>(state);
  stateRef.current = state;
  const taskTimer = useRef<number | undefined>(undefined);
  const task = (label: string, p?: number, nav?: TaskNav | null) => {
    setState((s) => ({
      ...s,
      task: { label, p: p ?? s.task.p },
      // 显式传 nav 时更新跳转目标；空闲时清空；否则沿用已有目标（进度回调不覆盖）。
      taskNav: nav !== undefined ? nav : (label === '空闲' ? null : s.taskNav),
    }));
    if (label === '空闲' && taskTimer.current) window.clearTimeout(taskTimer.current);
  };
  const actions = buildActions(setState, task, () => stateRef.current);

  // 启动即拉取类型树与工程库（真实或 mock 回退）
  useEffect(() => {
    actions.initFilm();
    actions.loadSpoken();
    actions.loadCreation();
    // Dev only: 挂载到 window 方便 devtools 调试与回归
    if ((import.meta as any).env?.DEV) {
      (window as any).__VIDEOSFLOW__ = { state, actions, task };
      console.log('[videosflow-debug] window.__VIDEOSFLOW__ mounted for devtools');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <Ctx.Provider value={{ state, set: setState, task, actions }}>{children}</Ctx.Provider>;
}
