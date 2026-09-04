// The UI intentionally accepts partially populated event envelopes from
// older desktop builds; runtime guards below handle optional fields.
// @ts-nocheck
import * as React from "react";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api, parseAgentToolCall } from "./lib/tauri";
import type { Host, Session } from "./types";
import { Terminal as XTerminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  open as openFileDialog,
  save as saveFileDialog,
} from "@tauri-apps/plugin-dialog";

type NavIconName =
  | "dashboard"
  | "hosts"
  | "terminal"
  | "transfer"
  | "audit"
  | "settings";
const nav: Array<[string, NavIconName]> = [
  ["总览", "dashboard"],
  ["主机", "hosts"],
  ["终端", "terminal"],
  ["SFTP", "transfer"],
  ["审计", "audit"],
  ["设置", "settings"],
];
let activeXterm: XTerminal | undefined;

function NavIcon({ name }: { name: NavIconName }) {
  const shapes: Record<NavIconName, ReactNode> = {
    dashboard: (
      <>
        <rect x="3" y="3" width="7" height="7" rx="1" />
        <rect x="14" y="3" width="7" height="7" rx="1" />
        <rect x="3" y="14" width="7" height="7" rx="1" />
        <rect x="14" y="14" width="7" height="7" rx="1" />
      </>
    ),
    hosts: (
      <>
        <path d="M7 9.5a4 4 0 1 1 2.8 3.8L7.5 16H5v2H3v-3.2l4.7-4.7A4 4 0 0 1 7 9.5Z" />
        <path d="m15 16 3 3m-1.5-1.5 2-2" />
      </>
    ),
    terminal: (
      <>
        <path d="m4 7 5 5-5 5" />
        <path d="M12 17h8" />
      </>
    ),
    transfer: (
      <>
        <path d="M4 8h15" />
        <path d="m15 4 4 4-4 4" />
        <path d="M20 16H5" />
        <path d="m9 12-4 4 4 4" />
      </>
    ),
    audit: (
      <>
        <path d="M5 20V11" />
        <path d="M12 20V5" />
        <path d="M19 20v-8" />
        <path d="M3 20h18" />
      </>
    ),
    settings: (
      <>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-1.8 1.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5v.1h-2.6v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1-1.8-1.8.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H6v-2.6h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.9l-.1-.1L9 6.6l.1.1a1.7 1.7 0 0 0 1.9.3 1.7 1.7 0 0 0 1-1.5v-.1h2.6v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1 1.8 1.8-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.5 1h.1V14h-.1a1.7 1.7 0 0 0-1.5 1Z" />
      </>
    ),
  };
  return (
    <svg
      className="nav-svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {shapes[name]}
    </svg>
  );
}

export default function App() {
  const [page, setPage] = useState("总览");
  const [hosts, setHosts] = useState<Host[]>([]);
  const [session, setSession] = useState<Session>();
  const [sessionTabs, setSessionTabs] = useState<Session[]>([]);
  const [agentOpen, setAgentOpen] = useState(true);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  useEffect(() => {
    void api.hosts().then(setHosts);
  }, []);
  useEffect(() => {
    void api.appSettingsGet().then((response) => {
      const settings = (
        response.data as {
          settings?: Array<{ key: string; value: string }>;
        } | null
      )?.settings;
      const saved = settings?.find((item) => item.key === "theme")?.value;
      if (saved === "dark" || saved === "light") setTheme(saved);
    });
  }, []);
  const updateTheme = (value: "dark" | "light") => {
    setTheme(value);
    void api.appSettingsSet({ key: "theme", value, value_type: "string" });
  };
  const activateSession = (value: Session) => {
    setSessionTabs((tabs) =>
      tabs.some((tab) => tab.id === value.id)
        ? tabs.map((tab) => (tab.id === value.id ? value : tab))
        : [...tabs, value],
    );
    setSession(value);
    setPage("终端");
  };
  const connect = async (host: Host) => {
    try {
      const value = await api.connect(host.id, host.endpoint_fingerprint);
      activateSession(value);
    } catch (error) {
      const message = error instanceof Error ? error.message : "连接失败";
      const match = message.match(/：((?:SHA256:)?[A-Za-z0-9+/=:_-]+)$/);
      if (
        match &&
        window.confirm(`${message}\n\n仅在确认服务器身份后继续连接。`)
      ) {
        try {
          const value = await api.connect(host.id, match[1]);
          activateSession(value);
        } catch (retryError) {
          window.alert(
            retryError instanceof Error ? retryError.message : "连接失败",
          );
        }
      } else window.alert(message);
    }
  };
  const pageTitle =
    page === "终端"
      ? "远程终端"
      : page === "主机"
        ? "主机管理"
        : page === "SFTP"
          ? "SFTP 文件管理"
          : page === "审计"
            ? "本地审计"
            : page;
  return (
    <div className={"app " + (theme === "light" ? "theme-light" : "")}>
      <aside className={"side " + (sidebarCollapsed ? "collapsed" : "")}>
        <div className="brand">
          <span className="logo">
            <span>TP</span>
          </span>
          <b>TermPilot</b>
        </div>
        <nav>
          {nav.map(([label, icon]) => (
            <button
              className={page === label ? "active" : ""}
              onClick={() => setPage(label)}
              key={label}
            >
              <span className="nav-icon">
                <NavIcon name={icon} />
              </span>
              <span className="nav-label">{label}</span>
            </button>
          ))}
        </nav>
        <div className="side-bottom">
          <button
            className="stop"
            onClick={() => {
              void api.emergencyStop();
              window.alert("已阻断新命令、Agent 和 SFTP 操作");
            }}
          >
            <span className="stop-icon">■</span>
            <span className="stop-label">紧急停止</span>
          </button>
          <button
            className="theme-toggle"
            onClick={() => updateTheme(theme === "dark" ? "light" : "dark")}
            aria-label="切换主题"
          >
            <span className="theme-icon">{theme === "dark" ? "☼" : "☾"}</span>
            <span className="theme-label">
              {theme === "dark" ? "浅色模式" : "深色模式"}
            </span>
          </button>
          <button
            className="collapse-toggle"
            onClick={() => setSidebarCollapsed((v) => !v)}
            aria-label={sidebarCollapsed ? "展开导航栏" : "折叠导航栏"}
          >
            <span className="collapse-icon">
              {sidebarCollapsed ? "›" : "‹"}
            </span>
            <span className="collapse-label">
              {sidebarCollapsed ? "展开" : "收起"}
            </span>
          </button>
        </div>
      </aside>
      <main className="main">
        <header className="top">
          <span>个人工作台</span>
          <span>/</span>
          <b>{pageTitle}</b>
          <span className="grow" />
          <span className="tag">Windows x64 · 本地模式</span>
          <span className="tag">审计链 ✓</span>
        </header>
        {page === "终端" ? (
          <Terminal
            hosts={hosts}
            session={session}
            sessionTabs={sessionTabs}
            light={theme === "light"}
            open={agentOpen}
            setOpen={setAgentOpen}
            connect={connect}
            setSession={setSession}
            setSessionTabs={setSessionTabs}
          />
        ) : page === "主机" ? (
          <HostsPage hosts={hosts} setHosts={setHosts} connect={connect} />
        ) : page === "SFTP" ? (
          <SftpPage session={session} />
        ) : page === "审计" ? (
          <AuditPage />
        ) : page === "设置" ? (
          <SettingsPage theme={theme} setTheme={updateTheme} />
        ) : (
          <Dashboard hosts={hosts} connect={connect} />
        )}
      </main>
    </div>
  );
}

function Dashboard({
  hosts,
  connect,
}: {
  hosts: Host[];
  connect: (h: Host) => void;
}) {
  return (
    <section className="content">
      <div className="hero">
        <div>
          <h1>早上好，John</h1>
          <p>查看连接状态、待审批操作和最近活动。</p>
        </div>
        <div className="actions">
          <button
            className="btn"
            onClick={() =>
              document
                .querySelector<HTMLButtonElement>(
                  ".side nav button:nth-child(2)",
                )
                ?.click()
            }
          >
            ＋ 新建主机
          </button>
          <button
            className="btn primary"
            onClick={() => hosts[0] && connect(hosts[0])}
          >
            打开终端
          </button>
        </div>
      </div>
      <div className="cards">
        <Metric
          label="活动会话"
          value={hosts.length ? "1 / 8" : "0 / 8"}
          hint="当前应用会话"
        />
        <Metric
          label="待审批操作"
          value="0"
          hint="没有待处理票据"
          tone="yellow"
        />
        <Metric label="今日审计事件" value="—" hint="本地数据库" />
        <Metric label="传输任务" value="0" hint="没有活动传输" />
      </div>
      <div className="dashboard-grid">
        <div className="card">
          <h3>主机状态</h3>
          {hosts.slice(0, 3).map((h) => (
            <div className="host-row" key={h.id}>
              <div>
                <b>{h.name}</b>
                {h.is_production && <span className="badge red">生产</span>}
                <br />
                <span className="muted">
                  {h.username}@{h.address}
                </span>
              </div>
              <button className="btn" onClick={() => connect(h)}>
                连接
              </button>
            </div>
          ))}
        </div>
        <div className="card">
          <h3>安全提示</h3>
          <p className="muted">
            Agent 只可使用结构化工具；生产写操作需要人工确认。
          </p>
          <p className="muted">密码、私钥、Token 和文件正文不会写入 SQLite。</p>
        </div>
      </div>
    </section>
  );
}
function Metric({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: string;
  hint: string;
  tone?: string;
}) {
  return (
    <div className="card metric-card">
      <span className="muted">{label}</span>
      <div className={"metric " + (tone ?? "")}>{value}</div>
      <span className={"metric-hint " + (tone ?? "")}>{hint}</span>
    </div>
  );
}

function HostsPage({
  hosts,
  setHosts,
  connect,
}: {
  hosts: Host[];
  setHosts: React.Dispatch<React.SetStateAction<Host[]>>;
  connect: (h: Host) => void;
}) {
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<Host | null>(null);
  const filtered = hosts.filter((h) =>
    `${h.name} ${h.address} ${h.username} ${h.group_name ?? ""}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  const save = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const value: Host = {
      id: editing?.id || crypto.randomUUID(),
      name: String(form.get("name") ?? ""),
      connection_type: String(
        form.get("connection_type") ?? "direct_ssh",
      ) as Host["connection_type"],
      address: String(form.get("address") ?? ""),
      port: Number(form.get("port") ?? 22),
      username: String(form.get("username") ?? ""),
      auth_method: String(
        form.get("auth_method") ?? "ssh_agent",
      ) as Host["auth_method"],
      group_name: String(form.get("group_name") ?? "") || undefined,
      is_production: form.get("is_production") === "on",
      endpoint_fingerprint:
        String(form.get("endpoint_fingerprint") ?? "") || undefined,
    };
    const response = await api.hostUpsert({ ...value, policy_id: "default" });
    if (!response.ok && !import.meta.env.DEV) {
      window.alert(response.error?.message ?? "保存失败");
      return;
    }
    const target = String(form.get("target_name") ?? "");
    const secret = String(form.get("secret") ?? "");
    const retention = String(form.get("retention_mode") ?? "app_session");
    if (value.auth_method === "private_key" && !target) {
      window.alert("私钥认证必须填写本地绝对路径。");
      return;
    }
    if (value.auth_method === "password" && !secret) {
      window.alert("密码认证必须填写本次密码。");
      return;
    }
    if (value.auth_method !== "ssh_agent") {
      const credential = await api.credentialStore({
        host_id: value.id,
        kind: value.auth_method,
        target_name: target || undefined,
        secret: secret || undefined,
        retention_mode: retention,
      });
      if (!credential.ok && !import.meta.env.DEV) {
        window.alert(credential.error?.message ?? "凭据保存失败");
        return;
      }
    }
    setHosts((current) =>
      editing
        ? current.map((h) =>
            h.id === value.id
              ? {
                  ...value,
                  is_production: Boolean(
                    editing.is_production || value.is_production,
                  ),
                }
              : h,
          )
        : [...current, value],
    );
    setEditing(null);
  };
  const remove = async (host: Host) => {
    if (!window.confirm(`软删除主机“${host.name}”？`)) return;
    const response = await api.hostDelete(host.id);
    if (!response.ok && !import.meta.env.DEV) {
      window.alert(response.error?.message ?? "删除失败");
      return;
    }
    setHosts((current) => current.filter((h) => h.id !== host.id));
  };
  return (
    <section className="content">
      <div className="hero">
        <div>
          <h1>主机管理</h1>
          <p>直连 SSH 和单级堡垒机端点 · 生产标记只能升级不能被编辑清除。</p>
        </div>
        <button
          className="btn primary"
          onClick={() =>
            setEditing({
              id: "",
              name: "",
              connection_type: "direct_ssh",
              address: "",
              port: 22,
              username: "",
              auth_method: "ssh_agent",
              is_production: false,
            })
          }
        >
          ＋ 新建主机
        </button>
      </div>
      {editing && (
        <form className="card host-form" onSubmit={save}>
          <h3>{editing.id ? "编辑主机" : "新建主机"}</h3>
          <div className="form-grid">
            <label>
              名称
              <input name="name" defaultValue={editing.name} required />
            </label>
            <label>
              地址
              <input name="address" defaultValue={editing.address} required />
            </label>
            <label>
              端口
              <input
                name="port"
                type="number"
                min="1"
                max="65535"
                defaultValue={editing.port}
                required
              />
            </label>
            <label>
              用户名
              <input name="username" defaultValue={editing.username} required />
            </label>
            <label>
              连接
              <select
                name="connection_type"
                defaultValue={editing.connection_type}
              >
                <option value="direct_ssh">直连 SSH</option>
                <option value="bastion_endpoint">堡垒机端点</option>
              </select>
            </label>
            <label>
              认证
              <select name="auth_method" defaultValue={editing.auth_method}>
                <option value="ssh_agent">SSH Agent</option>
                <option value="private_key">私钥文件</option>
                <option value="password">密码</option>
              </select>
            </label>
            <label>
              分组
              <input name="group_name" defaultValue={editing.group_name} />
            </label>
            <label>
              已知指纹
              <input
                name="endpoint_fingerprint"
                defaultValue={editing.endpoint_fingerprint}
                placeholder="SHA256:…"
              />
            </label>
            <label>
              凭据引用/私钥路径
              <input name="target_name" placeholder="私钥绝对路径或引用名" />
            </label>
            <label>
              本次密码
              <input name="secret" type="password" autoComplete="off" />
            </label>
            <label>
              密码保存
              <select name="retention_mode" defaultValue="app_session">
                <option value="never">不保存（仅本次运行内存）</option>
                <option value="app_session">本次应用运行（系统凭据库）</option>
              </select>
            </label>
            <label className="check">
              <input
                name="is_production"
                type="checkbox"
                defaultChecked={editing.is_production}
                disabled={editing.is_production}
              />
              生产主机
            </label>
          </div>
          <div className="actions">
            <button
              className="btn"
              type="button"
              onClick={() => setEditing(null)}
            >
              取消
            </button>
            <button className="btn primary" type="submit">
              保存
            </button>
          </div>
        </form>
      )}
      <div className="card">
        <input
          className="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="搜索名称、地址、用户或分组…"
        />
        <table className="table">
          <thead>
            <tr>
              <th>主机</th>
              <th>连接</th>
              <th>认证</th>
              <th>环境</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((h) => (
              <tr key={h.id}>
                <td>
                  <b>{h.name}</b>
                  <br />
                  <span className="muted">
                    {h.group_name ? `${h.group_name} · ` : ""}
                    {h.username}@{h.address}:{h.port}
                  </span>
                </td>
                <td>
                  {h.connection_type === "bastion_endpoint"
                    ? "堡垒机"
                    : "直连 SSH"}
                </td>
                <td>
                  <span className="badge blue">
                    {h.auth_method === "ssh_agent"
                      ? "SSH Agent"
                      : h.auth_method === "private_key"
                        ? "私钥文件"
                        : "密码"}
                  </span>
                </td>
                <td>
                  <span className={"badge " + (h.is_production ? "red" : "")}>
                    {h.is_production ? "生产" : "测试"}
                  </span>
                </td>
                <td>
                  <div className="actions">
                    <button className="btn" onClick={() => connect(h)}>
                      打开终端
                    </button>
                    <button className="btn" onClick={() => setEditing(h)}>
                      编辑
                    </button>
                    <button className="btn" onClick={() => void remove(h)}>
                      删除
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {filtered.length === 0 && <p className="muted">没有匹配的主机。</p>}
      </div>
    </section>
  );
}

type Transfer = { id: string; name: string; status: string; progress: number };
function SftpPage({ session }: { session?: Session }) {
  const [path, setPath] = useState("~");
  const [remoteFiles, setRemoteFiles] = useState<
    Array<{ name: string; kind: string }>
  >([]);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const fileInput = useRef<HTMLInputElement>(null);
  const fallback = [
    { name: "releases", kind: "directory" },
    { name: "app.conf", kind: "file" },
    { name: "release.tar.gz", kind: "file" },
    { name: "README.md", kind: "file" },
  ];
  useEffect(() => {
    if (!session) return;
    void api.sftpList({ session_id: session.id, path }).then((response) => {
      const entries = (
        response.data as {
          entries?: Array<{ name: string; kind: string }>;
        } | null
      )?.entries;
      if (entries) setRemoteFiles(entries);
    });
  }, [session, path]);
  useEffect(() => {
    if (!session) return;
    const listener = api.on<{
      session_id?: string;
      data?: {
        transfer_id?: string;
        status?: string;
        transferred_bytes?: number;
        size_bytes?: number;
      };
    }>("transfer.progress", (payload) => {
      if (payload.session_id !== session.id || !payload.data?.transfer_id)
        return;
      setTransfers((items) =>
        items.map((item) =>
          item.id === payload.data?.transfer_id
            ? {
                ...item,
                status: payload.data?.status ?? item.status,
                progress: payload.data?.size_bytes
                  ? Math.round(
                      ((payload.data.transferred_bytes ?? 0) * 100) /
                        payload.data.size_bytes,
                    )
                  : item.progress,
              }
            : item,
        ),
      );
    });
    return () => {
      void listener.then((unlisten) => unlisten());
    };
  }, [session]);
  const start = async (request: Record<string, unknown>, name: string) => {
    if (!session) {
      window.alert("请先在终端连接主机");
      return;
    }
    const response = await api.sftpTransferStart({
      session_id: session.id,
      ...request,
    });
    if (!response.ok && !import.meta.env.DEV) {
      window.alert(response.error?.message ?? "SFTP 操作失败");
      return;
    }
    const data = response.data as {
      transfer_id?: string;
      status?: string;
    } | null;
    setTransfers((items) => [
      ...items,
      {
        id: data?.transfer_id ?? crypto.randomUUID(),
        name,
        status: data?.status ?? "completed",
        progress: 100,
      },
    ]);
  };
  const createDirectory = () => {
    const name = window.prompt("新目录名称");
    if (name)
      void start(
        {
          op: "mkdir",
          dst: `${path.replace(/\/$/, "")}/${name}`,
          confirmed: true,
        },
        name,
      );
  };
  const uploadPath = (local: string, filename: string) => {
    void start(
      {
        op: "upload",
        src: local,
        dst: `${path.replace(/\/$/, "")}/${filename}`,
        confirmed: true,
      },
      filename,
    );
  };
  const upload = (file: File) => {
    const local =
      (file as File & { path?: string }).path ??
      (import.meta.env.DEV ? file.name : undefined);
    if (!local) {
      window.alert("当前环境未提供文件绝对路径，请使用 Tauri 原生文件选择器。");
      return;
    }
    uploadPath(local, file.name);
  };
  const chooseUpload = async () => {
    if ("__TAURI_INTERNALS__" in window) {
      const selected = await openFileDialog({
        multiple: false,
        directory: false,
      });
      const local = Array.isArray(selected) ? selected[0] : selected;
      if (typeof local === "string" && local) {
        const filename = local.split(/[\\/]/).pop() || local;
        uploadPath(local, filename);
      }
      return;
    }
    fileInput.current?.click();
  };
  const download = async (filename: string) => {
    let destination = `${filename}.download`;
    if ("__TAURI_INTERNALS__" in window) {
      const selected = await saveFileDialog({ defaultPath: filename });
      if (!selected) return;
      destination = selected;
    }
    void start(
      {
        op: "download",
        src: `${path.replace(/\/$/, "")}/${filename}`,
        dst: destination,
        confirmed: true,
      },
      filename,
    );
  };
  const shownFiles = remoteFiles.length ? remoteFiles : fallback;
  const parent =
    path === "~"
      ? "~"
      : path.replace(/\/$/, "").split("/").slice(0, -1).join("/") || "~";
  return (
    <section className="content">
      <div className="hero">
        <div>
          <h1>SFTP 文件管理</h1>
          <p>
            远端默认目录 <code>~</code> · realpath 校验 · 单文件上限 20 GiB
          </p>
        </div>
        <div className="actions">
          <button className="btn" onClick={createDirectory}>
            ＋ 新目录
          </button>
          <button className="btn primary" onClick={() => void chooseUpload()}>
            ↑ 上传文件
          </button>
          <input
            ref={fileInput}
            type="file"
            hidden
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) upload(file);
              event.currentTarget.value = "";
            }}
          />
        </div>
      </div>
      <div className="split-grid">
        <div className="card">
          <div className="breadcrumbs">
            <button className="btn" onClick={() => setPath("~")}>
              ⌂
            </button>
            {path !== "~" && (
              <button className="btn" onClick={() => setPath(parent)}>
                ←
              </button>
            )}{" "}
            / <b>{path}</b>
          </div>
          {shownFiles.map((file) => (
            <div className="file-row" key={file.name}>
              <button
                className="file-main"
                onClick={() =>
                  file.kind === "directory" &&
                  setPath(`${path.replace(/\/$/, "")}/${file.name}`)
                }
              >
                <span>{file.kind === "directory" ? "📁" : "📄"}</span>
                <b>{file.name}</b>
                <span className="muted">
                  {file.kind === "directory" ? "目录" : "远端文件"}
                </span>
              </button>
              {file.kind !== "directory" && (
                <div className="file-actions">
                  <button
                    className="btn"
                    onClick={() =>
                      session &&
                      void api
                        .readRemoteFile({
                          session_id: session.id,
                          path: `${path.replace(/\/$/, "")}/${file.name}`,
                        })
                        .then((response) =>
                          window.alert(
                            (response.data as { content?: string } | null)
                              ?.content ??
                              response.error?.message ??
                              "读取失败",
                          ),
                        )
                    }
                  >
                    读取
                  </button>
                  <button
                    className="btn"
                    onClick={() => void download(file.name)}
                  >
                    下载
                  </button>
                  <button
                    className="btn"
                    onClick={() => {
                      const next = window.prompt("重命名为", file.name);
                      if (next && next !== file.name)
                        void start(
                          {
                            op: "rename",
                            src: `${path.replace(/\/$/, "")}/${file.name}`,
                            dst: `${path.replace(/\/$/, "")}/${next}`,
                            confirmed: true,
                          },
                          next,
                        );
                    }}
                  >
                    重命名
                  </button>
                  <button
                    className="btn red"
                    onClick={() =>
                      window.confirm(`删除 ${file.name}？`) &&
                      void start(
                        {
                          op: "delete",
                          src: `${path.replace(/\/$/, "")}/${file.name}`,
                          confirmed: true,
                        },
                        file.name,
                      )
                    }
                  >
                    删除
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
        <div className="card">
          <h3>传输队列</h3>
          {transfers.length === 0 && <p className="muted">暂无传输任务</p>}
          {transfers.map((item) => (
            <div className="transfer-row" key={item.id}>
              <b>{item.name}</b>
              <span
                className={
                  "badge " + (item.status === "completed" ? "green" : "")
                }
              >
                {item.status}
              </span>
              <div className="progress">
                <i style={{ width: `${item.progress}%` }} />
              </div>
              <small className="muted">
                {item.status === "paused" && (
                  <button
                    className="btn"
                    onClick={() => void api.transferResume(item.id)}
                  >
                    继续
                  </button>
                )}
                <button
                  className="btn"
                  onClick={() => void api.transferPause(item.id)}
                >
                  暂停
                </button>
                <button
                  className="btn"
                  onClick={() => void api.transferCancel(item.id)}
                >
                  取消
                </button>
                <button
                  className="btn"
                  onClick={() => void api.transferRetry(item.id, true)}
                >
                  重试
                </button>
              </small>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function AuditPage() {
  const [events, setEvents] = useState<Array<Record<string, unknown>>>([]);
  const [exported, setExported] = useState("");
  const [verification, setVerification] = useState<boolean | null>(null);
  useEffect(() => {
    void api.auditList(200).then((response) => {
      const value = response.data as {
        events?: Array<Record<string, unknown>>;
      } | null;
      if (value?.events) setEvents(value.events);
    });
  }, []);
  const exportAudit = async () => {
    const response = await api.auditExport();
    const value = response.data as { path?: string } | null;
    if (value?.path) {
      setExported(value.path);
      const verify = await api.auditExportVerify(value.path);
      setVerification(
        Boolean((verify.data as { valid?: boolean } | null)?.valid),
      );
    } else if (!response.ok)
      window.alert(response.error?.message ?? "导出失败");
  };
  return (
    <section className="content">
      <div className="hero">
        <div>
          <h1>本地审计</h1>
          <p>SHA-256 追加哈希链 · JSONL 与 manifest 离线校验</p>
        </div>
        <button className="btn" onClick={() => void exportAudit()}>
          导出并校验 JSONL
        </button>
      </div>
      {exported && (
        <p className="muted">
          最近导出：{exported} · 校验 {verification ? "通过" : "失败"}
        </p>
      )}
      <div className="cards">
        <Metric
          label="链状态"
          value={verification === false ? "✕ 异常" : "✓ 连续"}
          hint="追加写入正常"
        />
        <Metric
          label="事件总数"
          value={String(events.length)}
          hint="当前数据库"
        />
        <Metric
          label="最近事件"
          value={events[0] ? String(events[0].created_at ?? "—") : "—"}
          hint="最新追加"
        />
        <Metric label="保留期限" value="90 天" hint="本地配置" />
      </div>
      <div className="card table-card">
        <table className="table">
          <thead>
            <tr>
              <th>时间</th>
              <th>事件</th>
              <th>目标</th>
              <th>操作者</th>
              <th>风险</th>
              <th>哈希</th>
            </tr>
          </thead>
          <tbody>
            {events.slice(0, 50).map((event, index) => (
              <tr key={`${String(event.event_id ?? index)}`}>
                <td>{String(event.created_at ?? "")}</td>
                <td>{String(event.event_type ?? "")}</td>
                <td>{String(event.target_host_id ?? "—")}</td>
                <td>{String(event.actor ?? "")}</td>
                <td>
                  <span className="badge">
                    {String(event.severity ?? "info")}
                  </span>
                </td>
                <td>{String(event.hash ?? "").slice(0, 12)}…</td>
              </tr>
            ))}
          </tbody>
        </table>
        {events.length === 0 && (
          <p className="muted">
            暂无审计事件。连接、审批和文件操作会在此显示。
          </p>
        )}
      </div>
    </section>
  );
}

function SettingsPage({
  theme,
  setTheme,
}: {
  theme: "dark" | "light";
  setTheme: (t: "dark" | "light") => void;
}) {
  const [policy, setPolicy] = useState("读取中…");
  useEffect(() => {
    void api.policyGet().then((response) => {
      const value = response.data as { mode?: string; version?: number } | null;
      if (value)
        setPolicy(
          `${value.mode ?? "ask_before_execute"} · v${value.version ?? 1}`,
        );
    });
  }, []);
  const backup = async () => {
    const path = window.prompt("输入备份 .db 的绝对路径");
    if (path)
      window.alert(
        (await api.databaseBackup(path)).ok ? "备份完成" : "备份失败",
      );
  };
  const restore = async () => {
    const path = window.prompt("输入要恢复的 .db 绝对路径");
    if (path && window.confirm("恢复会覆盖本地业务数据，确定继续？"))
      window.alert(
        (await api.databaseRestore(path, true)).ok
          ? "恢复完成，请重启应用"
          : "恢复失败",
      );
  };
  return (
    <section className="content">
      <div className="hero">
        <div>
          <h1>设置</h1>
          <p>个人版本地配置和安全策略。</p>
        </div>
        <div className="actions">
          <button className="btn" onClick={backup}>
            备份数据库
          </button>
          <button className="btn red" onClick={restore}>
            恢复数据库
          </button>
        </div>
      </div>
      <div className="settings-grid">
        <div className="card">
          <h3>模型配置</h3>
          <p className="setting-line">
            Provider <span className="badge">Ollama</span>
          </p>
          <p className="setting-line">
            配置 <code>%USERPROFILE%\.termpilot\config.toml</code>
          </p>
          <p className="muted">模型不可用时不影响 SSH/SFTP。</p>
        </div>
        <div className="card">
          <h3>执行策略</h3>
          <p className="setting-line">
            当前策略 <span className="badge yellow">{policy}</span>
          </p>
          <p className="setting-line">
            生产自动执行 <span className="badge red">关闭</span>
          </p>
          <p className="setting-line">
            固定只读 <span className="badge green">df -h · pwd · whoami</span>
          </p>
        </div>
        <div className="card">
          <h3>本地数据</h3>
          <p className="setting-line">
            数据库 <span className="ok-text">可写</span>
          </p>
          <p className="setting-line">
            审计保留 <b>90 天</b>
          </p>
          <p className="setting-line">
            远程默认目录 <code>~</code>
          </p>
        </div>
        <div className="card">
          <h3>外观与安全</h3>
          <p className="setting-line">
            主题{" "}
            <button
              className="btn"
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            >
              {theme === "dark" ? "切换浅色模式" : "切换深色模式"}
            </button>
          </p>
          <p className="muted">
            密码、私钥和 Token 不会写入 SQLite、日志或模型请求。
          </p>
        </div>
      </div>
    </section>
  );
}

function ShellSurface({
  light,
  sessionId,
  onStatus,
}: {
  light: boolean;
  sessionId?: string;
  onStatus?: (status: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    const term = new XTerminal({
      convertEol: true,
      cursorBlink: true,
      fontFamily: "Consolas, monospace",
      fontSize: 13,
      theme: {
        background: light ? "#ffffff" : "#080d18",
        foreground: light ? "#24344a" : "#cbd6ed",
        cursor: light ? "#24344a" : "#cbd6ed",
      },
    });
    activeXterm = term;
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    const resize = () => {
      fit.fit();
      if (sessionId)
        void api.sessionResize({
          session_id: sessionId,
          rows: term.rows,
          cols: term.cols,
        });
    };
    resize();
    term.writeln("TermPilot SSH session · output is not persisted");
    term.writeln(
      sessionId
        ? "\x1b[32m● Connected · fingerprint verified\x1b[0m"
        : "\x1b[33m● Mock terminal · connect a host to send input\x1b[0m",
    );
    term.writeln("");
    term.write("\x1b[94mops@remote:~$\x1b[0m ");
    const onData = term.onData((data) => {
      if (sessionId) {
        const bytes = btoa(unescape(encodeURIComponent(data)));
        void api.sessionSendInput({
          session_id: sessionId,
          bytes_base64: bytes,
        });
      } else if (data === "\r") term.write("\r\n\x1b[94mops@remote:~$\x1b[0m ");
    });
    const unlistenOutput = sessionId
      ? api.on<{
          session_id?: string;
          data?: { bytes_base64?: string };
          bytes_base64?: string;
        }>("session.output", (payload) => {
          if (payload.session_id && payload.session_id !== sessionId) return;
          const encoded = payload.data?.bytes_base64 ?? payload.bytes_base64;
          if (encoded) {
            try {
              term.write(decodeURIComponent(escape(atob(encoded))));
            } catch {
              /* malformed event */
            }
          }
        })
      : Promise.resolve(() => undefined);
    const unlistenStatus = sessionId
      ? api.on<{ session_id?: string; data?: { status?: string } }>(
          "session.status",
          (payload) => {
            if (payload.session_id === sessionId && payload.data?.status)
              onStatus?.(payload.data.status);
          },
        )
      : Promise.resolve(() => undefined);
    const observer = new ResizeObserver(resize);
    observer.observe(host.current);
    window.addEventListener("resize", resize);
    return () => {
      if (activeXterm === term) activeXterm = undefined;
      window.removeEventListener("resize", resize);
      observer.disconnect();
      onData.dispose();
      void unlistenOutput.then((fn) => fn());
      void unlistenStatus.then((fn) => fn());
      term.dispose();
    };
  }, [light, sessionId, onStatus]);
  return <div className="xterm-host" ref={host} />;
}

function Terminal({
  hosts,
  session,
  sessionTabs,
  light,
  open,
  setOpen,
  connect,
  setSession,
  setSessionTabs,
}: {
  hosts: Host[];
  session?: Session;
  sessionTabs: Session[];
  light: boolean;
  open: boolean;
  setOpen: (v: boolean) => void;
  connect: (h: Host) => void;
  setSession: (s?: Session) => void;
  setSessionTabs: React.Dispatch<React.SetStateAction<Session[]>>;
}) {
  const [agentWidth, setAgentWidth] = useState(380);
  const [draft, setDraft] = useState("");
  const [shellDraft, setShellDraft] = useState("");
  const [searchDraft, setSearchDraft] = useState("");
  const [mode, setMode] = useState("ask_before_execute");
  const [status, setStatus] = useState(session?.status ?? "disconnected");
  const [messages, setMessages] = useState<string[]>([
    "连接后终端输出不会写入数据库。",
    "我会先执行安全的只读诊断。",
  ]);
  const [approvals, setApprovals] = useState<
    Array<{ id: string; risk: string }>
  >([]);
  const [activeTaskId, setActiveTaskId] = useState<string>();
  const agentBodyRef = useRef<HTMLDivElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const currentHost = hosts.find((h) => h.id === session?.host_id) ?? hosts[0];
  useEffect(() => {
    if (agentBodyRef.current)
      agentBodyRef.current.scrollTop = agentBodyRef.current.scrollHeight;
  }, [messages, approvals]);
  useEffect(() => {
    setStatus(session?.status ?? "disconnected");
    if (!session) return;
    setSessionTabs((tabs) =>
      tabs.some((tab) => tab.id === session.id)
        ? tabs.map((tab) => (tab.id === session.id ? session : tab))
        : [...tabs, session],
    );
  }, [session]);
  useEffect(() => {
    const listener = api.on<{ data?: { approval_id?: string; risk?: string } }>(
      "approval.created",
      (payload) => {
        if (payload.data?.approval_id)
          setApprovals((items) => [
            ...items,
            {
              id: payload.data.approval_id ?? "",
              risk: payload.data.risk ?? "medium",
            },
          ]);
      },
    );
    const agentListener = api.on<{
      session_id?: string;
      data?: { task_id?: string; status?: string; delta?: string };
    }>("agent.delta", async (payload) => {
      if (payload.session_id && payload.session_id !== session?.id) return;
      const delta = payload.data?.delta;
      if (!delta) return;
      setActiveTaskId(undefined);
      setMessages((items) => [...items, delta]);
      const toolCall = parseAgentToolCall(delta);
      if (!toolCall || !session) return;
      setMessages((items) => [...items, `正在调用工具：${toolCall.tool}`]);
      const toolResult = await api.agentToolDispatch(toolCall, session.id);
      const resultText = toolResult.ok
        ? (JSON.stringify(toolResult.data) ?? "工具已完成")
        : (toolResult.error?.message ?? "工具调用失败");
      setMessages((items) => [...items, `${toolCall.tool}：${resultText}`]);
    });
    return () => {
      void listener.then((unlisten) => unlisten());
      void agentListener.then((unlisten) => unlisten());
    };
  }, [session?.id]);
  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    const move = (ev: PointerEvent) => {
      const rect = shellRef.current?.getBoundingClientRect();
      if (rect)
        setAgentWidth(Math.max(260, Math.min(620, rect.right - ev.clientX)));
    };
    const stop = () => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", stop);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", stop);
  };
  const sendShell = () => {
    const value = shellDraft.trim();
    if (!value || !session) return;
    const bytes = btoa(unescape(encodeURIComponent(`${value}\n`)));
    void api.sessionSendInput({ session_id: session.id, bytes_base64: bytes });
    setShellDraft("");
  };
  const runSearch = () => {
    const query = searchDraft.trim().toLowerCase();
    if (!query || !activeXterm) return;
    const buffer = activeXterm.buffer.active;
    for (let line = 0; line < buffer.length; line += 1) {
      const text =
        buffer.getLine(line)?.translateToString(true).toLowerCase() ?? "";
      if (text.includes(query)) {
        activeXterm.scrollToLine(line);
        return;
      }
    }
  };
  const copySelection = async () => {
    const selected = activeXterm?.getSelection();
    if (selected) await navigator.clipboard?.writeText(selected);
  };
  const sendAgent = async () => {
    const value = draft.trim();
    if (!value) return;
    setMessages((items) => [...items, value]);
    setDraft("");
    const response = await api.agentMessageSend({
      session_id: session?.id ?? "mock-session",
      text: value,
      mode,
      client_request_id: crypto.randomUUID(),
    });
    const data = response.data as {
      response?: string;
      task_id?: string;
      status?: string;
    } | null;
    if (!response.ok) {
      setMessages((items) => [
        ...items,
        response.error?.message ?? "Agent 暂时不可用",
      ]);
      return;
    }
    const taskId = data?.task_id;
    if (data?.status === "active" && taskId) {
      setActiveTaskId(taskId);
      setMessages((items) => [...items, "Agent 正在处理…"]);
      return;
    }
    const responseText = data?.response ?? "Agent 暂时不可用";
    setMessages((items) => [...items, responseText]);
    const toolCall = parseAgentToolCall(responseText);
    if (!toolCall) return;
    if (!session) {
      setMessages((items) => [...items, "工具调用需要先连接远程会话。"]);
      return;
    }
    setMessages((items) => [...items, `正在调用工具：${toolCall.tool}`]);
    const toolResult = await api.agentToolDispatch(toolCall, session.id);
    const resultText = toolResult.ok
      ? (JSON.stringify(toolResult.data) ?? "工具已完成")
      : (toolResult.error?.message ?? "工具调用失败");
    setMessages((items) => [...items, `${toolCall.tool}：${resultText}`]);
  };
  const cancelAgent = async () => {
    if (!activeTaskId) return;
    await api.agentCancel(activeTaskId, "用户取消");
    setMessages((items) => [...items, "已发送 Agent 取消请求。"]);
  };
  const decide = async (id: string, decision: "approve" | "reject") => {
    const response = await api.approvalDecide(id, decision);
    if (response.ok)
      setApprovals((items) => items.filter((item) => item.id !== id));
    else window.alert(response.error?.message ?? "审批失败");
  };
  return (
    <section className="content terminal-content">
      <div className="hero">
        <div>
          <h1>远程终端</h1>
          <p>SSH PTY 会话 · Shell 与 Agent 分栏 · {status}</p>
        </div>
        <div className="actions">
          <button
            className="btn primary"
            onClick={() => currentHost && connect(currentHost)}
          >
            ＋ 连接主机
          </button>
          <button
            className="btn red"
            onClick={() =>
              session &&
              void api
                .sessionDisconnect({ session_id: session.id, reason: "user" })
                .then(() => {
                  const remaining = sessionTabs.filter(
                    (tab) => tab.id !== session.id,
                  );
                  setSessionTabs(remaining);
                  setSession(remaining.at(-1));
                })
            }
          >
            断开当前会话
          </button>
        </div>
      </div>
      <div
        ref={shellRef}
        className={"term-shell " + (!open ? "agent-hidden" : "")}
        style={{
          gridTemplateColumns: open ? `minmax(0,1fr) ${agentWidth}px` : "1fr",
        }}
      >
        <section className="shell-pane">
          <div className="term-tabs">
            <div className="session-tabs" role="tablist" aria-label="终端会话">
              {sessionTabs.map((tab) => {
                const host = hosts.find((item) => item.id === tab.host_id);
                const selected = tab.id === session?.id;
                return (
                  <button
                    className={"tab " + (selected ? "active" : "")}
                    key={tab.id}
                    role="tab"
                    aria-selected={selected}
                    onClick={() => setSession(tab)}
                    title={`${host?.name ?? tab.host_id} · ${tab.status}`}
                  >
                    {host?.name ?? tab.host_id} · {tab.status}
                  </button>
                );
              })}
              {!sessionTabs.length && (
                <span className="tab active">未连接 · disconnected</span>
              )}
            </div>
            <input
              className="term-search"
              value={searchDraft}
              onChange={(event) => setSearchDraft(event.target.value)}
              onKeyDown={(event) => event.key === "Enter" && runSearch()}
              placeholder="搜索"
            />
            <button
              className="btn"
              onClick={() => void copySelection()}
              title="复制选区"
            >
              复制
            </button>
            <button
              className="btn plus"
              title="新建连接"
              onClick={() => currentHost && connect(currentHost)}
            >
              ＋
            </button>
            <button
              className="btn agent-toggle on"
              onClick={() => setOpen(!open)}
            >
              ✦ Agent
            </button>
          </div>
          <ShellSurface
            light={light}
            sessionId={session?.id}
            onStatus={setStatus}
          />
          <div className="shell-input">
            <span>
              {currentHost?.username ?? "ops"}@{currentHost?.name ?? "remote"}
              :~$
            </span>
            <input
              value={shellDraft}
              onChange={(event) => setShellDraft(event.target.value)}
              onKeyDown={(event) => event.key === "Enter" && sendShell()}
              placeholder="输入命令（直接发送到远端 Shell）"
            />
          </div>
        </section>
        {open && (
          <>
            <aside className="agent-pane">
              <div className="agent-head">
                <strong>● Agent 助手</strong>
                {activeTaskId && (
                  <button
                    className="btn red"
                    onClick={() => void cancelAgent()}
                  >
                    取消
                  </button>
                )}
                <select
                  value={mode}
                  onChange={(event) => setMode(event.target.value)}
                >
                  <option value="ask_before_execute">需审批</option>
                  <option value="readonly">只读</option>
                  <option value="manual_only">仅建议</option>
                </select>
                <button className="btn" onClick={() => setOpen(false)}>
                  ×
                </button>
              </div>
              <div className="agent-body" ref={agentBodyRef}>
                {messages.map((message, index) => (
                  <div
                    className={"agent-msg " + (index % 2 === 1 ? "user" : "")}
                    key={`${message}-${index}`}
                  >
                    {message}
                  </div>
                ))}
                {approvals.map((item) => (
                  <div className="agent-msg approval" key={item.id}>
                    <b>待审批 · {item.risk}</b>
                    <p className="muted">Agent 请求执行结构化命令</p>
                    <button
                      className="btn primary"
                      onClick={() => void decide(item.id, "approve")}
                    >
                      批准
                    </button>
                    <button
                      className="btn"
                      onClick={() => void decide(item.id, "reject")}
                    >
                      拒绝
                    </button>
                  </div>
                ))}
              </div>
              <div className="agent-compose">
                <input
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) =>
                    event.key === "Enter" && void sendAgent()
                  }
                  placeholder="询问 Agent…"
                />
                <button
                  className="btn primary"
                  onClick={() => void sendAgent()}
                >
                  ↑
                </button>
              </div>
            </aside>
            <div
              className="resizer"
              style={{ left: "auto", right: `${agentWidth - 4}px` }}
              onPointerDown={startResize}
            />
          </>
        )}
      </div>
    </section>
  );
}
