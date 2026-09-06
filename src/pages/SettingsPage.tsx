import { useCallback, useEffect, useState, type ReactNode } from "react";
import { FolderPlus, Trash2, Copy } from "lucide-react";
import { Button } from "../components/Button";
import { Input } from "../components/Input";
import { PasswordInput } from "../components/PasswordInput";
import { Select } from "../components/Select";
import { api, invokeErrorMessage, isTauri, isTimeoutError, pickFolder } from "../lib/api";
import { useApp } from "../context/AppContext";
import { StatusMessage } from "../components/StatusMessage";
import type { ActiveSession, EngineInfo, HouseholdUser, LibraryRoot, Series } from "../types";
import packageInfo from "../../package.json";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b border-border py-6 last:border-b-0">
      <h2 className="mb-3 text-[11px] font-medium uppercase tracking-[0.16em] text-muted">{title}</h2>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1.5 py-1.5">
      <span className="shrink-0 text-sm text-muted">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-border bg-surface px-1.5 py-0.5 font-mono text-[11px] text-text">
      {children}
    </kbd>
  );
}

const SHORTCUTS: { keys: string; action: string }[] = [
  { keys: "Arrows · PgUp/Dn · Space", action: "Page" },
  { keys: "Home / End", action: "Jump" },
  { keys: "Esc", action: "Close" },
  { keys: "F", action: "Fullscreen" },
  { keys: "T", action: "Translation" },
  { keys: "V / Shift+V", action: "Version" },
];

export function SettingsPage() {
  const { settings, refreshSettings, openWhatsNew } = useApp();
  const [roots, setRoots] = useState<LibraryRoot[]>([]);
  const [ocrEngines, setOcrEngines] = useState<EngineInfo[]>([]);
  const [translators, setTranslators] = useState<EngineInfo[]>([]);
  const [users, setUsers] = useState<HouseholdUser[]>([]);
  const [sessions, setSessions] = useState<ActiveSession[]>([]);
  const [series, setSeries] = useState<Series[]>([]);
  const [ownerPassword, setOwnerPassword] = useState("");
  const [remoteHostname, setRemoteHostname] = useState("isaach2910.dpdns.org");
  const [tunnelToken, setTunnelToken] = useState("");
  const [newUsername, setNewUsername] = useState("");
  const [newDisplayName, setNewDisplayName] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [newAccessAll, setNewAccessAll] = useState(false);
  const [newGrants, setNewGrants] = useState<number[]>([]);
  const [cacheSizeMb, setCacheSizeMb] = useState(512);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const availableOcrEngines = ocrEngines.filter((engine) => engine.available);
  const availableTranslators = translators.filter((translator) => translator.available);

  const loadPage = useCallback(async () => {
    if (!isTauri()) return;
    setLoading(true);
    setError(null);
    const [rootsRes, enginesRes, translatorsRes, usersRes, sessionsRes, seriesRes] = await Promise.allSettled([
      api.listLibraryRoots(),
      api.listOcrEngines(),
      api.listTranslators(),
      api.listUsers(),
      api.listSessions(),
      api.listSeries(),
    ]);

    if (rootsRes.status === "fulfilled") setRoots(rootsRes.value);
    else setRoots([]);

    if (enginesRes.status === "fulfilled") setOcrEngines(enginesRes.value);
    else setOcrEngines([]);

    if (translatorsRes.status === "fulfilled") setTranslators(translatorsRes.value);
    else setTranslators([]);

    if (usersRes.status === "fulfilled") setUsers(usersRes.value);
    else setUsers([]);

    if (sessionsRes.status === "fulfilled") setSessions(sessionsRes.value);
    else setSessions([]);

    if (seriesRes.status === "fulfilled") setSeries(seriesRes.value);
    else setSeries([]);

    const failed = [rootsRes, enginesRes, translatorsRes, usersRes, sessionsRes].some((r) => r.status === "rejected");
    if (failed) {
      setError("Some settings failed to load. You can retry.");
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void loadPage();
  }, [loadPage]);

  useEffect(() => {
    if (settings?.cacheSizeMb) setCacheSizeMb(settings.cacheSizeMb);
  }, [settings?.cacheSizeMb]);

  useEffect(() => {
    if (settings?.remote.configuredHostname) {
      setRemoteHostname(settings.remote.configuredHostname);
    }
  }, [settings?.remote.configuredHostname]);

  useEffect(() => {
    if (!isTauri()) return;
    if (!settings?.remote.enabled && !settings?.remote.running) return;
    const id = window.setInterval(() => {
      void refreshSettings();
    }, 1500);
    return () => window.clearInterval(id);
  }, [settings?.remote.enabled, settings?.remote.running, refreshSettings]);

  if (!isTauri()) {
    return (
      <div className="p-8 text-sm text-muted">
        Settings live on the Mac. This device is a thin reader.
      </div>
    );
  }

  const addRoot = async () => {
    setBusyAction("add-root");
    setNotice(null);
    setError(null);
    try {
      const path = await pickFolder();
      if (!path) return;
      await api.addLibraryRoot(path);
      setRoots(await api.listLibraryRoots());
      setNotice("Folder added.");
    } catch {
      setError("Could not add folder.");
    } finally {
      setBusyAction(null);
    }
  };

  const persistCloud = async () => {
    const password = ownerPassword.trim();
    const hostname = remoteHostname.trim() || settings?.remote.configuredHostname || "";
    const token = tunnelToken.trim();
    if (password && password.length < 8) {
      throw new Error("Password must be at least 8 characters.");
    }
    if (password) {
      await api.setOwnerPassword(password);
      setOwnerPassword("");
    }
    if (hostname) {
      await api.setRemoteCredentials(hostname, token || undefined);
      setTunnelToken("");
    }
    await refreshSettings();
  };

  const saveCloud = async () => {
    if (!ownerPassword.trim() && !remoteHostname.trim() && !tunnelToken.trim()) {
      setRemoteError("Nothing to save.");
      return false;
    }
    setBusyAction("credentials");
    setNotice(null);
    setError(null);
    setRemoteError(null);
    try {
      await persistCloud();
      setNotice("Saved.");
      return true;
    } catch (error) {
      const message = invokeErrorMessage(error, "Could not save.");
      setRemoteError(message);
      setError(message);
      return false;
    } finally {
      setBusyAction(null);
    }
  };

  const toggleRemote = async () => {
    const enabling = !settings?.remote.enabled;
    setBusyAction("remote");
    setNotice(null);
    setError(null);
    setRemoteError(null);
    try {
      if (enabling) {
        if (!(settings?.remoteLoginReady || ownerPassword.trim().length >= 8)) {
          setRemoteError("Set a password first.");
          return;
        }
        if (!(settings?.remote.configuredHostname || remoteHostname.trim())) {
          setRemoteError("Set a hostname first.");
          return;
        }
        if (!(settings?.remote.hasToken || tunnelToken.trim())) {
          setRemoteError("Set a token first.");
          return;
        }
        await persistCloud();
      }
      await api.setRemoteConfig(enabling);
      await refreshSettings();
      setNotice(enabling ? "Enabled." : "Disabled.");
    } catch (error) {
      const message = isTimeoutError(error)
        ? "Timed out."
        : invokeErrorMessage(error, "Could not update cloud access.");
      setRemoteError(message);
      setError(message);
      await refreshSettings();
    } finally {
      setBusyAction(null);
    }
  };

  const cloudReady =
    (settings?.remoteLoginReady || ownerPassword.trim().length >= 8) &&
    !!(settings?.remote.configuredHostname || remoteHostname.trim()) &&
    !!(settings?.remote.hasToken || tunnelToken.trim());
  const cloudStatus = !settings?.remote.cloudflaredInstalled
    ? "Install cloudflared to enable."
    : settings?.remote.enabled && settings.remote.status && settings.remote.status !== "stopped"
      ? `${settings.remote.status}${settings.remote.restartCount ? ` · ${settings.remote.restartCount}` : ""}`
      : null;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="border-b border-border px-4 py-4 md:px-8">
        <h1 className="text-xl font-semibold">Settings</h1>
      </header>
      <div className="mx-auto w-full max-w-xl flex-1 overflow-y-auto px-4 py-2 md:px-8">
        {error && (
          <StatusMessage
            tone="error"
            live
            className="mt-4"
            action={
              <Button variant="ghost" size="sm" className="px-0 text-accent hover:bg-transparent hover:text-accent-hover" onClick={() => void loadPage()}>
                Retry
              </Button>
            }
          >
            {error}
          </StatusMessage>
        )}
        {notice && (
          <StatusMessage tone="success" live className="mt-4">
            {notice}
          </StatusMessage>
        )}
        {loading && <StatusMessage className="mt-4">Loading…</StatusMessage>}

        <Section title="Library">
          <div className="space-y-1">
            {roots.map((root) => (
              <div key={root.id} className="flex items-center justify-between gap-3 py-1.5">
                <span className="truncate text-sm">{root.path}</span>
                <Button
                  onClick={async () => {
                    setBusyAction(`remove-root-${root.id}`);
                    setNotice(null);
                    setError(null);
                    try {
                      await api.removeLibraryRoot(root.id);
                      setRoots(await api.listLibraryRoots());
                      setNotice("Folder removed.");
                    } catch {
                      setError("Could not remove folder.");
                    } finally {
                      setBusyAction(null);
                    }
                  }}
                  disabled={busyAction === `remove-root-${root.id}`}
                  variant="ghost"
                  size="sm"
                  className="p-1.5 hover:bg-transparent hover:text-red-400"
                  aria-label={`Remove ${root.path}`}
                >
                  <Trash2 size={14} />
                </Button>
              </div>
            ))}
            {roots.length === 0 && !loading && (
              <p className="py-1.5 text-sm text-muted">No folders yet.</p>
            )}
          </div>
          <div className="mt-2 flex gap-2">
            <Button onClick={addRoot} disabled={busyAction === "add-root"} variant="primary" size="sm">
              <FolderPlus size={14} />
              {busyAction === "add-root" ? "Adding…" : "Add folder"}
            </Button>
            <Button
              onClick={async () => {
                setBusyAction("rescan");
                setNotice(null);
                setError(null);
                try {
                  await api.rescanLibrary();
                  setNotice("Rescan started.");
                } catch {
                  setError("Could not start rescan.");
                } finally {
                  setBusyAction(null);
                }
              }}
              disabled={busyAction === "rescan"}
              variant="secondary"
              size="sm"
            >
              {busyAction === "rescan" ? "Starting…" : "Rescan"}
            </Button>
          </div>
        </Section>

        <Section title="Reader">
          <Row label="Cache">
            <div className="flex items-center gap-2">
              <span className="shrink-0 whitespace-nowrap text-xs text-muted">{settings?.cacheUsedMb ?? 0} / {settings?.cacheSizeMb ?? 512} MB</span>
              <Input
                type="number"
                min={64}
                max={8192}
                aria-label="Page cache size in MB"
                value={cacheSizeMb}
                onChange={(e) => setCacheSizeMb(parseInt(e.target.value, 10) || 0)}
                className="w-20 bg-surface px-2 py-1"
              />
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  if (cacheSizeMb < 64 || cacheSizeMb > 8192) {
                    setError("Cache size must be between 64 MB and 8192 MB.");
                    return;
                  }
                  setBusyAction("cache-size");
                  setNotice(null);
                  setError(null);
                  api
                    .updateAppSettings(cacheSizeMb)
                    .then(refreshSettings)
                    .then(() => setNotice("Cache size updated."))
                    .catch((error) => setError(isTimeoutError(error) ? "Updating cache size timed out." : "Could not update cache size."))
                    .finally(() => setBusyAction(null));
                }}
                disabled={busyAction === "cache-size"}
              >
                {busyAction === "cache-size" ? "Saving…" : "Save"}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="px-0 text-red-400 hover:bg-transparent hover:text-red-300"
                onClick={() => {
                  setBusyAction("clear-cache");
                  setNotice(null);
                  setError(null);
                  api
                    .clearCache()
                    .then(refreshSettings)
                    .then(() => setNotice("Cache cleared."))
                    .catch(() => setError("Could not clear cache."))
                    .finally(() => setBusyAction(null));
                }}
                disabled={busyAction === "clear-cache"}
              >
                {busyAction === "clear-cache" ? "Clearing…" : "Clear"}
              </Button>
            </div>
          </Row>
          <Row label="OCR">
            {availableOcrEngines.length > 0 ? (
              <Select
                value={availableOcrEngines.some((engine) => engine.id === settings?.ocrEngineId) ? settings?.ocrEngineId : availableOcrEngines[0]!.id}
                aria-label="OCR engine"
                onChange={(e) => {
                  setBusyAction("ocr-engine");
                  setNotice(null);
                  setError(null);
                  api
                    .updateAppSettings(undefined, e.target.value, undefined)
                    .then(refreshSettings)
                    .then(() => setNotice("OCR engine updated."))
                    .catch((error) => setError(isTimeoutError(error) ? "Updating OCR engine timed out." : "Could not update OCR engine."))
                    .finally(() => setBusyAction(null));
                }}
                className="w-48 bg-surface py-1"
              >
                {availableOcrEngines.map((engine) => (
                  <option key={engine.id} value={engine.id}>{engine.name}</option>
                ))}
              </Select>
            ) : (
              <span className="text-sm text-muted">Unavailable</span>
            )}
          </Row>
          <Row label="Translate">
            {availableTranslators.length > 0 ? (
              <Select
                value={availableTranslators.some((translator) => translator.id === settings?.translatorId) ? settings?.translatorId : availableTranslators[0]!.id}
                aria-label="Translator"
                onChange={(e) => {
                  setBusyAction("translator");
                  setNotice(null);
                  setError(null);
                  api
                    .updateAppSettings(undefined, undefined, e.target.value)
                    .then(refreshSettings)
                    .then(() => setNotice("Translator updated."))
                    .catch((error) => setError(isTimeoutError(error) ? "Updating translator timed out." : "Could not update translator."))
                    .finally(() => setBusyAction(null));
                }}
                className="w-48 bg-surface py-1"
              >
                {availableTranslators.map((translator) => (
                  <option key={translator.id} value={translator.id}>{translator.name}</option>
                ))}
              </Select>
            ) : (
              <span className="text-sm text-muted">Unavailable</span>
            )}
          </Row>
        </Section>

        <Section title="Shortcuts">
          <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            {SHORTCUTS.map((item) => (
              <div key={item.action} className="flex items-center justify-between gap-3">
                <span className="text-muted">{item.action}</span>
                <Kbd>{item.keys}</Kbd>
              </div>
            ))}
          </div>
        </Section>

        <Section title="Cloud">
          <p className="mb-3 text-sm text-muted">
            This Mac stays awake while Cloud is on, including with the lid closed on battery. macOS
            may ask for your password once. A sleeping or shut-down Mac cannot serve files. Do not
            put it in a bag while Cloud is on.
          </p>
          <div className="flex items-center gap-4 py-1.5">
            <span className="w-20 shrink-0 text-sm text-muted">Password</span>
            <div className="min-w-0 flex-1">
              <PasswordInput
                value={ownerPassword}
                onChange={(e) => setOwnerPassword(e.target.value)}
                placeholder="8+ characters"
                autoComplete="new-password"
                aria-label="Owner password"
                className="bg-surface"
              />
            </div>
          </div>
          <div className="flex items-center gap-4 py-1.5">
            <span className="w-20 shrink-0 text-sm text-muted">Hostname</span>
            <div className="min-w-0 flex-1">
              <Input
                value={remoteHostname}
                onChange={(e) => setRemoteHostname(e.target.value)}
                placeholder="example.com"
                autoComplete="off"
                spellCheck={false}
                aria-label="Public hostname"
                className="bg-surface"
              />
            </div>
          </div>
          <div className="flex items-center gap-4 py-1.5">
            <span className="w-20 shrink-0 text-sm text-muted">Token</span>
            <div className="min-w-0 flex-1">
              <PasswordInput
                value={tunnelToken}
                onChange={(e) => setTunnelToken(e.target.value)}
                placeholder={settings?.remote.hasToken ? "Saved" : "Cloudflare token"}
                autoComplete="off"
                aria-label="Cloudflare tunnel token"
                className="bg-surface"
              />
            </div>
          </div>
          {settings?.remote.enabled && settings.remote.publicUrl && (
            <div className="flex items-center gap-4 py-1.5">
              <span className="w-20 shrink-0 text-sm text-muted">URL</span>
              <div className="flex min-w-0 flex-1 items-center gap-1.5">
                <span className="min-w-0 flex-1 truncate text-sm text-accent">{settings.remote.publicUrl}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="shrink-0 px-1.5"
                  aria-label="Copy link"
                  onClick={() => {
                    void navigator.clipboard.writeText(settings.remote.publicUrl ?? "").then(
                      () => setNotice("Copied."),
                      () => setRemoteError("Could not copy."),
                    );
                  }}
                >
                  <Copy size={14} />
                </Button>
              </div>
            </div>
          )}
          {settings?.remote.enabled && settings.remote.qrDataUrl && (
            <img src={settings.remote.qrDataUrl} alt="Cloud QR" className="mt-2 h-24 w-24 rounded bg-white p-1" />
          )}
          {cloudStatus && <p className="mt-2 text-xs text-muted">{cloudStatus}</p>}
          {remoteError && (
            <StatusMessage tone="error" live className="mt-3">
              {remoteError}
            </StatusMessage>
          )}
          <div className="mt-3 flex gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void saveCloud()}
              disabled={busyAction === "credentials"}
            >
              {busyAction === "credentials" ? "Saving…" : "Save"}
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={() => void toggleRemote()}
              disabled={busyAction === "remote" || (!settings?.remote.enabled && !cloudReady)}
            >
              {busyAction === "remote" ? "Applying…" : settings?.remote.enabled ? "Disable" : "Enable"}
            </Button>
          </div>
        </Section>

        <Section title="Household">
          <div className="mb-3 space-y-1">
            {users.map((user) => (
              <div key={user.id} className="flex items-center justify-between gap-2 py-1 text-sm">
                <p>
                  {user.displayName}{" "}
                  <span className="text-muted">
                    @{user.username}
                    {user.disabled ? " · disabled" : ""}
                  </span>
                </p>
                {user.role !== "owner" && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="px-0 text-xs text-red-400 hover:bg-transparent hover:text-red-300"
                    onClick={() => {
                      setBusyAction(`disable-${user.id}`);
                      api
                        .updateUser(user.id, { disabled: !user.disabled })
                        .then(() => api.listUsers().then(setUsers))
                        .catch(() => setError("Could not update user."))
                        .finally(() => setBusyAction(null));
                    }}
                  >
                    {user.disabled ? "Enable" : "Disable"}
                  </Button>
                )}
              </div>
            ))}
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            <Input placeholder="username" value={newUsername} onChange={(e) => setNewUsername(e.target.value)} className="bg-surface" />
            <Input placeholder="Display name" value={newDisplayName} onChange={(e) => setNewDisplayName(e.target.value)} className="bg-surface" />
            <PasswordInput placeholder="Password" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} autoComplete="new-password" className="bg-surface" />
            <label className="flex items-center gap-2 text-sm text-muted">
              <input type="checkbox" checked={newAccessAll} onChange={(e) => setNewAccessAll(e.target.checked)} />
              Whole library
            </label>
          </div>
          {!newAccessAll && series.length > 0 && (
            <div className="mt-2 max-h-36 space-y-1 overflow-y-auto rounded-lg border border-border p-2 text-sm">
              {series.map((item) => (
                <label key={item.id} className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={newGrants.includes(item.id)}
                    onChange={(e) => {
                      setNewGrants((current) =>
                        e.target.checked ? [...current, item.id] : current.filter((id) => id !== item.id),
                      );
                    }}
                  />
                  {item.title}
                </label>
              ))}
            </div>
          )}
          <Button
            variant="secondary"
            size="sm"
            className="mt-3"
            disabled={busyAction === "create-user"}
            onClick={() => {
              setBusyAction("create-user");
              setError(null);
              api
                .createUser({
                  username: newUsername,
                  displayName: newDisplayName,
                  password: newPassword,
                  accessAll: newAccessAll,
                  seriesIds: newGrants,
                })
                .then(() => {
                  setNewUsername("");
                  setNewDisplayName("");
                  setNewPassword("");
                  setNewGrants([]);
                  setNotice("Account created.");
                  return api.listUsers().then(setUsers);
                })
                .catch((err) => setError(err instanceof Error ? err.message : "Could not create user."))
                .finally(() => setBusyAction(null));
            }}
          >
            Add member
          </Button>
        </Section>

        <Section title="Devices">
          {sessions.length === 0 ? (
            <p className="text-sm text-muted">No signed-in devices.</p>
          ) : (
            <div className="space-y-1">
              {sessions.map((item) => (
                <div key={item.id} className="flex items-center justify-between gap-3 py-1 text-sm">
                  <div className="min-w-0">
                    <p className="truncate">
                      {item.deviceLabel} <span className="text-muted">@{item.username}</span>
                    </p>
                    <p className="text-xs text-muted">{new Date(item.lastUsedAt).toLocaleString()}</p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="px-0 text-xs text-red-400 hover:bg-transparent hover:text-red-300"
                    onClick={() => {
                      setBusyAction(`revoke-${item.id}`);
                      api
                        .revokeSession(item.id)
                        .then(() => api.listSessions().then(setSessions))
                        .then(() => setNotice("Session revoked."))
                        .catch(() => setError("Could not revoke session."))
                        .finally(() => setBusyAction(null));
                    }}
                  >
                    Revoke
                  </Button>
                </div>
              ))}
            </div>
          )}
        </Section>

        <Section title="About">
          <Row label="Shelf">
            <div className="flex items-center gap-3">
              <span className="text-sm">{packageInfo.version}</span>
              <Button variant="ghost" size="sm" className="px-0 text-accent hover:bg-transparent hover:text-accent-hover" onClick={openWhatsNew}>
                What’s new
              </Button>
            </div>
          </Row>
        </Section>
      </div>
    </div>
  );
}
