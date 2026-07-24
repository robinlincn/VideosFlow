import React from 'react';

interface State { error: Error | null; }

/** 全局错误边界：某个视图渲染崩溃时，显示可读错误而非整页白屏。 */
export default class ErrorBoundary extends React.Component<{ children: React.ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // eslint-disable-next-line no-console
    console.error('[ErrorBoundary]', error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 24, color: '#dc2626', fontFamily: 'var(--font-mono, monospace)' }}>
          <div style={{ fontWeight: 700, fontSize: 15, marginBottom: 8 }}>⚠ 页面渲染出错（已拦截，未白屏）</div>
          <div style={{ fontSize: 13, lineHeight: 1.6, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
            {String(this.state.error?.message || this.state.error)}
          </div>
          <button
            className="btn sm"
            style={{ marginTop: 12 }}
            onClick={() => this.setState({ error: null })}
          >重试</button>
        </div>
      );
    }
    return this.props.children;
  }
}
