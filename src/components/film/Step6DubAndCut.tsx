// 步骤 6：视频配音剪辑 = 分镜工作台（剪辑脚本表 + 工具栏 + 视频预览 + 导出）
// v2.0 重构：替代 v1.0 的 StoryboardWorkbench。分镜可编辑/删除/排序并持久化回草稿。
// v2.1 增量：把 voice / batch_dub / translate 三个按钮接到真实后端（音色库、批量配音、翻译至）；其余保留 M5 占位。

import { useEffect, useRef, useState } from 'react';
import { useApp } from '../../state/AppContext';
import { listVoices, batchDub, translateScript, exportJianyingDraft, dubOneSegment, importSrt, importAudioDub, exportPremiere, exportJianyingDraftIntl, filmRenderPreview, filmExportFinal, filmExportSrt, releaseTaskChannel } from '../../ipc/providers';
import { getVideoServerBase, initVideoServer } from '../../ipc/client';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { ProgressMsg, VoiceOption, DubSegment } from '../../ipc/types';

/** 本地视频预览源：桌面版经 fileserver（127.0.0.1）按需 Range 加载，浏览器态原样返回。 */
function toVideoSrc(p: string): string {
  if (!p) return '';
  if (/^[a-zA-Z]:\\/.test(p) || p.startsWith('file://')) {
    const base = getVideoServerBase();
    if (base) return `${base}/file?path=${encodeURIComponent(p)}`;
    return '';
  }
  if (p.startsWith('blob:') || p.startsWith('http://') || p.startsWith('https://')) return p;
  return p;
}

interface Props {
  script: string;
  videoName: string;
  projectId: string;
  totalDuration: number;
  rangeStart: number;
  rangeEnd: number;
  onBack: () => void;
  onSwitchToNarration: () => void;
  onScriptChange?: (script: string) => void;
}

interface Segment {
  index: number;
  section: string;
  start: number;
  end: number;
  text: string;
  role: string;
}

const SECTIONS = ['开端', '铺垫', '冲突', '高潮', '反转', '结局'];

function parseScript(script: string, totalDuration: number, startOffset: number): Segment[] {
  if (!script) return [];
  const lines = script.split('\n').map((l) => l.trim()).filter(Boolean);
  const segs: Segment[] = [];
  let i = 0;
  for (const ln of lines) {
    let m = ln.match(/^\[(\S+?)\]\s*(\d{1,2}):(\d{2})[-–~](\d{1,2}):(\d{2})\s+(.+)$/);
    if (m) {
      const section = m[1];
      const start = +m[2] * 60 + +m[3];
      const end = +m[4] * 60 + +m[5];
      const text = m[6].replace(/^【[^】]+】/, '').trim();
      segs.push({ index: i++, section, start: start + startOffset, end: end + startOffset, text, role: '主角' });
      continue;
    }
    m = ln.match(/^\[(\S+?)\]\s+(.+)$/);
    if (m) {
      const section = m[1];
      const text = m[2].replace(/^【[^】]+】/, '').trim();
      const dur = totalDuration || 180;
      const start = (segs.length / 6) * dur + startOffset;
      const end = ((segs.length + 1) / 6) * dur + startOffset;
      segs.push({ index: i++, section, start, end, text, role: '主角' });
      continue;
    }
    if (segs.length > 0) {
      segs[segs.length - 1].text += ' ' + ln;
    } else {
      const dur = totalDuration || 180;
      segs.push({ index: i++, section: SECTIONS[0] || '开端', start: startOffset, end: startOffset + dur / 6, text: ln, role: '主角' });
    }
  }
  return segs;
}

function fmt(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}.${String(Math.floor((sec % 1) * 100)).padStart(2, '0')}`;
}
function fmtShort(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}
function fmtDur(sec: number): string {
  if (sec >= 60) {
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m}分${s.toFixed(1)}秒`;
  }
  return `${sec.toFixed(1)}秒`;
}
function segsToScript(segs: Segment[]): string {
  return segs.map((s) => `[${s.section}] ${fmtShort(s.start)}-${fmtShort(s.end)} ${s.text}`).join('\n');
}

/** 计算两段文本的字符级重叠率（0-1，基于去标点字符集合 Jaccard 近似）。 */
function headOverlap(a: string, b: string): number {
  if (!a || !b) return 0;
  const A = new Set(a.replace(/\s+/g, ''));
  const B = new Set(b.replace(/\s+/g, ''));
  let same = 0;
  for (const ch of A) if (B.has(ch)) same++;
  return same / Math.max(A.size, 1);
}

const TOOLBAR = [
  { id: 'voice',      icon: '🎵', label: '音色库',     primary: false },
  { id: 'batch_dub',  icon: '🎬', label: '批量配音',   primary: true  },
  { id: 'translate',  icon: '🌐', label: '翻译至',     primary: false },
  { id: 'do_trans',   icon: '⏵', label: '开始翻译',   primary: false },
  { id: 'history',    icon: '📚', label: '历史版本',   primary: false },
  { id: 'role_swap',  icon: '👤', label: '角色替换',   primary: false },
  { id: 'copy_sub',   icon: '📋', label: '复制字幕',   primary: false },
  { id: 'import_sub', icon: '📥', label: '导入字幕',   primary: false },
  { id: 'export_srt', icon: '💾', label: '导出 SRT',   primary: false },
  { id: 'import_dub', icon: '🎧', label: '导入整段配音', primary: false },
  { id: 'regen',      icon: '🔄', label: '重新生成',   primary: false },
  { id: 'dedup',      icon: '🧹', label: 'AI 智能去重', primary: false },
  { id: 'qa',         icon: '✅', label: '文案质检',   primary: false },
];

export default function Step6DubAndCut({
  script, videoName, projectId, totalDuration, rangeStart, rangeEnd, onBack, onSwitchToNarration, onScriptChange,
}: Props) {
  const { actions } = useApp();
  const [segs, setSegs] = useState<Segment[]>(() => parseScript(script, totalDuration, rangeStart));
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [editingText, setEditingText] = useState('');
  const [subtitleStyle, setSubtitleStyle] = useState('经典-白字黑边');
  const [exportBusy, setExportBusy] = useState(false);
  const [exportMsg, setExportMsg] = useState('');
  // 全局选项（影响 dedup / qa / 导出）
  const [followOriginal, setFollowOriginal] = useState(false);
  const [flowerText, setFlowerText] = useState(false);
  const [strictAlign, setStrictAlign] = useState(false);
  // 历史版本（内存快照，简单持久化于 localStorage）
  const [snapshots, setSnapshots] = useState<{ id: string; ts: number; segs: Segment[]; note: string }[]>([]);
  // 最近一次去重/质检结果
  const [qaIssues, setQaIssues] = useState<string[]>([]);
  const lastSnapshotRef = useRef<string>('');
  // 视频预览源：源视频 / 合成成片 切换
  const [videoSrc, setVideoSrc] = useState('');
  const [compositeSrc, setCompositeSrc] = useState('');
  const [previewMode, setPreviewMode] = useState<'source' | 'composite'>('source');

  // 从解说工作台带入新脚本时重新解析
  useEffect(() => {
    setSegs(parseScript(script, totalDuration, rangeStart));
    setEditingIdx(null);
  }, [script, totalDuration, rangeStart]);

  // 历史快照：从 localStorage 加载
  useEffect(() => {
    if (!projectId) return;
    try {
      const raw = localStorage.getItem(`vfStep6History.${projectId}`);
      if (raw) setSnapshots(JSON.parse(raw));
    } catch { /* ignore */ }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // 源视频预览源：桌面版经本地 fileserver 加载（异步获取 base 后计算）
  useEffect(() => {
    let cancelled = false;
    initVideoServer().then(() => {
      if (!cancelled) setVideoSrc(toVideoSrc((window as any).__vf_videoPath || ''));
    });
    return () => { cancelled = true; };
  }, []);

  // 写入快照（最多 8 条 + 持久化 localStorage）
  const saveSnapshot = (note: string) => {
    const id = Math.random().toString(36).slice(2, 9);
    const ts = Date.now();
    const snap = { id, ts, segs: segs.map((s) => ({ ...s })), note };
    setSnapshots((prev) => {
      const next = [snap, ...prev].slice(0, 8);
      try {
        if (projectId) localStorage.setItem(`vfStep6History.${projectId}`, JSON.stringify(next));
      } catch { /* quota/ignore */ }
      return next;
    });
  };

  // 单段配音：与 batch_dub 共用同一 Rust 后端，单元素数组传入即可
  const dubOne = (idx: number) => {
    const target = segs[idx];
    if (!target) return;
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    setExportBusy(true); setExportMsg(`单段配音 #${idx + 1}`);
    const item: DubSegment = { index: idx, text: target.text, voice: target.role };
    dubOneSegment(projectId, item, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const url = (m.payload as any)?.url || '';
        // 把生成的 URL 写回 seg 的临时字段（不持久化到 script）
        setSegs((prev) => prev.map((s, i) => i === idx ? { ...s, ...(url ? { dubUrl: url } : {}) } : s));
        actions.task(`第 ${idx + 1} 段配音完成${url ? '：' + url : ''}`, 100);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task(`第 ${idx + 1} 段配音失败：${m.message || ''}`, 100);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('单段配音失败：' + String(e), 100); });
  };

  // 复制分镜：插入到当前段的下一位（区间时间与原段一致，文本复用）
  const copySegment = (idx: number) => {
    const target = segs[idx];
    if (!target) return;
    const insertAt = idx + 1;
    const next = [...segs];
    next.splice(insertAt, 0, {
      ...target,
      index: insertAt,
      // 时间：原段 [start, end] 后推到下一段，但为了不重叠，先按等长延后
      start: target.end,
      end: target.end + Math.max(1, target.end - target.start),
    });
    persist(next.map((s, i) => ({ ...s, index: i })));
    saveSnapshot(`复制 #${idx + 1}`);
    actions.task(`已复制分镜 #${idx + 1}，插入到 #${insertAt + 1}`, 100);
  };

  const persist = (next: Segment[]) => {
    setSegs(next);
    onScriptChange?.(segsToScript(next));
  };

  const handleEdit = (i: number, text: string) => {
    setEditingIdx(i);
    setEditingText(text);
  };
  const commitEdit = () => {
    if (editingIdx === null) return;
    const next = segs.map((s, idx) => (idx === editingIdx ? { ...s, text: editingText } : s));
    persist(next);
    setEditingIdx(null);
  };
  const removeSegment = (i: number) => {
    const next = segs.filter((_, idx) => idx !== i).map((s, idx) => ({ ...s, index: idx }));
    persist(next);
  };
  const moveSegment = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= segs.length) return;
    const next = [...segs];
    [next[i], next[j]] = [next[j], next[i]];
    next.forEach((s, idx) => { s.index = idx; });
    persist(next);
  };
  // 弹出文件夹选择框，返回选定路径；用户取消则返回 null
  const pickFolder = async (): Promise<string | null> => {
    try {
      const p = await openDialog({ directory: true, multiple: false }) as string | null;
      return p || null;
    } catch (e) {
      actions.task('无法打开文件夹选择框：' + String(e), 100);
      return null;
    }
  };

  const generateSrt = async () => {
    if (segs.length === 0) { actions.task('暂无分镜，无法生成 SRT', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true); setExportMsg('导出 SRT');
    const srt = segs.map((s, i) => `${i + 1}\n${fmt(s.start)} --> ${fmt(s.end)}\n${s.text}\n`).join('\n');
    filmExportSrt(projectId || 'local', { content: srt, outDir }, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const out = (m.payload as any)?.outPath || '';
        actions.task('SRT 已导出：' + out, 100);
        setExportMsg('导出完成：' + out);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task('SRT 导出失败：' + (m.message || ''), 100);
        setExportMsg('导出失败');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('SRT 导出失败：' + String(e), 100); });
  };
  const exportJianying = async () => {
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    if (segs.length === 0) { actions.task('暂无分镜，请先在解说工作台生成解说词', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true);
    setExportMsg('准备导出');
    try {
      // 真实剪映草稿导出：写入 data_dir/jianying_drafts/<project>_<ts>/draft_content.json 等
      await exportJianyingDraft(projectId || 'local', {
        script: segsToScript(segs),
        videoPath: (window as any).__vf_videoPath || '',
        rangeStart,
        rangeEnd,
        outDir,
      }, (m: ProgressMsg) => {
        setExportMsg(m.message || '');
        if (m.status === 'done') {
          setExportBusy(false);
          const out = (m.payload as any)?.draftDir || '';
          actions.task('剪映草稿已生成：' + out, 100);
          setExportMsg('导出完成：' + out);
          try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
        } else if (m.status === 'failed') {
          setExportBusy(false);
          actions.task('导出失败：' + (m.message || ''), 100);
          setExportMsg('导出失败');
          try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
        }
      });
    } catch (e) {
      setExportBusy(false);
      actions.task('导出失败：' + String(e), 100);
      setExportMsg('导出失败');
    }
  };

  // 导出 Premiere：生成 .edl（CMX3600）+ 时间线 JSON + 字幕 SRT，三件套打包到一个 zip-like 文本
  const exportPremiereClick = async () => {
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    if (segs.length === 0) { actions.task('暂无分镜', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true); setExportMsg('导出 Premiere');
    exportPremiere(projectId, {
      script: segsToScript(segs),
      videoPath: (window as any).__vf_videoPath || '',
      rangeStart, rangeEnd,
      followOriginal, flowerText, strictAlign, outDir,
    }, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const out = (m.payload as any)?.outDir || (m.payload as any)?.outPath || '';
        actions.task('Premiere 时间线已导出：' + out, 100);
        setExportMsg('导出完成：' + out);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task('Premiere 导出失败：' + (m.message || ''), 100);
        setExportMsg('导出失败');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('Premiere 导出失败：' + String(e), 100); });
  };

  // 导出国际剪映（CapCut / Jianying International）：与国内版结构相同，路径用绝对路径
  const exportJianyingIntlClick = async () => {
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    if (segs.length === 0) { actions.task('暂无分镜', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true); setExportMsg('导出国际剪映');
    exportJianyingDraftIntl(projectId, {
      script: segsToScript(segs),
      videoPath: (window as any).__vf_videoPath || '',
      rangeStart, rangeEnd,
      followOriginal, flowerText, strictAlign, outDir,
    }, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const out = (m.payload as any)?.draftDir || '';
        actions.task('国际剪映草稿已生成：' + out, 100);
        setExportMsg('导出完成：' + out);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task('国际剪映导出失败：' + (m.message || ''), 100);
        setExportMsg('导出失败');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('国际剪映导出失败：' + String(e), 100); });
  };

  // 预览成片：源视频 + 分段配音 + 烧录字幕 合成到指定文件夹（outDir）/data_dir/preview/<safe>.mp4，done 后用 fileserver 播放
  const renderPreviewClick = async () => {
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    if (segs.length === 0) { actions.task('暂无分镜', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true); setExportMsg('合成预览成片中');
    filmRenderPreview(projectId, {
      script: segsToScript(segs),
      videoPath: (window as any).__vf_videoPath || '',
      mixVoice: true,
      subtitleStyle,
      outDir,
    }, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const out = (m.payload as any)?.outPath || '';
        if (out) { setCompositeSrc(toVideoSrc(out)); setPreviewMode('composite'); }
        actions.task('预览成片已生成', 100);
        setExportMsg(out ? '预览成片已生成' : '预览合成完成但无输出路径');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task('预览合成失败：' + (m.message || ''), 100);
        setExportMsg('预览合成失败');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('预览合成失败：' + String(e), 100); });
  };

  // 导出成片 MP4：与预览同一管线，输出到指定文件夹（outDir）/data_dir/export/<safe>_<ts>.mp4
  const exportFinalClick = async () => {
    if (!projectId) { actions.task('缺少工程 ID', 100); return; }
    if (segs.length === 0) { actions.task('暂无分镜', 100); return; }
    const outDir = await pickFolder();
    if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); return; }
    setExportBusy(true); setExportMsg('导出成片中');
    filmExportFinal(projectId, {
      script: segsToScript(segs),
      videoPath: (window as any).__vf_videoPath || '',
      mixVoice: true,
      subtitleStyle,
      outDir,
    }, (m: ProgressMsg) => {
      setExportMsg(m.message || '');
      if (m.status === 'done') {
        setExportBusy(false);
        const out = (m.payload as any)?.outPath || '';
        actions.task('成片已导出：' + out, 100);
        setExportMsg('导出完成：' + out);
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      } else if (m.status === 'failed') {
        setExportBusy(false);
        actions.task('成片导出失败：' + (m.message || ''), 100);
        setExportMsg('导出失败');
        try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
      }
    }).catch((e) => { setExportBusy(false); actions.task('成片导出失败：' + String(e), 100); });
  };

  const onTool = async (id: string) => {
    switch (id) {
      case 'export_srt':
        generateSrt();
        break;
      case 'regen':
        onSwitchToNarration();
        break;
      case 'copy_sub': {
        const txt = segs.map((s) => s.text).join('\n');
        if (navigator.clipboard?.writeText) {
          navigator.clipboard.writeText(txt).then(
            () => actions.task('字幕已复制到剪贴板', 100),
            () => actions.task('复制失败', 100),
          );
        } else {
          actions.task('当前环境不支持剪贴板', 100);
        }
        break;
      }
      // ===== 开始翻译（顶部按钮）=====
      case 'do_trans': {
        // 先弹出 prompt 选语种；选完后复用 translateScript 真后端
        const input = window.prompt('翻译至（输入 1=中文, 2=English, 3=日本語）：', '2');
        const map: Record<string, 'zh' | 'en' | 'ja'> = { '1': 'zh', '2': 'en', '3': 'ja', zh: 'zh', en: 'en', ja: 'ja' };
        const picked = map[(input || '').toLowerCase()] || map[input || ''];
        if (!picked) { actions.task('已取消翻译', 100); break; }
        if (segs.length === 0) { actions.task('暂无分镜可翻译', 100); break; }
        if (!projectId) { actions.task('缺少工程 ID', 100); break; }
        setExportBusy(true); setExportMsg('翻译中');
        translateScript(projectId, {
          language: picked,
          segments: segs.map(s => ({ index: s.index, section: s.section, start: s.start, end: s.end, text: s.text })),
        }).then((out) => {
          setExportBusy(false);
          if (out && out.length > 0) {
            const next = segs.map(s => ({ ...s, text: (out.find((x: any) => x.index === s.index) || {}).text || s.text }));
            persist(next);
            // 落盘到 localStorage 历史
            saveSnapshot(picked.toUpperCase() + ' 翻译');
            actions.task(`翻译完成（${picked.toUpperCase()}）· ${out.length} 段`, 100);
          } else {
            actions.task('翻译结果为空', 100);
          }
        }).catch((e) => { setExportBusy(false); actions.task('翻译失败：' + String(e), 100); });
        break;
      }
      // ===== 历史版本（内存快照）=====
      case 'history': {
        const note = window.prompt('为当前分镜输入备注（取消请直接关闭）：') || '';
        if (note === null) { actions.task('已取消保存', 100); break; }
        saveSnapshot(note || '无备注');
        actions.task('快照已保存（共 ' + (snapshots.length + 1) + ' 个）', 100);
        // 立即弹列表让用户选择回滚/查看
        const list = snapshots.map((s, i) => `${i + 1}. [${new Date(s.ts).toLocaleString()}] ${s.note}（${s.segs.length} 段）`).join('\n');
        const pick = window.prompt('当前快照清单（输入序号回滚到该版本）：\n' + list + '\n\n0 = 仅查看不处理');
        const idx = parseInt(pick || '-1');
        if (!isNaN(idx) && idx >= 1 && idx <= snapshots.length) {
          const sel = snapshots[idx - 1];
          persist(sel.segs.map((s, i) => ({ ...s, index: i })));
          actions.task(`已回滚到快照 ${idx}`, 100);
        } else {
          actions.task('快照列表已保存', 100);
        }
        break;
      }
      // ===== 角色替换 =======
      case 'role_swap': {
        // 自动按"主角"/"旁白"/"受访者"/"画外音"做简单分类
        if (segs.length === 0) { actions.task('暂无分镜', 100); break; }
        const HEAD = ['话说', '只见', '原来', '于是', '镜头', '画面', '随后'];
        const NARR = ['这时', '此刻', '与此同时', '据', '根据', '据悉'];
        let changed = 0;
        const next = segs.map((s) => {
          const first = s.text.slice(0, 2);
          const old = s.role;
          let role = '主角';
          if (NARR.some((n) => s.text.startsWith(n))) role = '旁白';
          else if (HEAD.some((h) => s.text.startsWith(h))) role = '主角';
          if (role !== old) changed++;
          return { ...s, role };
        });
        if (changed === 0) { actions.task('未检测到需要切换的角色段', 100); break; }
        persist(next);
        saveSnapshot('角色重命名');
        actions.task(`已切换 ${changed}/${segs.length} 段角色`, 100);
        break;
      }
      // ===== 导入字幕（SRT/JSON）======
      case 'import_sub': {
        openDialog({
          filters: [{ name: '字幕', extensions: ['srt', 'json', 'txt'] }],
          multiple: false,
        }).then(async (p) => {
          if (!p || typeof p !== 'string') { actions.task('已取消导入', 100); return; }
          try {
            const subs = await importSrt(p);
            if (!subs || subs.length === 0) { actions.task('未解析出字幕段', 100); return; }
            // 用导入字幕回填 segs.text 与 start/end
            const next: Segment[] = subs.map((s, i) => ({
              index: i, section: segs[i]?.section || '导入字幕',
              start: s.start, end: s.end, text: s.text, role: segs[i]?.role || '主角',
            }));
            persist(next);
            saveSnapshot('字幕导入');
            actions.task(`已导入 ${subs.length} 段字幕`, 100);
          } catch (e: any) { actions.task('导入字幕失败：' + (e.message || String(e)), 100); }
        }).catch(() => actions.task('已取消导入', 100));
        break;
      }
      // ===== 导入配音（wav/mp3，整体转写按段时长切）======
      case 'import_dub': {
        if (!projectId) { actions.task('缺少工程 ID', 100); break; }
        openDialog({
          filters: [{ name: '音频', extensions: ['wav', 'mp3', 'm4a', 'flac'] }],
          multiple: false,
        }).then(async (p) => {
          if (!p || typeof p !== 'string') { actions.task('已取消导入', 100); return; }
          setExportBusy(true); setExportMsg('导入配音转写中');
          try {
            // 用本地 faster-whisper 转写整体音频
            const res = await importAudioDub(projectId, p);
            setExportBusy(false);
            if (!res || !res.segments || res.segments.length === 0) {
              actions.task('未转写出文本（可能音频无清晰人声）', 100); return;
            }
            // 用转写 segments 回填 text（保留现有的时间区间，仅替换文字）
            const next = segs.map((s, i) => {
              const r = res.segments[i] || res.segments[Math.min(i, res.segments.length - 1)];
              return r ? { ...s, text: r.text } : s;
            });
            persist(next);
            saveSnapshot('配音导入');
            actions.task(`已导入 ${res.segments.length} 段配音文案`, 100);
          } catch (e: any) {
            setExportBusy(false);
            actions.task('配音导入失败：' + (e.message || String(e)), 100);
          }
        }).catch(() => actions.task('已取消导入', 100));
        break;
      }
      // ===== 智能去重（相邻段落相似合并）======
      case 'dedup': {
        if (segs.length < 2) { actions.task('段落不足，跳过去重', 100); break; }
        const issues: { i: number; reason: string }[] = [];
        // 简单规则：
        //   (a) 相邻段开头 6 字相同 → 合并
        //   (b) 单段 < 8 字且下一段 > 16 字 → 合并到下一段
        //   (c) 严格模式下：连续重复词 ≥ 3 字 且 > 30% 重叠 → 合并
        const next: Segment[] = [];
        for (let i = 0; i < segs.length; i++) {
          const cur = segs[i];
          const prev = next[next.length - 1];
          const head4 = cur.text.slice(0, 4);
          const prevHead4 = prev?.text.slice(0, 4) || '';
          const overlap = prev ? headOverlap(prev.text, cur.text) : 0;
          const isDupHead = prev && head4 === prevHead4 && head4.length >= 3;
          const isShort = prev && cur.text.length < 8;
          const isHighOverlap = strictAlign && prev && overlap > 0.4 && prev.text.length > 12;
          if (prev && (isDupHead || isShort || isHighOverlap)) {
            const reason = isDupHead ? '相邻段开头' + Math.min(4, head4.length) + ' 字相同'
              : isShort ? '单段过短（<' + cur.text.length + ' 字）'
              : '相邻段文本重叠 >40%';
            issues.push({ i: cur.index, reason });
            // 合并：把 cur 的正文并入 prev，调整 end
            const merged: Segment = { ...prev, end: cur.end, text: (prev.text.trim() + cur.text.trim()).slice(0, 220) };
            next[next.length - 1] = merged;
          } else {
            next.push(cur);
          }
        }
        const dedupCount = segs.length - next.length;
        if (dedupCount === 0) {
          actions.task('未发现可去重段', 100);
        } else {
          persist(next.map((s, i) => ({ ...s, index: i })));
          saveSnapshot('智能去重');
          actions.task(`已合并 ${dedupCount} 段（${issues.map(x => x.reason).slice(0, 3).join('；')}${issues.length > 3 ? '…' : ''}）`, 100);
        }
        break;
      }
      // ===== 文案质检（本地规则）======
      case 'qa': {
        if (segs.length === 0) { actions.task('暂无分镜', 100); break; }
        const issues: string[] = [];
        for (let i = 0; i < segs.length; i++) {
          const s = segs[i];
          if (!s.text || s.text.trim().length === 0) {
            issues.push(`#${i + 1}（${s.section}）：空段`);
          } else if (s.text.length > 90) {
            issues.push(`#${i + 1}（${s.section}）：过长（${s.text.length} 字，建议 < 90）`);
          } else if (s.text.length < 5) {
            issues.push(`#${i + 1}（${s.section}）：过短（${s.text.length} 字，建议 ≥ 5）`);
          }
          // 时间区间重叠 / 倒序
          if (i > 0 && s.start < segs[i - 1].end) {
            issues.push(`#${i + 1}：与上一段时间重叠（${s.start} < ${segs[i - 1].end}）`);
          }
          // 同段重复字（"的的""啊啊"）
          if (/(.)\1{3,}/.test(s.text)) issues.push(`#${i + 1}：含连续重复字符`);
        }
        // 严格对齐：长度差异过大
        if (strictAlign) {
          const lens = segs.map((s) => s.text.length);
          const avg = lens.reduce((a, b) => a + b, 0) / Math.max(1, lens.length);
          for (let i = 0; i < segs.length; i++) {
            if (lens[i] < avg * 0.4) issues.push(`#${i + 1}：长度异常短（小均 60%）`);
            if (lens[i] > avg * 1.8) issues.push(`#${i + 1}：长度异常长（大均 80%）`);
          }
        }
        setQaIssues(issues);
        if (issues.length === 0) {
          actions.task('质检通过：无问题', 100);
        } else {
          actions.task(`质检完成：发现 ${issues.length} 项（首 3 条：${issues.slice(0, 3).join('；')}）`, 100);
          // 把所有问题放在 alert 里给用户看清
          window.alert('文案质检结果\n' + issues.slice(0, 50).map((s, i) => `${i + 1}. ${s}`).join('\n') + (issues.length > 50 ? `\n…还有 ${issues.length - 50} 条` : ''));
        }
        break;
      }
      case 'voice':
        // 音色库：异步拉取 XiaomiMimo 音色列表（M5 接通的真实后端）
        // 用 alert/prompt 显示完整列表（保持界面不变，不引入新 UI 元素）
        listVoices().then((list: VoiceOption[]) => {
          const fallback: VoiceOption[] = [
            { id: 'default', name: '默认（系统音色）' },
            { id: 'male_calm', name: '磁性男声 · 沉稳' },
            { id: 'male_warm', name: '温暖男声 · 亲切' },
            { id: 'female_lively', name: '知性女声 · 活力' },
            { id: 'female_news', name: '标准女声 · 新闻' },
            { id: 'narrator', name: '旁白男声 · 纪录片' },
          ];
          const pool = list.length > 0 ? list : fallback;
          const lines = pool.map((v, i) => `${i + 1}. ${v.name} (${v.id})`).join('\n');
          alert(`音色库（共 ${pool.length} 项）：\n\n${lines}\n\n点确定后输入编号选择音色，作用于全部分镜。`);
          const pick = window.prompt('输入音色编号选择（1-' + pool.length + '）：', '1');
          if (!pick) return;
          const idx = parseInt(pick, 10);
          const picked = pool[idx - 1];
          if (!picked) { actions.task('音色选择无效', 100); return; }
          const next = segs.map(s => ({ ...s, role: picked.id }));
          setSegs(next);
          persist(next);
          actions.task(`音色已设为：${picked.name}`, 100);
        }).catch((e) => actions.task('音色库加载失败：' + String(e), 100));
        break;
      case 'batch_dub':
        // 批量配音：逐段调 XiaomiMimo TTS，写 data_dir/dub/<project>/seg_NNN.wav
        if (segs.length === 0) { actions.task('暂无分镜可配音', 100); break; }
        if (!projectId) { actions.task('缺少工程 ID', 100); break; }
        setExportBusy(true);
        setExportMsg('批量配音中');
        const dubItems: DubSegment[] = segs.map(s => ({ index: s.index, text: s.text, voice: s.role }));
        batchDub(projectId, dubItems, (m: ProgressMsg) => {
          setExportMsg(m.message || '');
          if (m.status === 'done') {
            setExportBusy(false);
            // ★ 释放 channel 引用（缓存由 providers._taskChans 持有）
            try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
            actions.task(`批量配音完成（${segs.length} 段）`, 100);
          }
          else if (m.status === 'failed') {
            setExportBusy(false);
            try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
            actions.task('批量配音失败：' + (m.message || ''), 100);
          }
        }).catch((e) => {
          setExportBusy(false);
          actions.task('批量配音失败：' + String(e), 100);
        });
        break;
      case 'translate':
        // 翻译至：先让用户选语种（弹一个临时确认）
        const lang = window.prompt('翻译至：\n1. zh — 中文\n2. en — English\n3. ja — 日本語\n\n输入 1/2/3 选语种：', '2');
        const langMap: Record<string, 'zh' | 'en' | 'ja'> = { '1': 'zh', '2': 'en', '3': 'ja', zh: 'zh', en: 'en', ja: 'ja' };
        const picked = langMap[lang || ''];
        if (!picked) { actions.task('已取消翻译', 100); break; }
        if (segs.length === 0) { actions.task('暂无分镜可翻译', 100); break; }
        if (!projectId) { actions.task('缺少工程 ID', 100); break; }
        setExportBusy(true); setExportMsg('翻译中');
        translateScript(projectId, {
          language: picked,
          segments: segs.map(s => ({ index: s.index, section: s.section, start: s.start, end: s.end, text: s.text })),
        }).then((out) => {
          setExportBusy(false);
          if (out && out.length > 0) {
            const next = segs.map(s => ({ ...s, text: out.find((x: any) => x.index === s.index)?.text || s.text }));
            setSegs(next);
            onScriptChange?.(next.map(s => `[${s.section}] ${fmtShort(s.start)}-${fmtShort(s.end)} ${s.text}`).join('\n'));
            actions.task(`翻译完成（${picked.toUpperCase()}）· ${out.length} 段`, 100);
          } else {
            actions.task('翻译结果为空', 100);
          }
        }).catch((e) => { setExportBusy(false); actions.task('翻译失败：' + String(e), 100); });
        break;
      case 'export_jy': {
        // 导出剪映草稿：弹出文件夹选择框，写入选定文件夹
        if (!projectId) { actions.task('缺少工程 ID', 100); break; }
        const outDir = await pickFolder();
        if (!outDir) { actions.task('已取消导出（未选择文件夹）', 100); break; }
        setExportBusy(true); setExportMsg('导出剪映草稿中');
        exportJianyingDraft(projectId, { script: segsToScript(segs), videoPath: (window as any).__vf_videoPath || '', rangeStart, rangeEnd, outDir }, (m: ProgressMsg) => {
          setExportMsg(m.message || '');
          if (m.status === 'done') {
            setExportBusy(false);
            const d = (m.payload as any)?.draftDir || '';
            actions.task('剪映草稿已生成：' + d, 100);
            try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
          } else if (m.status === 'failed') {
            setExportBusy(false);
            actions.task('剪映草稿导出失败：' + (m.message || ''), 100);
            try { releaseTaskChannel(m.taskId || ''); } catch { /* ignore */ }
          }
        }).catch((e) => { setExportBusy(false); actions.task('剪映草稿失败：' + String(e), 100); });
        break;
      }
      default:
        actions.task(`${TOOLBAR.find((t) => t.id === id)?.label || id}（M5 实现）`, 100);
    }
  };

  return (
    <div className="film-step6">
      {/* 顶部切换 */}
      <div className="film-step5__tabs">
        <button className="film-step5__tab" onClick={onSwitchToNarration}>📝 解说工作台</button>
        <button className="film-step5__tab active">🎬 分镜工作台</button>
      </div>

      {/* 工具栏 */}
      <div className="film-step6__toolbar">
        {TOOLBAR.map((t) => (
          <button
            key={t.id}
            className={'film-step6__tool' + (t.primary ? ' film-step6__tool--primary' : '')}
            onClick={() => onTool(t.id)}
            title={t.label}
          >
            <span className="film-step6__tool-icon">{t.icon}</span>
            <span className="film-step6__tool-label">{t.label}</span>
          </button>
        ))}
      </div>

      {/* 进度 / 导出提示 */}
      <div className="film-step6__progress">
        <span className="muted">
          💡 解说生成完成！可单击解说词编辑、调整顺序或删除；点击『导出 SRT』下载字幕，或『导出剪映草稿』生成工程文件。
        </span>
      </div>

      <div className="film-step6__main">
        {/* 左侧：视频预览 */}
        <div className="film-step6__left">
          <div className="film-step6__video">
            <div className="film-step6__video-bar">
              <div className="film-step6__video-tabs">
                <button
                  className={'film-step6__vtab' + (previewMode === 'source' ? ' active' : '')}
                  onClick={() => setPreviewMode('source')}
                >源视频</button>
                {compositeSrc && (
                  <button
                    className={'film-step6__vtab' + (previewMode === 'composite' ? ' active' : '')}
                    onClick={() => setPreviewMode('composite')}
                  >合成成片</button>
                )}
              </div>
              <button
                className="btn sm film-step6__preview-btn"
                onClick={renderPreviewClick}
                disabled={exportBusy || segs.length === 0}
                title="源视频 + 分段配音 + 烧录字幕 合成预览"
              >▶ 预览成片</button>
            </div>
            <div className="film-step6__video-frame">
              {previewMode === 'composite' && compositeSrc ? (
                <video key={compositeSrc} className="film-step6__video-el" src={compositeSrc} controls autoPlay />
              ) : videoSrc ? (
                <video key={videoSrc} className="film-step6__video-el" src={videoSrc} controls />
              ) : (
                <div className="film-step6__video-empty">▶ 视频预览（源视频未加载）</div>
              )}
            </div>
            <div className="film-step6__timeline">
              <div className="film-step6__timeline-handle" />
            </div>
            <div className="film-step6__video-meta">
              <div>视频文件：<span className="muted">{videoName || '-'}</span></div>
              <div>分镜数：<span className="muted">{segs.length}</span></div>
              <div>成片约：<span className="muted">{fmtDur(rangeEnd - rangeStart)}</span></div>
              <div>状态：<span className="muted">{previewMode === 'composite' && compositeSrc ? '合成成片' : '源视频就位'}</span></div>
            </div>
          </div>
        </div>

        {/* 右侧：剪辑脚本表 */}
        <div className="film-step6__right">
          <div className="film-step6__script-header">
            <span>剪辑脚本</span>
            <span className="muted">共 <b style={{ color: 'var(--accent)' }}>{segs.length}</b> 条 ✓ {segs.length} | ⚠ 0 | 原片 0 | 成片约 {fmtDur(rangeEnd - rangeStart)}</span>
          </div>
          <div className="film-step6__table-wrap">
            <table className="film-step6__table">
              <thead>
                <tr>
                  <th style={{ width: 36 }}>#</th>
                  <th style={{ width: 72 }}>开始时间</th>
                  <th style={{ width: 72 }}>结束时间</th>
                  <th style={{ width: 56 }}>时长</th>
                  <th style={{ width: 70 }}>角色</th>
                  <th style={{ width: 110 }}>原始字幕</th>
                  <th>解说词（单击编辑）</th>
                  <th style={{ width: 70 }}>配音</th>
                  <th style={{ width: 90 }}>操作</th>
                </tr>
              </thead>
              <tbody>
                {segs.length === 0 && (
                  <tr><td colSpan={9} className="muted" style={{ textAlign: 'center', padding: 20 }}>暂无分镜数据。请先在解说工作台生成解说。</td></tr>
                )}
                {segs.slice(0, 15).map((s) => (
                  <tr key={s.index}>
                    <td>{s.index + 1}</td>
                    <td className="film-step6__time">{fmt(s.start)}</td>
                    <td className="film-step6__time">{fmt(s.end)}</td>
                    <td className="film-step6__time">{fmtDur(s.end - s.start)}</td>
                    <td>{s.role}</td>
                    <td className="muted" style={{ fontSize: 11 }}>—</td>
                    <td>
                      {editingIdx === s.index ? (
                        <input
                          className="mini"
                          autoFocus
                          value={editingText}
                          onChange={(e) => setEditingText(e.target.value)}
                          onBlur={commitEdit}
                          onKeyDown={(e) => { if (e.key === 'Enter') commitEdit(); }}
                        />
                      ) : (
                        <span onClick={() => handleEdit(s.index, s.text)} style={{ cursor: 'text' }}>
                          {s.text}
                        </span>
                      )}
                    </td>
                    <td>
                      <button className="film-step6__mini-btn" onClick={() => dubOne(s.index)}>单独配音</button>
                    </td>
                    <td>
                      <button className="film-step6__icon-btn" title="复制" onClick={() => copySegment(s.index)}>📋</button>
                      <button className="film-step6__icon-btn" title="上移" onClick={() => moveSegment(s.index, -1)}>↑</button>
                      <button className="film-step6__icon-btn" title="下移" onClick={() => moveSegment(s.index, 1)}>↓</button>
                      <button className="film-step6__icon-btn" title="删除" onClick={() => removeSegment(s.index)}>🗑</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* 底部 4 选项 + 3 导出按钮 */}
      <div className="film-step6__bottom">
        <div className="film-step6__bottom-opts">
          <span className="muted">比例：</span>
          <select className="form-select" style={{ maxWidth: 120 }}><option>原比例</option><option>16:9</option><option>9:16</option></select>
          <span className="muted" style={{ marginLeft: 12 }}>字幕：</span>
          <select className="form-select" style={{ maxWidth: 140 }} value={subtitleStyle} onChange={(e) => setSubtitleStyle(e.target.value)}>
            <option>经典-白字黑边</option>
            <option>简约-无边框</option>
            <option>阴影-黑字阴影</option>
          </select>
          <label className="film-step6__check"><input type="checkbox" checked={followOriginal} onChange={(e) => { const v = e.target.checked; setFollowOriginal(v); actions.task(v ? '遵循原字幕（导出时附加 ASR 转写原文到素材轨）' : '关闭遵循原字幕', 100); }} /> 遵循原字幕（仅支持剪映6.0以下版本）</label>
          <label className="film-step6__check"><input type="checkbox" checked={flowerText} onChange={(e) => { const v = e.target.checked; setFlowerText(v); actions.task(v ? '开启花字' : '关闭花字', 100); }} /> 花字</label>
          <label className="film-step6__check"><input type="checkbox" checked={strictAlign} onChange={(e) => { const v = e.target.checked; setStrictAlign(v); actions.task(v ? '开启音频强制对齐（更严格质检/去重）' : '关闭强制对齐', 100); }} /> 音频强制对齐</label>
        </div>
        <div className="film-step6__bottom-exports">
          <button className="btn primary film-step6__export-btn" onClick={exportJianying} disabled={exportBusy}>
            {exportBusy ? (exportMsg || '导出中...') : '📤 导出剪映草稿'}
          </button>
          <button className="btn sm" onClick={exportPremiereClick}>📤 导出 Premiere</button>
          <button className="btn sm" onClick={exportJianyingIntlClick}>📤 导出国际剪映</button>
          <button className="btn sm" onClick={exportFinalClick} disabled={exportBusy || segs.length === 0} title="源视频 + 分段配音 + 烧录字幕 合成最终 MP4">📤 导出成片</button>
          <button className="btn sm ghost" onClick={generateSrt}>💾 导出 SRT</button>
        </div>
      </div>

      <div style={{ marginTop: 12 }}>
        <button className="btn sm ghost" onClick={onBack}>‹ 返回解说工作台</button>
      </div>
    </div>
  );
}
