import React, { createContext, useContext, useRef, useState } from 'react';
import {
  AppState, ModuleKey,
} from '../data/mock';
import {
  initialFilmCats, initialFilmProjects, initialEditorState, initialSpokenVideos,
  initialSettings, initialCreation, defaultSubs,
} from '../data/mock';
import { downloadJson, toSec } from '../lib/jianying';

const initialState: AppState = {
  module: 'film',
  task: { label: '空闲', p: 0 },
  filmCat: 'c1', filmStage: 'library', editorSub: 'gen',
  editingProj: null, selectedClip: null,
  filmCats: initialFilmCats, filmProjects: initialFilmProjects, editorState: initialEditorState,
  spokenSel: 'v1', spokenStage: 'upload',
  spokenVideos: initialSpokenVideos,
  cStage: 'req', cState: initialCreation,
  settingsSub: 'api', settingsState: initialSettings,
};

type SetState = React.Dispatch<React.SetStateAction<AppState>>;

interface AppCtx {
  state: AppState;
  set: SetState;
  task: (label: string, p?: number) => void;
  actions: ReturnType<typeof buildActions>;
}

const Ctx = createContext<AppCtx | null>(null);

export function useApp() {
  const c = useContext(Ctx);
  if (!c) throw new Error('useApp must be used within AppProvider');
  return c;
}

function buildActions(set: SetState, task: (l: string, p?: number) => void) {
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

  // ---------- 影片 ----------
  const importFilm = () => {
    const cat = initialState.filmCat;
    set((s) => {
      const projects = { ...s.filmProjects };
      const list = projects[s.filmCat] || [];
      const name = '新素材 ' + (list.length + 1);
      projects[s.filmCat] = [...list, { t: name, s: '草稿' }];
      const cats = s.filmCats.map((c) => c.id === s.filmCat ? { ...c, n: c.n + 1 } : c);
      return { ...s, filmProjects: projects, filmCats: cats, editingProj: { cat: s.filmCat, t: name }, filmStage: 'editor', editorSub: 'gen' };
    });
  };
  const openEditor = (cat: string, t: string) =>
    patch({ editingProj: { cat, t }, filmStage: 'editor', editorSub: 'gen', filmCat: cat });
  const goEditorSub = (id: string) => patch({ editorSub: id });

  const filmScriptMap: Record<string, string> = {
    '城市之光': '第一段，老街清晨，阳光洒在青石板上；第二段，转角面馆热气升腾；第三段，一碗面下肚，满足上扬；第四段，窗边静坐，写一段给城市的话。',
    '外婆的菜园': '第一段，晨雾里的菜畦；第二段，外婆弯腰摘豆；第三段，灶台烟火气；第四段，一碗时蔬汤，是童年的味。',
    '候鸟': '第一段，秋风起，翅影掠过湖面；第二段，南飞编队穿云；第三段，湿地歇脚；第四段，春归，生命轮回。',
  };
  const genFilmScript = () => {
    const proj = initialState.editingProj;
    const script = proj ? (filmScriptMap[proj.t] || '根据影片内容自动生成一段可二次编辑的解说文案……') : '根据影片内容自动生成一段可二次编辑的解说文案……';
    sim('生成解说文案…', 1200, () => set((s) => ({ ...s, editorState: { ...s.editorState, script } })));
  };
  const alignFilm = () => sim('导入视频并对齐文案…', 1300, () =>
    set((s) => ({ ...s, editorState: { ...s.editorState, imported: true, aligned: true, alignedPct: 100 } })));

  const buildVoiceLines = (script: string) =>
    script.split('\n').filter(Boolean).map((x, i) => ({ id: i, t: `0:0${i}`, x }));

  const genVoiceForFilm = () => sim('智能配音 + 生成字幕…', 1300, () =>
    set((s) => ({ ...s, editorState: { ...s.editorState, voiceLines: buildVoiceLines(s.editorState.script), aligned: true } })));
  const reVoiceForFilm = () => genVoiceForFilm();
  const editVoiceLine = (i: number, v: string) => set((s) => ({
    ...s, editorState: { ...s.editorState, voiceLines: (s.editorState.voiceLines || []).map((l) => l.id === i ? { ...l, x: v } : l) },
  }));
  const setVoiceMix = (v: number) => set((s) => ({ ...s, editorState: { ...s.editorState, voiceMix: v } }));
  const previewMix = () => task('预览混音（原声 ' + Math.round(initialState.editorState.voiceMix * 100) + '%）', 60);

  const autoCut = () => {
    const lines = initialState.editorState.script.split('\n').filter(Boolean);
    const cuts = lines.map((tx, i) => ({ t1: `0:0${i}`, t2: `0:1${i}`, tx, dur: 6, kept: true }));
    sim('自动切点…', 1200, () => set((s) => ({ ...s, editorState: { ...s.editorState, cuts } })));
  };
  const archiveToFilm = () => sim('归档到影片库…', 900, () => patch({ filmStage: 'library' }));

  // ---------- 口播 ----------
  const uploadSpoken = () => set((s) => {
    const id = 'v' + Date.now();
    const v = { id, name: '口播视频' + (s.spokenVideos.length + 1) + '.mp4', dur: '0' + (1 + Math.floor(Math.random() * 3)) + ':' + String(Math.floor(Math.random() * 60)).padStart(2, '0'), tr: [], script: null, keywords: [], assets: [], issues: [], matchResults: null, cleanScript: null };
    return { ...s, spokenVideos: [v, ...s.spokenVideos], spokenSel: id, spokenStage: 'upload' };
  });
  const transcribe = (id: string) => sim('识别音频中…', 1300, () => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === id ? { ...v, script: v.tr.map((r) => r.x.replace(/那个|呃/g, '')).join('') } : v),
  })));
  const setIssue = (vid: string, iid: string, val: boolean) => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === vid ? { ...v, issues: v.issues.map((i) => i.id === iid ? { ...i, accepted: val } : i) } : v),
  }));
  const acceptAllIssues = () => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === s.spokenSel ? { ...v, issues: v.issues.map((i) => ({ ...i, accepted: true })) } : v),
  }));
  const ignoreAllIssues = () => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === s.spokenSel ? { ...v, issues: v.issues.map((i) => ({ ...i, accepted: false })) } : v),
  }));
  const cleanFromAccepted = () => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === s.spokenSel ? {
      ...v, cleanScript: '大家好，今天给大家介绍我们的新产品 VideosFlow。\n它是一款基于 AI 的智能视频剪辑工具。\n可以自动根据文案剪辑视频。\n还能修掉口播里的气口和口误，提升观感。\n大家记得点赞关注哦。',
    } : v),
  }));
  const uploadAsset = (id: string) => {
    const v = initialState.spokenVideos.find((x) => x.id === id);
    const types: ('image' | 'bgm' | 'sfx' | 'clip')[] = ['image', 'bgm', 'sfx', 'clip'];
    set((s) => ({
      ...s, spokenVideos: s.spokenVideos.map((x) => {
        if (x.id !== id) return x;
        const arr = x.assets || [];
        const t = types[arr.length % 4];
        return { ...x, assets: [...arr, { name: '素材' + (arr.length + 1) + '.' + t, type: t }] };
      }),
    }));
    void v;
  };
  const delAsset = (id: string, name: string) => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((x) => x.id === id ? { ...x, assets: x.assets.filter((a) => a.name !== name) } : x),
  }));
  const doMatch = (id: string) => sim('智能匹配素材中…', 1200, () => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => {
      if (v.id !== id) return v;
      const segs = v.tr.filter((r) => r.x.length > 6);
      const matchResults = segs.slice(0, 4).map((r, i) => {
        const a = (v.assets && v.assets[i]) || null;
        const kw = (v.keywords && v.keywords[i]) || null;
        return { seg: r.t, text: r.x.slice(0, 12), asset: a ? a.name : '(暂无匹配素材)', kw: kw || '', applied: !!a };
      });
      return { ...v, matchResults };
    }),
  })));
  const toggleMatch = (id: string, seg: string) => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === id ? {
      ...v, matchResults: (v.matchResults || []).map((m) => m.seg === seg ? { ...m, applied: !m.applied } : m),
    } : v),
  }));
  const applyAllMatch = (id: string) => set((s) => ({
    ...s, spokenVideos: s.spokenVideos.map((v) => v.id === id ? { ...v, matchResults: (v.matchResults || []).map((m) => ({ ...m, applied: true })) } : v),
  }));
  const pickSpokenFlower = (id: string) => set((s) => ({ ...s, editorState: { ...s.editorState, flower: id } }));
  const burnFlower = () => sim('烧录花字到视频中…', 1300, () => { });
  const exportSpoken = () => sim('导出干净口播片段…', 1100, () => { });
  const exportSpokenJianYing = () => {
    sim('生成剪映工程文件…', 1300, () => {
      set((s) => {
        const v = s.spokenVideos.find((x) => x.id === s.spokenSel) || s.spokenVideos[0];
        const fps = 30;
        const tpl = s.editorState.flower;
        const total = v.dur ? toSec(v.dur) : v.tr.reduce((m, r) => Math.max(m, toSec(r.t)), 0);
        const colorMap: Record<string, string> = { emphasis: '#f59e0b', emotion: '#a855f7', shout: '#ef4444', keyword: '#e5e7eb', underline: '#3b82f6', shake: '#ef4444' };
        const texts = v.tr.map((r, i) => {
          const start = toSec(r.t);
          const end = v.tr[i + 1] ? toSec(v.tr[i + 1].t) : total;
          const kw = (v.keywords || []).find((k) => r.x.includes(k));
          return { id: 'text_' + i, content: r.x, start: +start.toFixed(3), end: +end.toFixed(3), flower: !!kw, template: tpl, color: kw ? (colorMap[tpl] || '#fff') : '#ffffff' };
        });
        const draft = {
          app_version: '5.x', fps, canvas: { w: 1920, h: 1080 }, duration: +total.toFixed(3),
          materials: { videos: [{ id: 'vid_0', file_name: v.name, type: 'video', duration: +total.toFixed(3) }], texts },
          tracks: [
            { type: 'video', segments: [{ material_id: 'vid_0', source: 'local', start: 0, end: +total.toFixed(3) }] },
            { type: 'text', segments: texts.map((t, i) => ({ material_id: 'text_' + i, start: t.start, end: t.end })) },
          ],
        };
        downloadJson('draft_content.json', draft);
        return s;
      });
    });
  };

  // ---------- 创作 ----------
  const genScript = () => sim('生成文案中…', 1200, () => set((s) => ({
    ...s,
    cStage: 'script',
    cState: { ...s.cState, script: '大家好，今天聊一个新手也能上手的事——用 AI 把文案变成视频。\n\n你只需要给个大体的需求，它就能自动写稿、拆分镜、出图片，还能配音加字幕。\n\n以前剪一条视频要折腾大半天，现在把想法交给它，剩下的交给流程。\n\n如果你也想轻松做视频，不妨试试看。' },
  })));
  const goHuman = () => patch({ cStage: 'human' });
  const doHuman = () => sim('去 AI 味中…', 1100, () => set((s) => ({
    ...s,
    cStage: 'story',
    cState: { ...s.cState, human: '嗨，今天说个特适合新手的事儿——用 AI 把文案直接变成视频。\n\n你大概说个想法就行，它自己写稿、拆镜头、出图，连配音字幕都帮你弄好。\n\n以前剪一条视频得忙活大半天，现在你把点子丢给它，流程自动跑完。\n\n想轻松做视频的话，真的可以试一下。' },
  })));
  const goStory = () => patch({ cStage: 'story' });
  const genStory = () => sim('生成分镜中…', 1200, () => set((s) => ({
    ...s,
    cStage: 'story',
    cState: { ...s.cState, story: [
      { desc: '开场：主持人近景微笑，背景虚化，轻松氛围。', dialogue: '嗨，今天说个特适合新手的事儿——用 AI 把文案变成视频。', dur: 5, cam: '近景' },
      { desc: '界面展示：AI 剪辑按钮高亮，光标点击。', dialogue: '你大概说个想法就行，它自己写稿、拆镜头、出图。', dur: 6, cam: '推近' },
      { desc: '动画：文案自动变成时间线与图片。', dialogue: '连配音字幕都帮你弄好，一条龙。', dur: 6, cam: '平摇' },
      { desc: '结尾：主持人比赞，品牌 logo 浮现。', dialogue: '想轻松做视频的话，真的可以试一下。', dur: 4, cam: '中景' },
    ] },
  })));
  const editStory = (i: number, f: string, v: string) => set((s) => ({
    ...s, cState: { ...s.cState, story: s.cState.story.map((sh, k) => k === i ? { ...sh, [f]: f === 'dur' ? +v : v } : sh) },
  }));
  const pickHumanPrompt = (v: string) => set((s) => ({ ...s, cState: { ...s.cState, humanPrompt: v } }));
  const pickStyleRef = (v: string) => set((s) => ({ ...s, cState: { ...s.cState, styleRef: v } }));
  const goImage = () => patch({ cStage: 'image' });
  const genImg = (i: number) => sim('生成图片 ' + (i + 1) + '…', 1100, () => set((s) => ({ ...s, cState: { ...s.cState, imgs: { ...s.cState.imgs, [i]: true } } })));
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
  const goFrames = () => patch({ cStage: 'frames' });
  const genFrames = (i: number) => sim('生成首尾帧视频 ' + (i + 1) + '…', 1300, () => set((s) => ({ ...s, cState: { ...s.cState, frames: { ...s.cState.frames, [i]: true } } })));
  const goVoice = () => patch({ cStage: 'voice' });
  const toggleVoice = (n: string) => set((s) => {
    const ex = s.cState.voices.find((x) => x.name === n);
    return { ...s, cState: { ...s.cState, voices: ex ? s.cState.voices.filter((x) => x.name !== n) : [...s.cState.voices, { name: n, ip: n }] } };
  });
  const setVoiceIP = (n: string, v: string) => set((s) => ({
    ...s, cState: { ...s.cState, voices: s.cState.voices.map((x) => x.name === n ? { ...x, ip: v } : x) },
  }));
  const genVoice = () => sim('生成配音…', 1200, () => set((s) => ({ ...s, cState: { ...s.cState, voice: { ok: true }, subs: defaultSubs() } })));
  const goExport = () => patch({ cStage: 'export' });
  const exportCreationJianYing = () => {
    sim('生成剪映工程文件…', 1300, () => {
      set((st) => {
        const shots = st.cState.story;
        const total = shots.reduce((m, x) => m + (x.dur || 0), 0) || 30;
        const texts = (st.cState.subs.length ? st.cState.subs : defaultSubs()).map((t, i) => ({ id: 'text_' + i, content: t.x, start: t.t, flower: false }));
        const voices = st.cState.voices.length ? st.cState.voices : [{ name: '默认', ip: '旁白' }];
        const draft = {
          app_version: '5.x', fps: 30, canvas: { w: 1920, h: 1080 }, duration: +total.toFixed(3),
          materials: {
            videos: shots.map((x, i) => ({ id: 'shot_' + i, file_name: '镜头' + (i + 1) + '.mp4', type: 'video', duration: +(x.dur || 0) })),
            texts, audios: voices.map((v, i) => ({ id: 'audio_' + i, voice: v.name, ip: v.ip })),
          },
          tracks: [
            { type: 'video', segments: shots.map((x, i) => ({ material_id: 'shot_' + i, start: 0, end: +(x.dur || 0) })) },
            { type: 'text', segments: texts.map((t, i) => ({ material_id: 'text_' + i, start: 0, end: +total })) },
            ...voices.map((v, i) => ({ type: 'audio', voice: v.name, ip: v.ip, segments: [{ material_id: 'audio_' + i, start: 0, end: +total }] })),
          ],
        };
        downloadJson('draft_content.json', draft);
        return st;
      });
    });
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

  return {
    set: patch, task,
    goModule, goSettingsSub,
    importFilm, openEditor, goEditorSub, genFilmScript, alignFilm,
    genVoiceForFilm, reVoiceForFilm, editVoiceLine, setVoiceMix, previewMix, autoCut, archiveToFilm,
    uploadSpoken, transcribe, setIssue, acceptAllIssues, ignoreAllIssues, cleanFromAccepted,
    uploadAsset, delAsset, doMatch, toggleMatch, applyAllMatch,
    pickSpokenFlower, burnFlower, exportSpoken, exportSpokenJianYing,
    genScript, goHuman, doHuman, goStory, genStory, editStory, pickHumanPrompt, pickStyleRef,
    goImage, genImg, addRef, setRefCat, setRefCatItem, delRef, goFrames, genFrames,
    goVoice, toggleVoice, setVoiceIP, genVoice, goExport, exportCreationJianYing,
    testProvider, savePrompt, saveSettings, resetSettings, updOther,
  };
}

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<AppState>(initialState);
  const taskTimer = useRef<number | undefined>(undefined);
  const task = (label: string, p?: number) => {
    setState((s) => ({ ...s, task: { label, p: p ?? s.task.p } }));
    if (label === '空闲' && taskTimer.current) window.clearTimeout(taskTimer.current);
  };
  const actions = buildActions(setState, task);
  return <Ctx.Provider value={{ state, set: setState, task, actions }}>{children}</Ctx.Provider>;
}
