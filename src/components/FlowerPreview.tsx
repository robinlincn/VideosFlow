import type { CSSProperties } from 'react';
import { flowerTpls } from '../data/mock';
import type { FlowerTpl } from '../data/mock';

interface Props {
  selected: string;
  onPick: (id: string) => void;
}

/** ASS 颜色 &HAABBGGRR → CSS #RRGGBB。 */
function assColor(hex: string): string {
  const h = hex.replace(/&H/i, '').padStart(8, '0');
  const rr = h.slice(6, 8);
  const gg = h.slice(4, 6);
  const bb = h.slice(2, 4);
  return `#${rr}${gg}${bb}`;
}

const ALIGN_HINT: Record<number, string> = { 1: '左下', 2: '底部居中', 5: '画面正中' };

/**
 * 花字 6 模板预览 + 选模板（M2 固化，不支持自定义）。
 * 预览直接用 ASS assStyle 推导内联样式，保证 6 套都能正确呈现。
 */
export default function FlowerPreview({ selected, onPick }: Props) {
  return (
    <div className="flow-grid">
      {flowerTpls.map((t: FlowerTpl) => {
        const style: CSSProperties = {
          color: assColor(t.assStyle.PrimaryColour),
          background: t.assStyle.BorderStyle === 3 ? assColor(t.assStyle.BackColour) : 'transparent',
          fontWeight: t.assStyle.Bold ? 700 : 400,
          fontSize: Math.round(t.assStyle.FontSize * 0.7),
          padding: '4px 12px',
          borderRadius: 6,
          display: 'inline-block',
          marginBottom: 10,
          textShadow: t.assStyle.Shadow ? '0 1px 2px rgba(0,0,0,0.5)' : undefined,
          border: t.assStyle.BorderStyle === 1 ? `1px solid ${assColor(t.assStyle.BackColour)}` : undefined,
        };
        return (
          <div
            key={t.id}
            className={'flow-card ' + (selected === t.id ? 'active' : '')}
            onClick={() => onPick(t.id)}
          >
            <div className="pt">{t.name}</div>
            <span className="pp" style={style}>{t.demo}</span>
            <div className="ds">{t.desc} · {ALIGN_HINT[t.assStyle.Alignment] || '居中'}</div>
          </div>
        );
      })}
    </div>
  );
}
