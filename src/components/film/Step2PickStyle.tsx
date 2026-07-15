// 步骤 2：选择视频类型（9 个 Editorial 大地色卡片）
// v2.0 重构：替代 v1.0 的 5 个影片类型侧栏
// 配色统一采用 Editorial 设计系统的低饱和大地色（赭石 / 茶绿 / 陶土 / 苔绿…），
// 通过卡片顶部 4px 强调色条区分类型，保持与整体系统风格一致。

interface Props {
  onPick: (styleId: string, styleName: string) => void;
  onBack: () => void;
}

const STYLES = [
  { id: 'movie',        name: '电影解说',   desc: '抖音爆款电影·解说风格',         accent: '#b85c38' },
  { id: 'series',       name: '电视剧解说', desc: '追剧向电视剧解说',               accent: '#9c4828' },
  { id: 'variety',      name: '综艺解说',   desc: '综艺节目精彩片段解说',           accent: '#6b7a4b' },
  { id: 'anime',        name: '动漫解说',   desc: '番剧/动漫解说',                 accent: '#5b8a8a' },
  { id: 'shortdrama',   name: '短剧解说',   desc: '竖屏短剧解说',                   accent: '#c0883e' },
  { id: 'sports',       name: '体育解说',   desc: '体育赛事精彩解说',               accent: '#7a6b3b' },
  { id: 'documentary',  name: '纪录片解说', desc: '纪录片/科普解说',                 accent: '#566b4b' },
  { id: 'general',      name: '通用解说',   desc: '通用视频解说',                   accent: '#8a7a64' },
  { id: '__more__',     name: '更多风格',   desc: '敬请期待...',                     accent: '#a89a86' },
];

export default function Step2PickStyle({ onPick, onBack }: Props) {
  return (
    <div className="film-step2">
      <div className="film-step2__top">
        <button className="film-step2__back" onClick={onBack}>‹ 返回</button>
      </div>
      <div className="film-step2__header">
        <h1>🎬 选择视频类型</h1>
        <div className="film-step2__subtitle">选择你的视频类型，AI 将使用对应的风格生成解说词</div>
      </div>
      <div className="film-step2__grid">
        {STYLES.map((s) => (
          <div
            key={s.id}
            className={'film-step2-card' + (s.id === '__more__' ? ' film-step2-card--more' : '')}
            style={{ borderTopColor: s.accent }}
            onClick={() => onPick(s.id, s.name)}
          >
            <div className="film-step2-card__name">{s.name}</div>
            <div className="film-step2-card__desc">{s.desc}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
