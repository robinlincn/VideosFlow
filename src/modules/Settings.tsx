import { useEffect, useRef, useState } from 'react';
import { useApp } from '../state/AppContext';
import { settingsSteps, type ProviderCfg } from '../data/mock';
import {
  loadProviders, saveProvider, setProviderKey, testProvider as ipcTestProvider,
  getModelsDir, submitChatTask, downloadModel, checkLocalModel, LOCAL_ASR_MODELS,
} from '../ipc/providers';

const PROVIDER_OPTS: Record<string, string[]> = {
  llm: ['Agnes', 'OpenAI 兼容', 'DeepSeek', '通义', '豆包', '混元', 'Ollama'],
  img: ['Agnes', '通义万相', 'SDXL', 'DALL·E', 'Midjourney'],
  asr: ['XiaomiMimo', 'Agnes', '云 ASR', '本地 faster-whisper', 'Whisper'],
  tts: ['Mimo', 'Edge-TTS', 'CosyVoice', '云 TTS'],
  video: ['Agnes', 'Runway', '通义万相', 'SVD', 'Pika'],
};

// 语音识别里可选的两个「本地推理」引擎：选中后无需填写 Base URL / 模型 / API Key
const LOCAL_ASR = ['本地 faster-whisper', 'Whisper'];
const isLocalProvider = (p: ProviderCfg): boolean =>
  p.mode === 'local' || LOCAL_ASR.includes(p.provider);

export default function Settings() {
  const { state, actions } = useApp();
  const { settingsSub } = state;

  // 挂载时从后端拉取真实 Provider 配置（仅一次）
  const hydrated = useRef(false);
  useEffect(() => {
    if (hydrated.current) return;
    hydrated.current = true;
    loadProviders()
      .then((rows) => actions.hydrateProviders(rows))
      .catch(() => {});
  }, [actions]);

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
  const [chatPrompt, setChatPrompt] = useState('用一句话介绍 VideosFlow');
  const [chatLog, setChatLog] = useState<string[]>([]);
  const [chatBusy, setChatBusy] = useState(false);
  const [chatAnswer, setChatAnswer] = useState<string | null>(null);
  const [modelsDir, setModelsDir] = useState('');
  // 本地模型下载相关状态（仅 ASR 本地模式使用）
  const [dlModel, setDlModel] = useState('base');
  const [dlSource, setDlSource] = useState('hf-mirror');
  const [dlBusy, setDlBusy] = useState(false);
  const [dlProg, setDlProg] = useState<{ current: number; total: number; file: string; phase: string }>({ current: 0, total: 0, file: '', phase: '' });
  const [localReady, setLocalReady] = useState<boolean | null>(null);

  // 加载本地模型目录（资源文件夹下的 models 子目录），供「本地推理」提示卡展示
  useEffect(() => {
    getModelsDir().then(setModelsDir).catch(() => setModelsDir(''));
  }, []);

  const copyModelsDir = async (text: string) => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      actions.task('模型目录路径已复制 ✓', 100);
    } catch {
      /* 剪贴板不可用时静默 */
    }
  };

  // 进入本地 ASR 模式时，检查所选尺寸的本地模型是否已下载
  useEffect(() => {
    const asr = providers['asr'];
    if (asr && isLocalProvider(asr)) {
      checkLocalModel(dlModel).then(setLocalReady).catch(() => setLocalReady(false));
    }
  }, [providers, dlModel]);

  const handleDownload = async () => {
    setDlBusy(true);
    setDlProg({ current: 0, total: 0, file: '', phase: 'listing' });
    try {
      const dir = await downloadModel(dlModel, dlSource, (p: any) => {
        setDlProg({ current: p.current || 0, total: p.total || 0, file: p.file || '', phase: p.phase || '' });
      });
      setLocalReady(true);
      actions.task(`本地模型已下载到 ${dir} ✓`, 100);
    } catch (e) {
      setLocalReady(false);
      actions.task(`下载失败: ${String(e)}`, 100);
    } finally {
      setDlBusy(false);
    }
  };

  const handleSave = async () => {
    actions.task('保存配置中…', 40);
    try {
      console.log('[videosflow-debug] handleSave start; providers keys:', Object.keys(providers));
      for (const k of Object.keys(providers)) {
        const p = providers[k];
        console.log(`[videosflow-debug] provider ${k}: apiKey len=${(p.apiKey || '').length} hasKey=${p.hasKey}`);
        const isLocal = isLocalProvider(p);
        await saveProvider({
          kind: k, name: p.name, provider: p.provider,
          baseUrl: p.baseUrl, model: p.model, enabled: p.enabled,
          mode: isLocal ? 'local' : 'cloud',
        });
        // 本地推理无需密钥；仅云端模式才写入 API Key
        if (!isLocal && p.apiKey && p.apiKey.trim()) {
          await setProviderKey(k, p.apiKey.trim());
        }
      }
      // 保存后重新拉取后端配置，刷新「已保存 KEY」标记，使 UI 立即显示已保存提示
      try {
        const rows = await loadProviders();
        actions.hydrateProviders(rows);
      } catch { /* 刷新失败不影响已保存结果 */ }
      actions.task('配置已保存 ✓', 100);
    } catch (e) {
      actions.task('保存失败: ' + String(e), 100);
    }
  };

  const handleTest = async (k: string) => {
    actions.setProviderTest(k, 'testing');
    try {
      const res = await ipcTestProvider(k, providers[k].apiKey);
      actions.setProviderTest(k, res === 'ok' ? 'ok' : res === 'local' ? 'local' : 'fail');
    } catch (e) {
      actions.setProviderTest(k, 'fail');
      actions.task(`测试 ${providers[k].name} 失败: ${String(e)}`, 100);
    }
  };

  const handleChat = async () => {
    setChatBusy(true);
    setChatAnswer(null);
    setChatLog([]);
    try {
      await submitChatTask(chatPrompt, (m) => {
        setChatLog((l) => [...l, `[${Math.round(m.progress)}% · ${m.status}] ${m.message || ''}`]);
        const ans = (m.payload as any)?.answer;
        if (ans) setChatAnswer(ans);
      });
    } catch (e) {
      setChatLog((l) => [...l, '失败: ' + String(e)]);
    } finally {
      setChatBusy(false);
    }
  };

  return (
    <div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <button className="btn sm ok" onClick={handleSave}>💾 保存全部配置</button>
        <button className="btn sm ghost" onClick={actions.resetSettings}>↻ 恢复默认</button>
      </div>
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
        {Object.keys(providers).map((k) => {
          const p = providers[k];
          const isLocal = isLocalProvider(p);
          const status =
            p.test === 'ok' ? '✓ 已连接'
            : p.test === 'local' ? '本地'
            : p.test === 'testing' ? '测试中…'
            : p.test === 'fail' ? '连接失败'
            : p.test === 'idle' ? '未测试'
            : p.test;
          const keyHint = p.hasKey
            ? (p.apiKey ? '检测到已保存的 KEY，填写新值将覆盖原 KEY' : '已保存 API Key（明文不回显）；如需更换请填写新 KEY')
            : (p.apiKey ? '将保存新的 API Key' : '尚未保存 API Key（相关功能受限）');
          return (
            <div key={k} className="pcard">
              <div className="ph"><span className="dot" /><b>{p.name}</b>
                <span className="tag key" style={{ marginLeft: 'auto' }}>{status}</span></div>
              <div className="field"><label>Provider</label>
                <select value={p.provider} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, provider: e.target.value } } } })}>
                  {(PROVIDER_OPTS[k] || [p.provider]).map((o) => <option key={o}>{o}</option>)}
                </select>
              </div>

              {isLocal ? (
                <div className="pcard-local">
                  <div className="pcard-local__badge">⚙ 本地推理 · 无需联网</div>
                  <div className="muted sm" style={{ marginBottom: 8 }}>
                    该引擎在本地运行，无需填写 Base URL / 模型 / API Key。请把对应的本地大模型权重下载到下方模型目录：
                  </div>
                  <div className="pcard-local__path">
                    <code>{modelsDir || '（加载中…）'}</code>
                    <button className="btn sm ghost" type="button" onClick={() => copyModelsDir(modelsDir)} disabled={!modelsDir}>复制路径</button>
                  </div>

                  <div className="local-dl">
                    <div className="field" style={{ marginTop: 8 }}><label>模型尺寸</label>
                      <select value={dlModel} onChange={(e) => setDlModel(e.target.value)} disabled={dlBusy}>
                        {LOCAL_ASR_MODELS.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
                      </select>
                    </div>
                    <div className="field"><label>下载源</label>
                      <select value={dlSource} onChange={(e) => setDlSource(e.target.value)} disabled={dlBusy}>
                        <option value="hf-mirror">HuggingFace 镜像 (hf-mirror.com，国内推荐)</option>
                        <option value="huggingface">HuggingFace 官方 (需可访问外网)</option>
                      </select>
                    </div>
                    <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 6 }}>
                      <button className="btn sm ok" onClick={handleDownload} disabled={dlBusy}>
                        {dlBusy ? '下载中…' : localReady ? '⬇ 重新下载' : '⬇ 下载模型'}
                      </button>
                      {localReady === true && <span className="tag ok">已就绪</span>}
                      {localReady === false && !dlBusy && <span className="tag key">未下载</span>}
                    </div>
                    {dlBusy && dlProg.phase === 'listing' && (
                      <div className="muted sm" style={{ marginTop: 6 }}>正在列举模型文件…</div>
                    )}
                    {dlBusy && dlProg.total > 0 && (
                      <div className="dl-progress" style={{ marginTop: 6 }}>
                        <div className="dl-bar" style={{ width: `${Math.min(100, (dlProg.current / dlProg.total) * 100)}%` }} />
                        <div className="muted sm" style={{ marginTop: 4 }}>
                          {dlProg.file} · {(dlProg.current / 1048576).toFixed(1)} / {(dlProg.total / 1048576).toFixed(1)} MB
                        </div>
                      </div>
                    )}
                  </div>

                  <div className="pcard-local__hint">
                    • 下载使用 <b>faster-whisper</b> 的 CTranslate2 权重（config.json + model.bin + tokenizer.json + vocabulary.txt）。<br/>
                    • 下载完成后，影片 / 口播的「语音识别」将自动走本地推理，不再依赖云端（无 402 / 无网络要求）。<br/>
                    <span className="muted">如本地已安装 Python + faster-whisper，也可手动把模型放到上述目录；或用环境变量 VF_PYTHON / VF_TRANSCRIBE_SCRIPT 指定解释器与脚本路径。</span>
                  </div>
                </div>
              ) : (
                <>
                  <div className="field"><label>Base URL</label><input value={p.baseUrl} placeholder="(留空使用默认)" onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, baseUrl: e.target.value } } } })} /></div>
                  <div className="field"><label>模型</label><input value={p.model} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, model: e.target.value } } } })} /></div>
                  <div className="field"><label>API Key</label><input type="password" value={p.apiKey} placeholder="存入系统凭据，不进 SQLite 明文" onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, apiKey: e.target.value } } } })} />
                    <div className="muted sm" style={{ marginTop: 4, color: p.hasKey ? 'var(--accent, #b85c38)' : 'var(--muted, #8a7f74)' }}>{keyHint}</div>
                  </div>
                </>
              )}

              <div style={{ display: 'flex', gap: 8, marginTop: 6, alignItems: 'center' }}>
                {!isLocal && (
                  <button className="btn sm ghost" onClick={() => handleTest(k)} disabled={p.test === 'testing'}>🔌 测试连接</button>
                )}
                <label style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 12, color: 'var(--muted)' }}>
                  <input type="checkbox" checked={p.enabled} onChange={(e) => actions.set({ settingsState: { ...state.settingsState, providers: { ...state.settingsState.providers, [k]: { ...p, enabled: e.target.checked } } } })} /> 启用
                </label>
              </div>
            </div>
          );
        })}
      </div>
      <div className="pcard" style={{ marginTop: 14 }}>
        <div className="ph"><span className="dot" /><b>链路验证 · 真实 Agnes /v1/chat</b>
          <span className="tag key" style={{ marginLeft: 'auto' }}>全链路</span></div>
        <div className="muted sm" style={{ marginBottom: 8 }}>填好 LLM 的 API Key 并「保存全部配置」后，点击下方按钮会经「Rust 命令 → reqwest 直连 Agnes /chat/completions」真实调用一次对话，回答经进度通道回传。用于验证 M1 全链路打通（不依赖 Python sidecar）。</div>
        <div className="field"><label>测试提示词</label>
          <textarea rows={2} value={chatPrompt} onChange={(e) => setChatPrompt(e.target.value)} />
        </div>
        <button className="btn sm ok" onClick={handleChat} disabled={chatBusy}>🚀 发送真实对话</button>
        {chatLog.length > 0 && (
          <pre style={{ marginTop: 10, whiteSpace: 'pre-wrap', fontSize: 12, color: 'var(--muted)', fontFamily: "var(--mono, 'Geist Mono', monospace)" }}>{chatLog.join('\n')}</pre>
        )}
        {chatAnswer && (
          <div style={{ marginTop: 8, padding: 10, borderLeft: '3px solid var(--accent, #b85c38)', background: 'var(--surface-2, rgba(0,0,0,0.03))' }}>
            <b>Agnes 回答：</b>
            <div style={{ marginTop: 4, whiteSpace: 'pre-wrap' }}>{chatAnswer}</div>
          </div>
        )}
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
