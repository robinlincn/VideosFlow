import { useApp } from '../state/AppContext';
import { settingsSteps } from '../data/mock';

const PROVIDER_OPTS: Record<string, string[]> = {
  llm: ['OpenAI 兼容', 'DeepSeek', '通义', '豆包', '混元', 'Ollama'],
  img: ['通义万相', 'SDXL', 'DALL·E', 'Midjourney'],
  asr: ['本地 faster-whisper', '云 ASR', 'Whisper'],
  tts: ['Edge-TTS', 'CosyVoice', '云 TTS'],
  video: ['Runway', '通义万相', 'SVD', 'Pika'],
};

export default function Settings() {
  const { state, actions } = useApp();
  const { settingsSub, settingsState } = state;
  return (
    <div>
      <StepPills current={settingsSub} onPick={actions.goSettingsSub} />
      {settingsSub === 'api' && <ApiView />}
      {settingsSub === 'prompt' && <PromptView />}
      {settingsSub === 'other' && <OtherView />}
    </div>
  );
}

function StepPills({ current, onPick }: { current: string; onPick: (id: string) => void; }) {
  return (
    <div className="steps">
      {settingsSteps.map((s, i) => {
        const cur = settingsSteps.findIndex((x) => x.id === current);
        const cls = s.id === current ? 'active' : i < cur ? 'done' : '';
        return <div key={s.id} className={'step ' + cls} onClick={() => onPick(s.id)}>{s.name}</div>;
      })}
    </div>
  );
}

function ApiView() {
  const { state, actions } = useApp();
  const { providers } = state.settingsState;
  return (
    <div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <button className="btn sm ok" onClick={actions.saveSettings}>💾 保存全部配置</button>
        <button className="btn sm ghost" onClick={actions.resetSettings}>↻ 恢复默认</button>
      </div>
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
        {Object.keys(providers).map((k) => {
          const p = providers[k];
          const status = p.test === 'ok' ? '✓ 已连接' : p.test === 'local' ? '本地' : p.test === 'idle' ? '未测试' : p.test;
          return (
            <div key={k} className="pcard">
              <div className="ph"><span className="dot" /><b>{p.name}</b>
                <span className="tag key" style={{ marginLeft: 'auto' }}>{status}</span></div>
              <div className="field"><label>Provider</label>
                <select value={p.provider} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, provider: e.target.value } } } })}>
                  {(PROVIDER_OPTS[k] || [p.provider]).map((o) => <option key={o}>{o}</option>)}
                </select>
              </div>
              <div className="field"><label>Base URL</label><input value={p.baseUrl} placeholder="(留空使用默认)" onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, baseUrl: e.target.value } } } })} /></div>
              <div className="field"><label>模型</label><input value={p.model} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, model: e.target.value } } } })} /></div>
              <div className="field"><label>API Key</label><input type="password" value={p.apiKey} placeholder="存入系统凭据，不进 SQLite 明文" onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, apiKey: e.target.value } } } })} /></div>
              <div style={{ display: 'flex', gap: 8, marginTop: 6, alignItems: 'center' }}>
                <button className="btn sm ghost" onClick={() => actions.testProvider(k)}>🔌 测试连接</button>
                <label style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 12, color: 'var(--muted)' }}>
                  <input type="checkbox" checked={p.enabled} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, enabled: e.target.checked } } } })} /> 启用
                </label>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function PromptView() {
  const { state, actions } = useApp();
  const { prompts, promptEditing } = state.settingsState;
  const cur = prompts[promptEditing] || Object.values(prompts)[0];
  const keys = Object.keys(prompts);
  const activeKey = promptEditing && prompts[promptEditing] ? promptEditing : keys[0];
  return (
    <div className="edit-grid" style={{ gridTemplateColumns: '220px 1fr' }}>
      <div>
        <div className="side-label">提示词模板</div>
        <div className="prompt-list">
          {keys.map((k) => (
            <div key={k} className={'prompt-item ' + (k === activeKey ? 'active' : '')} onClick={() => actions.set({ settingsState: { ...state.settingsState, promptEditing: k } })}>{prompts[k].name}</div>
          ))}
        </div>
      </div>
      <div className="card">
        <div className="sec-title">{cur.name}</div>
        <div className="field"><label>模板名称</label><input value={cur.name} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, prompts: { ...prompts, [activeKey]: { ...cur, name: e.target.value } } } })} /></div>
        <div className="field"><label>模板内容</label><textarea rows={10} value={cur.body} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, prompts: { ...prompts, [activeKey]: { ...cur, body: e.target.value } } } })} /></div>
        <div className="muted sm">可用变量：{'{{brief}} {{style}} {{audience}} {{title}} {{script}} {{transcript}}'}</div>
        <div style={{ marginTop: 12, display: 'flex', gap: 8 }}>
          <button className="btn sm" onClick={actions.savePrompt}>💾 保存</button>
          <button className="btn sm ghost" onClick={actions.resetSettings}>↻ 恢复默认</button>
        </div>
      </div>
    </div>
  );
}

function OtherView() {
  const { state, actions } = useApp();
  const { other } = state.settingsState;
  const setO = (k: string, v: unknown) => actions.updOther(k, v);
  return (
    <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', maxWidth: 820 }}>
      <div className="pcard">
        <div className="ph"><span className="dot" /><b>外观与语言</b></div>
        <div className="field"><label>主题</label><select value={other.theme} onChange={(e) => setO('theme', e.target.value)}><option>浅色</option><option>深色</option><option>跟随系统</option></select></div>
        <div className="field"><label>语言</label><select value={other.lang} onChange={(e) => setO('lang', e.target.value)}><option>简体中文</option><option>English</option></select></div>
      </div>
      <div className="pcard">
        <div className="ph"><span className="dot" /><b>性能 / 硬件加速</b></div>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)' }}>
          <input type="checkbox" checked={other.hwAccel} onChange={(e) => setO('hwAccel', e.target.checked)} /> 启用 FFmpeg 硬件加速
        </label>
        <div className="field" style={{ marginTop: 8 }}><label>任务并发数</label><input type="number" value={other.taskConcurrency} onChange={(e) => setO('taskConcurrency', +e.target.value)} /></div>
      </div>
      <div className="pcard">
        <div className="ph"><span className="dot" /><b>默认导出</b></div>
        <div className="field"><label>分辨率</label><select value={other.exportResolution} onChange={(e) => setO('exportResolution', e.target.value)}>
          {['3840×2160', '1920×1080', '1280×720', '1080×1920 (竖屏)'].map((x) => <option key={x}>{x}</option>)}</select></div>
        <div className="field"><label>封装格式</label><select value={other.exportFormat} onChange={(e) => setO('exportFormat', e.target.value)}>
          {['MP4 (H.264)', 'MP4 (H.265/HEVC)', 'MOV (ProRes)', 'WebM (VP9)'].map((x) => <option key={x}>{x}</option>)}</select></div>
      </div>
      <div className="pcard">
        <div className="ph"><span className="dot" /><b>自动保存</b></div>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 12.5, color: 'var(--muted)' }}>
          <input type="checkbox" checked={other.autoSave} onChange={(e) => setO('autoSave', e.target.checked)} /> 自动保存工程
        </label>
        <div className="field" style={{ marginTop: 8 }}><label>保存间隔（秒）</label><input type="number" value={other.autoSaveSec} onChange={(e) => setO('autoSaveSec', +e.target.value)} /></div>
        <div className="field"><label>临时文件保留（天）</label><input type="number" value={other.cleanupDays} onChange={(e) => setO('cleanupDays', +e.target.value)} /></div>
        <div className="field"><label>FFmpeg 路径</label><input value={other.ffmpegPath} onChange={(e) => setO('ffmpegPath', e.target.value)} /></div>
        <div className="field"><label>临时目录</label><input value={other.tempDir} onChange={(e) => setO('tempDir', e.target.value)} /></div>
      </div>
    </div>
  );
}
