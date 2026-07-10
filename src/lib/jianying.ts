// 剪映工程导出辅助：把构造好的草稿对象下载为 draft_content.json
// 真实 App 中由 Rust 端生成完整草稿文件夹（draft_content.json + draft_meta_info.json + materials/）并拷贝素材。

export function downloadJson(filename: string, obj: unknown) {
  const blob = new Blob([JSON.stringify(obj, null, 2)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  a.click();
  URL.revokeObjectURL(a.href);
}

export function toSec(s: string): number {
  const p = String(s).split(':');
  return p.length === 2 ? +p[0] * 60 + +p[1] : +p[0];
}
