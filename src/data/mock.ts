// VideosFlow — 全局类型与初始 mock 数据
// 数据形态与交互流程对齐 preview/prototype.html，供 React 端状态初始化使用。

export type ModuleKey = 'film' | 'spoken' | 'creation' | 'settings';

export interface FilmCat { id: string; name: string; n: number; }
export interface FilmProject { t: string; s: string; }

export interface EditorState {
  videoName: string;
  imported: boolean;
  script: string;
  aligned: boolean;
  alignedPct: number;
  cuts: { t1: string; t2: string; tx: string; dur: number; kept: boolean }[] | null;
  clips: { id: string; track: string; start: number; end: number; label: string }[] | null;
  flower: string;
  voiceLines: { id: number; t: string; x: string }[] | null;
  voiceMix: number; // 原片原声占比 0-1
}

export interface SpokenIssue {
  id: string; type: 'gap' | 'mistake' | 'repeat'; ti: string; tx: string;
  suggestion: string; accepted: boolean | null;
}
export interface SpokenAsset { name: string; type: 'image' | 'bgm' | 'sfx' | 'clip'; }
export interface SpokenMatch { seg: string; text: string; kw: string; asset: string; applied: boolean; }
export interface SpokenVideo {
  id: string; name: string; dur: string;
  tr: { t: string; x: string }[];
  script: string | null;
  keywords: string[];
  assets: SpokenAsset[];
  issues: SpokenIssue[];
  matchResults: SpokenMatch[] | null;
  cleanScript: string | null;
}

export interface Shot { desc: string; dialogue: string; dur: number; cam: string; }
export interface VoiceSel { name: string; ip: string; }
export interface RefImg { name: string; dataUrl: string; cat: string; }
export interface CreationState {
  reqFromSpoken: string;
  script: string;
  human: string;
  story: Shot[];
  imgs: Record<number, boolean>;
  frames: Record<number, boolean>;
  voice: { ok: boolean };
  subs: { t: string; x: string }[];
  refs: Record<number, RefImg[]>;
  voices: VoiceSel[];
  humanPrompt: string;
  styleRef: string;
  refCat: Record<number, string>;
}

export interface ProviderCfg {
  name: string; provider: string; baseUrl: string; apiKey: string;
  model: string; enabled: boolean; test: string;
}
export interface PromptCfg { name: string; body: string; placeholder: string; }
export interface SettingsState {
  providers: Record<string, ProviderCfg>;
  prompts: Record<string, PromptCfg>;
  promptEditing: string;
  other: {
    theme: string; lang: string; hwAccel: boolean; exportResolution: string; exportFormat: string;
    taskConcurrency: number; autoSave: boolean; autoSaveSec: number; cleanupDays: number;
    ffmpegPath: string; tempDir: string;
  };
}

export const flowerTpls = [
  { id: 'emphasis', name: '重点强调', desc: '黄底加粗', cls: 'emphasis', demo: '关键词' },
  { id: 'emotion', name: '情绪渲染', desc: '粉紫渐变', cls: 'emotion', demo: '感慨一下' },
  { id: 'shout', name: '强烈感叹', desc: '红色大字', cls: 'shout', demo: '注意！' },
  { id: 'keyword', name: '关键词描边', desc: '白底边框', cls: 'keyword', demo: '新产品' },
  { id: 'underline', name: '重点下划线', desc: '蓝色加下划线', cls: 'underline', demo: '核心点' },
  { id: 'shake', name: '警示抖动', desc: '红色虚框', cls: 'shake', demo: '别忘了' },
];

export const stylePresets: Record<string, { tone: string; font: string; cam: string }> = {
  '现实': { tone: '自然写实·暖色', font: '无衬线', cam: '平实推进' },
  '科幻': { tone: '冷蓝紫·霓虹', font: '科技感几何', cam: '推拉摇移' },
  '卡通': { tone: '明快多彩', font: '圆体卡通', cam: '弹性运镜' },
  '写实': { tone: '高对比·胶片感', font: '衬线', cam: '固定长镜' },
  '动漫': { tone: '二次元·高饱和', font: '手写感', cam: '分镜式切' },
  '水彩': { tone: '淡彩晕染', font: '手写体', cam: '缓慢平移' },
};

export const refCats = ['IP形象', '场景', '产品', '风格', '材质', '其他'];

export const filmSteps = [
  { id: 'gen', name: '生成解说文案' },
  { id: 'align', name: '导入对齐' },
  { id: 'voice', name: '解说配音' },
  { id: 'cut', name: '自动切点' },
  { id: 'time', name: '时间线精修' },
  { id: 'out', name: '字幕花字导出' },
];

export const spokenSteps = [
  { id: 'upload', name: '上传' },
  { id: 'tr', name: '识别' },
  { id: 'fix', name: '纠正' },
  { id: 'match', name: '匹配素材' },
  { id: 'flw', name: '花字字幕' },
];

export const cSteps = [
  { id: 'req', name: '需求' },
  { id: 'script', name: '文案' },
  { id: 'human', name: '去AI味' },
  { id: 'story', name: '分镜' },
  { id: 'image', name: '图片' },
  { id: 'frames', name: '首尾帧视频' },
  { id: 'voice', name: '配音+字幕' },
  { id: 'export', name: '导出' },
];

export const settingsSteps = [
  { id: 'api', name: '模型 API' },
  { id: 'prompt', name: '提示词' },
  { id: 'other', name: '其他参数' },
];

export const initialFilmCats: FilmCat[] = [
  { id: 'c1', name: '电影', n: 3 }, { id: 'c2', name: '故事', n: 2 },
  { id: 'c3', name: '电视剧', n: 1 }, { id: 'c4', name: '动画片', n: 4 },
  { id: 'c5', name: '记录片', n: 2 },
];

export const initialFilmProjects: Record<string, FilmProject[]> = {
  c1: [{ t: '城市之光', s: '已发布' }, { t: '归途', s: '草稿' }, { t: '暗涌', s: '草稿' }],
  c2: [{ t: '外婆的菜园', s: '已发布' }, { t: '雨夜书店', s: '草稿' }],
  c3: [{ t: '长河', s: '制作中' }],
  c4: [{ t: '喵星日记', s: '已发布' }, { t: '齿轮王国', s: '已发布' }, { t: '云朵工厂', s: '草稿' }, { t: '小灯塔', s: '已发布' }],
  c5: [{ t: '候鸟', s: '已发布' }, { t: '匠心', s: '制作中' }],
};

export const initialEditorState: EditorState = {
  videoName: '旅行 vlog 原始素材.mp4',
  imported: false,
  script:
    '第一段，开场我走在这条老街上，阳光打在青石板上。\n第二段，转角有家老店，老板正在煮面，热气腾腾。\n第三段，我点了一碗面，尝一口，嘴角上扬。\n第四段，结尾我坐在窗边，看着行人，写一段话。',
  aligned: false, alignedPct: 0,
  cuts: null, clips: null, flower: 'emphasis', voiceLines: null, voiceMix: 0.15,
};

export const initialSpokenVideos: SpokenVideo[] = [
  {
    id: 'v1', name: '产品介绍口播.mp4', dur: '03:24',
    tr: [
      { t: '0:00', x: '大家好，今天给大家介绍我们的新产品 VideosFlow。' },
      { t: '0:03', x: '那个，它是一款，呃，基于 AI 的智能视频剪辑工具。' },
      { t: '0:08', x: '可以自动根据文案，根据文案剪辑视频。' },
      { t: '0:12', x: '还能修掉口播里的气口和口误，提升观感。' },
      { t: '0:17', x: '那个，那个，大家记得点赞关注哦。' },
      { t: '0:22', x: '还能修掉口播里的气口和口误，提升观感。' },
    ],
    script: null,
    keywords: ['AI', '智能', '剪辑', '气口', '口误', '重点'],
    assets: [
      { name: '产品截图.png', type: 'image' },
      { name: '背景音乐-BGM.mp3', type: 'bgm' },
      { name: '转场音效.wav', type: 'sfx' },
      { name: '品牌logo片段.mp4', type: 'clip' },
    ],
    issues: [
      { id: 'i1', type: 'gap', ti: '0:03', tx: '那个，它是一款，呃，', suggestion: '删除冗余填充词', accepted: null },
      { id: 'i2', type: 'repeat', ti: '0:08', tx: '根据文案，根据文案', suggestion: '合并重复', accepted: null },
      { id: 'i3', type: 'repeat', ti: '0:17', tx: '那个，那个，', suggestion: '删除重复填充', accepted: null },
      { id: 'i4', type: 'repeat', ti: '0:22', tx: '（与 0:12 重复）', suggestion: '删除重复句', accepted: null },
    ],
    matchResults: null,
    cleanScript: null,
  },
];

export const initialSettings: SettingsState = {
  providers: {
    llm: { name: '文字大模型', provider: 'OpenAI 兼容', baseUrl: 'https://api.openai.com/v1', apiKey: 'sk-************M2vN', model: 'gpt-4o-mini', enabled: true, test: 'ok' },
    img: { name: '图片大模型', provider: '通义万相', baseUrl: 'https://dashscope.aliyuncs.com/api/v1', apiKey: 'sk-************tPx8', model: 'wanx-v1', enabled: true, test: 'ok' },
    asr: { name: '语音识别', provider: '本地 faster-whisper', baseUrl: '', apiKey: '', model: 'medium', enabled: true, test: 'local' },
    tts: { name: '语音合成', provider: 'Edge-TTS', baseUrl: '', apiKey: '', model: 'zh-CN-XiaoxiaoNeural', enabled: true, test: 'ok' },
    video: { name: '视频大模型', provider: 'Runway / 通义万相', baseUrl: '', apiKey: 'sk-************aB91', model: 'gen-3', enabled: false, test: 'idle' },
  },
  prompts: {
    script: { name: '自动写文案', body: '请你担任资深短视频文案，根据以下需求撰写一份适合配音的画面感文案，长度约 60-80 字，语气自然：\n\n需求：{{brief}}\n风格：{{style}}\n受众：{{audience}}', placeholder: '{{brief}}\n{{style}}\n{{audience}}' },
    humanize: { name: '去 AI 味', body: '请你把以下文案改写成自然口语，去掉 AI 套话与空泛表达，加具体细节、停顿感与生活化比喻，保持原意：\n\n{{script}}', placeholder: '{{script}}' },
    storyboard: { name: '生成分镜', body: '请将以下文案拆为 4-6 个镜头，每个镜头给出：画面描述、台词、时长秒、运镜建议，JSON 数组返回：\n\n{{script}}', placeholder: '{{script}}' },
    narration: { name: '解说文案', body: '请你为以下视频撰写一段解说稿，画面感强、有节奏、可二次编辑，分段输出：\n\n视频：{{title}}\n风格：{{style}}', placeholder: '{{title}}\n{{style}}' },
    detect: { name: '口误检测', body: '请找出以下口播转写中的【口误/卡顿/重复/不流畅】，按 JSON 数组返回，含 issue_type / start / end / suggestion / text：\n\n{{transcript}}', placeholder: '{{transcript}}' },
    keywords: { name: '重点抽取', body: '请从以下文案中抽取 5-8 个值得在字幕中高亮/花字强调的关键词或短句，JSON 数组：\n\n{{script}}', placeholder: '{{script}}' },
  },
  promptEditing: 'script',
  other: {
    theme: '浅色', lang: '简体中文', hwAccel: true, exportResolution: '1920×1080', exportFormat: 'MP4 (H.264)',
    taskConcurrency: 2, autoSave: true, autoSaveSec: 30, cleanupDays: 30,
    ffmpegPath: '(自动检测)', tempDir: './data/tmp',
  },
};

export const initialCreation: CreationState = {
  reqFromSpoken: '',
  script: '', human: '', story: [],
  imgs: {}, frames: {}, voice: { ok: false }, subs: [],
  refs: {}, voices: [], humanPrompt: 'humanize', styleRef: '现实', refCat: {},
};

export function defaultSubs() {
  return [
    { t: '0:00', x: '大家好，今天聊一个新手也能上手的事' },
    { t: '0:05', x: '用 AI 把文案变成视频' },
    { t: '0:12', x: '自动写稿、分镜、配音一条龙' },
  ];
}

// 全局应用状态（对应原型中的全局可变变量 + 整体重渲染）
export interface AppState {
  module: ModuleKey;
  task: { label: string; p: number };
  filmCat: string;
  filmStage: 'library' | 'editor';
  editorSub: string;
  editingProj: { cat: string; t: string } | null;
  selectedClip: string | null;
  filmCats: FilmCat[];
  filmProjects: Record<string, FilmProject[]>;
  editorState: EditorState;
  spokenSel: string | null;
  spokenStage: string;
  spokenVideos: SpokenVideo[];
  cStage: string;
  cState: CreationState;
  settingsSub: string;
  settingsState: SettingsState;
}
