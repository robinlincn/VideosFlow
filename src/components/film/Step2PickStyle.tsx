// 步骤 2：选择视频类型（9 个渐变彩色卡片）
// v2.0 重构：替代 v1.0 的 5 个影片类型侧栏

interface Props {
  onPick: (styleId: string, styleName: string) => void;
  onBack: () => void;
}

const STYLES = [
  { id: 'movie',        name: '电影解说',   desc: '抖音爆款电影·解说风格',         from: '#667eea', to: '#764ba2' },
  { id: 'series',       name: '电视剧解说', desc: '追剧向电视剧解说',               from: '#a18cd1', to: '#fbc2eb' },
  { id: 'variety',      name: '综艺解说',   desc: '综艺节目精彩片段解说',           from: '#f093fb', to: '#f5576c' },
  { id: 'anime',        name: '动漫解说',   desc: '番剧/动漫解说',                 from: '#4facfe', to: '#00f2fe' },
  { id: 'shortdrama',   name: '短剧解说',   desc: '竖屏短剧解说',                   from: '#fa709a', to: '#fee140' },
  { id: 'sports',       name: '体育解说',   desc: '体育赛事精彩解说',               from: '#43e97b', to: '#38f9d7' },
  { id: 'documentary',  name: '纪录片解说', desc: '纪录片/科普解说',                 from: '#0ba360', to: '#3cba92' },
  { id: 'general',      name: '通用解说',   desc: '通用视频解说',                   from: '#a8edea', to: '#fed6e3' },
  { id: '__more__',     name: '更多风格',   desc: '敬请期待...',                     from: '#e0c3fc', to: '#8ec5fc' },
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
            style={{ background: `linear-gradient(135deg, ${s.from}, ${s.to})` }}
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
