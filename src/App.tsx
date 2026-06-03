import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Clipboard,
  FileText,
  Pencil,
  Play,
  Plus,
  Power,
  RefreshCw,
  Square,
  Trash2,
  X
} from "lucide-react";
import { api, copyText } from "./api";
import { defaultProfile, GatewayProfile, LogEntry, ProfileStatus, ServiceStatusKind } from "./types";

const featureLabels: Record<keyof GatewayProfile["features"], string> = {
  normalizeAdaptiveThinking: "Adaptive thinking",
  mapEffort: "Effort mapping",
  preserveSse: "SSE streaming",
  smoothStreamingText: "Smooth text",
  reasoningReplay: "Reasoning replay",
  oneMContextDefaults: "1M defaults",
  redactSensitiveLogs: "Log redaction"
};

const copyTargets = [
  ["anthropicUrl", "Anthropic URL"],
  ["openaiUrl", "OpenAI URL"],
  ["claudeUnix", "Claude env"],
  ["claudeWindows", "PowerShell env"]
] as const;

export default function App() {
  const [profiles, setProfiles] = useState<GatewayProfile[]>([]);
  const [statuses, setStatuses] = useState<Record<string, ProfileStatus>>({});
  const [editing, setEditing] = useState<GatewayProfile | null>(null);
  const [logsFor, setLogsFor] = useState<GatewayProfile | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<string>("");
  const [error, setError] = useState<string>("");

  const anyRunning = useMemo(
    () => Object.values(statuses).some((status) => status.status === "running"),
    [statuses]
  );

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => {
      void refreshStatuses();
      if (logsFor) void loadLogs(logsFor);
    }, 2000);
    return () => window.clearInterval(id);
  }, [logsFor]);

  async function refresh() {
    try {
      setError("");
      const [nextProfiles, nextStatuses] = await Promise.all([api.listProfiles(), api.statuses()]);
      setProfiles(nextProfiles);
      setStatuses(indexStatuses(nextStatuses));
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshStatuses() {
    try {
      setStatuses(indexStatuses(await api.statuses()));
    } catch (err) {
      setError(String(err));
    }
  }

  async function runAction(label: string, fn: () => Promise<void>) {
    setBusy(label);
    setError("");
    try {
      await fn();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function start(profile: GatewayProfile) {
    await runAction(`start-${profile.id}`, async () => {
      const status = await api.startProfile(profile.id);
      setStatuses((current) => ({ ...current, [profile.id]: status }));
      setToast(`${profile.name} started`);
    });
  }

  async function stop(profile: GatewayProfile) {
    await runAction(`stop-${profile.id}`, async () => {
      const status = await api.stopProfile(profile.id);
      setStatuses((current) => ({ ...current, [profile.id]: status }));
      setToast(`${profile.name} stopped`);
    });
  }

  async function save(profile: GatewayProfile) {
    await runAction("save", async () => {
      const next = await api.saveProfile(profile);
      setProfiles(next);
      setEditing(null);
      await refreshStatuses();
      setToast("Saved");
    });
  }

  async function remove(profile: GatewayProfile) {
    await runAction(`delete-${profile.id}`, async () => {
      const next = await api.deleteProfile(profile.id);
      setProfiles(next);
      setStatuses((current) => {
        const copy = { ...current };
        delete copy[profile.id];
        return copy;
      });
      setEditing(null);
      setLogsFor(null);
      setToast("Deleted");
    });
  }

  async function copy(profile: GatewayProfile, kind: string) {
    await runAction(`copy-${profile.id}-${kind}`, async () => {
      const text = await api.copyProxyText(profile.id, kind);
      await copyText(text);
      setToast("Copied");
    });
  }

  async function loadLogs(profile: GatewayProfile) {
    try {
      setLogs(await api.readLogs(profile.id, 500));
    } catch (err) {
      setError(String(err));
    }
  }

  async function openLogs(profile: GatewayProfile) {
    setLogsFor(profile);
    await loadLogs(profile);
  }

  const nextPort = 17777 + profiles.length;

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <h1>DeepSeek Gateway</h1>
          <p>Local proxy manager for agent clients.</p>
        </div>
        <div className="topbar-actions">
          <button className="secondary" onClick={() => void refresh()} disabled={busy !== null}>
            <RefreshCw size={16} /> Refresh
          </button>
          <button
            className="secondary"
            onClick={() => void runAction("stop-all", async () => setStatuses(indexStatuses(await api.stopAll())))}
            disabled={!anyRunning || busy !== null}
          >
            <Square size={16} /> Stop all
          </button>
          <button
            className="primary"
            onClick={() => void runAction("start-all", async () => setStatuses(indexStatuses(await api.startAll())))}
            disabled={profiles.length === 0 || busy !== null}
          >
            <Power size={16} /> Start all
          </button>
          <button className="primary" onClick={() => setEditing(defaultProfile(nextPort))}>
            <Plus size={16} /> Add
          </button>
        </div>
      </header>

      {error && (
        <div className="notice error" role="alert">
          {error}
        </div>
      )}
      {toast && (
        <button className="notice toast" onClick={() => setToast("")}>
          {toast}
        </button>
      )}

      <section className="profile-grid" aria-label="Gateway profiles">
        {profiles.length === 0 ? (
          <div className="empty">
            <Activity size={32} />
            <h2>No profiles</h2>
            <button className="primary" onClick={() => setEditing(defaultProfile(nextPort))}>
              <Plus size={16} /> Add profile
            </button>
          </div>
        ) : (
          profiles.map((profile) => (
            <ProfileCard
              key={profile.id}
              profile={profile}
              status={statuses[profile.id]}
              busy={busy}
              onStart={() => void start(profile)}
              onStop={() => void stop(profile)}
              onEdit={() => setEditing(profile)}
              onLogs={() => void openLogs(profile)}
              onCopy={(kind) => void copy(profile, kind)}
            />
          ))
        )}
      </section>

      {editing && (
        <ProfileEditor
          profile={editing}
          status={statuses[editing.id]}
          busy={busy}
          onClose={() => setEditing(null)}
          onSave={(profile) => void save(profile)}
          onDelete={editing.id ? () => void remove(editing) : undefined}
        />
      )}

      {logsFor && (
        <LogPanel
          profile={logsFor}
          logs={logs}
          onClose={() => setLogsFor(null)}
          onRefresh={() => void loadLogs(logsFor)}
          onClear={() =>
            void runAction("clear-logs", async () => {
              await api.clearLogs(logsFor.id);
              setLogs([]);
            })
          }
        />
      )}
    </main>
  );
}

function ProfileCard({
  profile,
  status,
  busy,
  onStart,
  onStop,
  onEdit,
  onLogs,
  onCopy
}: {
  profile: GatewayProfile;
  status?: ProfileStatus;
  busy: string | null;
  onStart: () => void;
  onStop: () => void;
  onEdit: () => void;
  onLogs: () => void;
  onCopy: (kind: string) => void;
}) {
  const kind = status?.status ?? "stopped";
  const running = kind === "running";
  const enabled = Object.entries(profile.features)
    .filter(([, value]) => value)
    .map(([key]) => featureLabels[key as keyof GatewayProfile["features"]]);

  return (
    <article className="profile-card">
      <div className="card-head">
        <div>
          <h2>{profile.name}</h2>
          <span className="mono">127.0.0.1:{profile.port}</span>
        </div>
        <StatusPill status={kind} />
      </div>
      <div className="surface-row">
        {profile.enabledSurfaces.map((surface) => (
          <span key={surface}>{surface === "openAi" ? "OpenAI" : "Anthropic"}</span>
        ))}
      </div>
      <div className="feature-list">
        {enabled.slice(0, 4).map((feature) => (
          <span key={feature}>{feature}</span>
        ))}
      </div>
      <dl className="metrics">
        <div>
          <dt>Requests</dt>
          <dd>{status?.requestCount ?? 0}</dd>
        </div>
        <div>
          <dt>Upstream</dt>
          <dd>{profile.upstreamBaseUrl.replace(/^https?:\/\//, "")}</dd>
        </div>
      </dl>
      {status?.lastError && <p className="last-error">{status.lastError}</p>}
      <div className="button-row">
        {running ? (
          <button onClick={onStop} disabled={busy !== null}>
            <Square size={16} /> Stop
          </button>
        ) : (
          <button className="primary" onClick={onStart} disabled={busy !== null}>
            <Play size={16} /> Start
          </button>
        )}
        <button onClick={onLogs}>
          <FileText size={16} /> Logs
        </button>
        <button onClick={onEdit}>
          <Pencil size={16} /> Edit
        </button>
      </div>
      <div className="copy-grid">
        {copyTargets.map(([kind, label]) => (
          <button key={kind} className="ghost" onClick={() => onCopy(kind)} disabled={busy !== null}>
            <Clipboard size={15} /> {label}
          </button>
        ))}
      </div>
    </article>
  );
}

function ProfileEditor({
  profile,
  status,
  busy,
  onClose,
  onSave,
  onDelete
}: {
  profile: GatewayProfile;
  status?: ProfileStatus;
  busy: string | null;
  onClose: () => void;
  onSave: (profile: GatewayProfile) => void;
  onDelete?: () => void;
}) {
  const [draft, setDraft] = useState(profile);
  const running = status?.status === "running";
  const canDelete = Boolean(onDelete);
  const set = <K extends keyof GatewayProfile>(key: K, value: GatewayProfile[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  return (
    <div className="modal-backdrop">
      <form
        className="modal"
        onSubmit={(event) => {
          event.preventDefault();
          onSave(draft);
        }}
      >
        <div className="modal-head">
          <h2>{draft.id ? "Edit profile" : "Add profile"}</h2>
          <button type="button" className="icon" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>

        <div className="form-grid">
          <label>
            Name
            <input value={draft.name} onChange={(event) => set("name", event.target.value)} required />
          </label>
          <label>
            Port
            <input
              type="number"
              min={1024}
              max={65535}
              value={draft.port}
              disabled={running}
              onChange={(event) => set("port", Number(event.target.value))}
              required
            />
          </label>
          <label className="span-2">
            Upstream
            <input
              value={draft.upstreamBaseUrl}
              onChange={(event) => set("upstreamBaseUrl", event.target.value)}
              required
            />
          </label>
          <label>
            Timeout
            <input
              type="number"
              min={1}
              max={600}
              value={draft.timeoutSeconds}
              onChange={(event) => set("timeoutSeconds", Number(event.target.value))}
              required
            />
          </label>
          <label>
            Log level
            <select value={draft.logLevel} onChange={(event) => set("logLevel", event.target.value as GatewayProfile["logLevel"])}>
              <option value="error">Error</option>
              <option value="info">Info</option>
              <option value="debug">Debug</option>
            </select>
          </label>
        </div>

        <fieldset>
          <legend>Surfaces</legend>
          {(["anthropic", "openAi"] as const).map((surface) => (
            <label className="check" key={surface}>
              <input
                type="checkbox"
                checked={draft.enabledSurfaces.includes(surface)}
                onChange={(event) => {
                  const next = event.target.checked
                    ? [...draft.enabledSurfaces, surface]
                    : draft.enabledSurfaces.filter((item) => item !== surface);
                  set("enabledSurfaces", next);
                }}
              />
              {surface === "openAi" ? "OpenAI" : "Anthropic"}
            </label>
          ))}
        </fieldset>

        <fieldset>
          <legend>Models</legend>
          <div className="form-grid">
            {(Object.keys(draft.modelMapping) as Array<keyof GatewayProfile["modelMapping"]>).map((key) => (
              <label key={key}>
                {key}
                <input
                  value={draft.modelMapping[key]}
                  onChange={(event) =>
                    setDraft((current) => ({
                      ...current,
                      modelMapping: { ...current.modelMapping, [key]: event.target.value }
                    }))
                  }
                />
              </label>
            ))}
          </div>
        </fieldset>

        <fieldset>
          <legend>Features</legend>
          <div className="checks">
            {(Object.keys(draft.features) as Array<keyof GatewayProfile["features"]>).map((key) => (
              <label className="check" key={key}>
                <input
                  type="checkbox"
                  checked={draft.features[key]}
                  onChange={(event) =>
                    setDraft((current) => ({
                      ...current,
                      features: { ...current.features, [key]: event.target.checked }
                    }))
                  }
                />
                {featureLabels[key]}
              </label>
            ))}
          </div>
        </fieldset>

        <div className="modal-actions">
          {canDelete && (
            <button type="button" className="danger" onClick={onDelete} disabled={busy !== null}>
              <Trash2 size={16} /> Delete
            </button>
          )}
          <span />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={busy !== null}>
            Save
          </button>
        </div>
      </form>
    </div>
  );
}

function LogPanel({
  profile,
  logs,
  onClose,
  onRefresh,
  onClear
}: {
  profile: GatewayProfile;
  logs: LogEntry[];
  onClose: () => void;
  onRefresh: () => void;
  onClear: () => void;
}) {
  return (
    <aside className="log-panel">
      <div className="log-head">
        <div>
          <h2>{profile.name} logs</h2>
          <span className="mono">127.0.0.1:{profile.port}</span>
        </div>
        <div className="button-row">
          <button onClick={onRefresh}>
            <RefreshCw size={16} /> Refresh
          </button>
          <button onClick={onClear}>Clear</button>
          <button className="icon" onClick={onClose} aria-label="Close logs">
            <X size={18} />
          </button>
        </div>
      </div>
      <div className="log-list">
        {logs.length === 0 ? (
          <p className="muted">No log entries</p>
        ) : (
          logs.map((entry, index) => (
            <div className="log-entry" key={`${entry.timestamp}-${index}`}>
              <span>{new Date(entry.timestamp).toLocaleTimeString()}</span>
              <strong>{entry.level}</strong>
              <code>{entry.requestId ?? "system"}</code>
              <p>{entry.message}</p>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}

function StatusPill({ status }: { status: ServiceStatusKind }) {
  return <span className={`status ${status}`}>{status}</span>;
}

function indexStatuses(statuses: ProfileStatus[]) {
  return Object.fromEntries(statuses.map((status) => [status.id, status]));
}
