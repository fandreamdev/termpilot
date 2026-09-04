import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { api } from './lib/tauri';
import type { Host, Session } from './types';
import { Terminal as XTerminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

type NavIconName = 'dashboard' | 'hosts' | 'terminal' | 'transfer' | 'audit' | 'settings';
const nav: Array<[string, NavIconName]> = [['总览', 'dashboard'], ['主机', 'hosts'], ['终端', 'terminal'], ['SFTP', 'transfer'], ['审计', 'audit'], ['设置', 'settings']];

function NavIcon({ name }: { name: NavIconName }) {
  const shapes: Record<NavIconName, ReactNode> = {
    dashboard: <><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></>,
    hosts: <><path d="M7 9.5a4 4 0 1 1 2.8 3.8L7.5 16H5v2H3v-3.2l4.7-4.7A4 4 0 0 1 7 9.5Z"/><path d="m15 16 3 3m-1.5-1.5 2-2"/></>,
    terminal: <><path d="m4 7 5 5-5 5"/><path d="M12 17h8"/></>,
    transfer: <><path d="M4 8h15"/><path d="m15 4 4 4-4 4"/><path d="M20 16H5"/><path d="m9 12-4 4 4 4"/></>,
    audit: <><path d="M5 20V11"/><path d="M12 20V5"/><path d="M19 20v-8"/><path d="M3 20h18"/></>,
    settings: <><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-1.8 1.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5v.1h-2.6v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1-1.8-1.8.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H6v-2.6h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.9l-.1-.1L9 6.6l.1.1a1.7 1.7 0 0 0 1.9.3 1.7 1.7 0 0 0 1-1.5v-.1h2.6v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1 1.8 1.8-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.5 1h.1V14h-.1a1.7 1.7 0 0 0-1.5 1Z"/></>
  };
  return <svg className="nav-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{shapes[name]}</svg>;
}

export default function App() {
  const [page, setPage] = useState('总览');
  const [hosts, setHosts] = useState<Host[]>([]);
  const [session, setSession] = useState<Session>();
  const [agentOpen, setAgentOpen] = useState(true);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [theme, setTheme] = useState<'dark' | 'light'>('dark');
  const [command, setCommand] = useState('');
  const [messages, setMessages] = useState<string[]>(['已连接 prod-api-01', '我会先执行安全的只读诊断。']);
  useEffect(() => { api.hosts().then(setHosts); }, []);
  const connect = async (h: Host) => setSession(await api.connect(h.id));
  const send = () => { const value = command.trim(); if (!value) return; setMessages(m => [...m, value, '收到，我会先读取脱敏终端上下文。']); setCommand(''); };

  return <div className={'app ' + (theme === 'light' ? 'theme-light' : '')}>
    <aside className={'side ' + (sidebarCollapsed ? 'collapsed' : '')}>
      <div className="brand"><span className="logo"><span>TP</span></span><b>TermPilot</b></div>
      <nav>{nav.map(([label, icon]) => <button className={page === label ? 'active' : ''} onClick={() => setPage(label)} key={label}><span className="nav-icon"><NavIcon name={icon}/></span><span className="nav-label">{label}</span></button>)}</nav>
      <div className="side-bottom">
        <button className="stop" onClick={() => api.emergencyStop()}><span className="stop-icon">■</span><span className="stop-label">紧急停止</span></button>
        <button className="theme-toggle" onClick={() => setTheme(v => v === 'dark' ? 'light' : 'dark')} aria-label={theme === 'dark' ? '切换浅色模式' : '切换深色模式'}><span className="theme-icon">{theme === 'dark' ? '☼' : '☾'}</span><span className="theme-label">{theme === 'dark' ? '浅色模式' : '深色模式'}</span></button>
        <button className="collapse-toggle" onClick={() => setSidebarCollapsed(v => !v)} aria-label={sidebarCollapsed ? '展开导航栏' : '折叠导航栏'}><span className="collapse-icon">{sidebarCollapsed ? '››' : '‹‹'}</span><span className="collapse-label">{sidebarCollapsed ? '展开' : '收起'}</span></button>
      </div>
    </aside>
    <main className="main"><header className="top"><span>个人工作台</span><span>/</span><b>{page}</b><span className="grow"/><span className="tag">Windows x64 · 本地模式</span><span className="tag">审计链 ✓</span></header>{page === '终端' ? <Terminal session={session} light={theme === 'light'} open={agentOpen} setOpen={setAgentOpen} command={command} setCommand={setCommand} messages={messages} send={send}/> : <Dashboard hosts={hosts} connect={connect} page={page}/>}</main>
  </div>;
}

function Dashboard({ hosts, connect, page }: { hosts: Host[]; connect: (h: Host) => void; page: string }) { return <section className="content"><div className="hero"><div><h1>{page}</h1><p>TermPilot 个人版工作台 · 所有远程操作经过 Rust Core 策略和审计。</p></div><button className="btn primary" onClick={() => hosts[0] && connect(hosts[0])}>打开终端</button></div><div className="cards">{[['活动会话', '0 / 8'], ['待审批操作', '2'], ['今日审计事件', '128'], ['传输任务', '1']].map(([k, v]) => <div className="card" key={k}><span className="muted">{k}</span><div className="metric">{v}</div></div>)}</div><div className="card table-card"><h2>主机状态</h2><table><thead><tr><th>名称</th><th>地址</th><th>认证</th><th>状态</th><th/></tr></thead><tbody>{hosts.map(h => <tr key={h.id}><td><b>{h.name}</b>{h.is_production && <span className="badge red">生产</span>}</td><td>{h.username}@{h.address}:{h.port}</td><td>{h.auth_method}</td><td><span className="badge green">● ready</span></td><td><button className="btn" onClick={() => connect(h)}>连接</button></td></tr>)}</tbody></table></div></section>; }
function ShellSurface({ light }: { light: boolean }) { const host = useRef<HTMLDivElement>(null); useEffect(() => { if (!host.current) return; const term = new XTerminal({ convertEol: true, cursorBlink: true, fontFamily: 'Consolas, monospace', fontSize: 13, theme: { background: light ? '#ffffff' : '#080d18', foreground: light ? '#24344a' : '#cbd6ed', cursor: light ? '#24344a' : '#cbd6ed' } }); const fit = new FitAddon(); term.loadAddon(fit); term.open(host.current); fit.fit(); term.writeln('TermPilot SSH session · fingerprint verified'); term.writeln('\x1b[32m● Connected to prod-api-01 (10.0.10.21)\x1b[0m'); term.writeln('Last login: Fri Sep 04 09:12:03 2026'); term.write('\x1b[94mops@prod-api-01:~$\x1b[0m '); const resize = () => fit.fit(); window.addEventListener('resize', resize); return () => { window.removeEventListener('resize', resize); term.dispose(); }; }, [light]); return <div className="xterm-host" ref={host}/>; }
function Terminal({ light, open, setOpen, command, setCommand, messages, send }: { session?: Session; light: boolean; open: boolean; setOpen: (v: boolean) => void; command: string; setCommand: (v: string) => void; messages: string[]; send: () => void }) { return <section className="content terminal-content"><div className="hero"><div><h1>远程终端</h1><p>SSH PTY 会话 · Shell 与 Agent 分屏工作区</p></div><button className="btn red">断开当前会话</button></div><div className={'term-shell ' + (!open ? 'agent-hidden' : '')}><section className="shell-pane"><div className="term-tabs"><span className="tab active">prod-api-01 · ready</span><span className="tab">test-api-02 · ready</span><span className="tab">bastion · connecting</span><span className="tab-spacer"/><button className="btn plus">＋</button><button className="btn agent-toggle on" onClick={() => setOpen(!open)}>✦ Agent {open ? '已打开' : ''}</button></div><ShellSurface light={light}/><div className="shell-input"><span>ops@prod-api-01:~$</span><input value={command} onChange={e => setCommand(e.target.value)} onKeyDown={e => e.key === 'Enter' && send()} placeholder="输入命令（Mock）"/></div></section>{open && <aside className="agent-pane"><div className="agent-head"><strong>● Agent 助手</strong><span className="badge yellow">需审批</span><button className="btn" onClick={() => setOpen(false)}>×</button></div><div className="agent-body">{messages.map((m, i) => <div className={'agent-msg ' + (i % 3 === 1 ? 'user' : '')} key={`${m}-${i}`}>{m}</div>)}</div><div className="agent-compose"><input placeholder="询问 Agent…"/><button className="btn primary">↑</button></div></aside>}</div></section>; }
