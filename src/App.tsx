import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { LoginGate } from "./components/PairGate";
import { AppContext } from "./context/AppContext";
import { api, fetchSessionInfo, isTauri, logoutRemote } from "./lib/api";
import {
  ActivityPage,
  HomePage,
  LibraryPage,
  SearchPage,
} from "./pages/LibraryPage";
import { CollectionsPage } from "./pages/CollectionsPage";
import { SeriesPage } from "./pages/SeriesPage";
import { SettingsPage } from "./pages/SettingsPage";
import type { AppSettings, IndexStatus, SessionInfo } from "./types";
import packageInfo from "../package.json";

const ReaderPage = lazy(() =>
  import("./pages/ReaderPage").then((m) => ({ default: m.ReaderPage })),
);

const WHATS_NEW_STORAGE_KEY = "shelf.whatsNew.lastSeenVersion";

function WhatsNewModal({ version, onClose }: { version: string; onClose: () => void }) {
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeButtonRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      openerRef.current?.focus();
    };
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4" role="dialog" aria-modal="true" aria-labelledby="whats-new-title">
      <div className="w-full max-w-lg rounded-2xl border border-border bg-elevated p-5 shadow-2xl animate-in">
        <p className="text-xs uppercase tracking-[0.12em] text-accent">What&apos;s new</p>
        <h2 id="whats-new-title" className="mt-1 text-xl font-semibold text-text">Shelf {version}</h2>
        <ul className="mt-4 space-y-2 text-sm text-muted">
          <li>Local-first library for PDFs and local video, indexed in place on your Mac.</li>
          <li>Chapter version preference in the reader, with V and Shift+V to cycle.</li>
          <li>Optional HTTPS cloud access, household accounts, and per-user reading progress.</li>
        </ul>
        <div className="mt-5 flex items-center justify-end gap-2">
          <button
            type="button"
            ref={closeButtonRef}
            onClick={onClose}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg hover:bg-accent-hover"
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}

function AppRoutes() {
  return (
    <Suspense fallback={<div className="flex h-full items-center justify-center text-muted">Loading…</div>}>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/library" element={<LibraryPage />} />
        <Route path="/search" element={<SearchPage />} />
        <Route path="/activity" element={<ActivityPage />} />
        <Route path="/continue" element={<Navigate to="/activity?section=favorites" replace />} />
        <Route path="/favorites" element={<Navigate to="/activity?section=favorites" replace />} />
        <Route path="/history" element={<Navigate to="/activity?section=continue" replace />} />
        <Route path="/collections" element={<CollectionsPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/series/:id" element={<SeriesPage />} />
        <Route path="/read/:chapterId" element={<ReaderPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Suspense>
  );
}

function AppShellHost() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [indexStatus, setIndexStatus] = useState<IndexStatus | null>(null);
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [auth, setAuth] = useState<"loading" | "in" | "out">(isTauri() ? "in" : "loading");
  const [showWhatsNew, setShowWhatsNew] = useState(false);

  const refreshSettings = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
    } catch {
      /* web client without session */
    }
  }, []);

  const refreshIndexStatus = useCallback(async () => {
    try {
      setIndexStatus(await api.getIndexStatus());
    } catch {
      /* ignore */
    }
  }, []);

  const signOut = useCallback(async () => {
    if (!isTauri()) {
      await logoutRemote();
    }
    setSession(null);
    setAuth("out");
  }, []);

  useEffect(() => {
    if (isTauri()) {
      setAuth("in");
      return;
    }
    let cancelled = false;
    fetchSessionInfo()
      .then((info) => {
        if (!cancelled) {
          setSession(info);
          setAuth("in");
        }
      })
      .catch(() => {
        if (!cancelled) setAuth("out");
      });
    const onUnauth = () => {
      setSession(null);
      setAuth("out");
    };
    window.addEventListener("shelf:unauthorized", onUnauth);
    return () => {
      cancelled = true;
      window.removeEventListener("shelf:unauthorized", onUnauth);
    };
  }, []);

  useEffect(() => {
    if (auth !== "in") return;
    refreshSettings();
    refreshIndexStatus();
    const interval = setInterval(refreshIndexStatus, 5000);
    return () => clearInterval(interval);
  }, [auth, refreshSettings, refreshIndexStatus]);

  useEffect(() => {
    if (auth !== "in") return;
    try {
      const lastSeenVersion = localStorage.getItem(WHATS_NEW_STORAGE_KEY);
      setShowWhatsNew(lastSeenVersion !== packageInfo.version);
    } catch {
      setShowWhatsNew(true);
    }
  }, [auth]);

  const closeWhatsNew = useCallback(() => {
    setShowWhatsNew(false);
    try {
      localStorage.setItem(WHATS_NEW_STORAGE_KEY, packageInfo.version);
    } catch {
      /* ignore storage failures */
    }
  }, []);

  const openWhatsNew = useCallback(() => {
    setShowWhatsNew(true);
  }, []);

  if (auth === "loading") {
    return <div className="flex h-full items-center justify-center text-muted">Connecting…</div>;
  }

  if (auth === "out") {
    return (
      <LoginGate
        onSignedIn={(info) => {
          setSession(info);
          setAuth("in");
        }}
      />
    );
  }

  return (
    <AppContext.Provider
      value={{ settings, indexStatus, refreshSettings, refreshIndexStatus, openWhatsNew, session, signOut }}
    >
      <>
        <AppShell>
          <AppRoutes />
        </AppShell>
        {showWhatsNew && <WhatsNewModal version={packageInfo.version} onClose={closeWhatsNew} />}
      </>
    </AppContext.Provider>
  );
}

export default function App() {
  return (
    <HashRouter>
      <AppShellHost />
    </HashRouter>
  );
}
