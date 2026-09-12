import type { ForwardRefExoticComponent, ReactNode, RefAttributes } from "react";
import { NavLink, useLocation } from "react-router-dom";
import {
  BookOpen,
  Clock,
  FolderHeart,
  Home,
  Library,
  Search,
  Settings,
  type LucideProps,
} from "lucide-react";
import { cn } from "../lib/utils";
import { useApp } from "../context/AppContext";
import { isTauri } from "../lib/api";

type NavItem = {
  to: string;
  label: string;
  icon: ForwardRefExoticComponent<Omit<LucideProps, "ref"> & RefAttributes<SVGSVGElement>>;
  end?: boolean;
};

const primary: NavItem[] = [
  { to: "/", label: "Home", icon: Home, end: true },
  { to: "/library", label: "Library", icon: Library },
  { to: "/search", label: "Search", icon: Search },
  { to: "/activity", label: "Activity", icon: BookOpen },
];

const collectionsNav: NavItem = { to: "/collections", label: "Collections", icon: FolderHeart };

const more: NavItem[] = [collectionsNav, { to: "/settings", label: "Settings", icon: Settings }];

const mobileNav: NavItem[] = [...primary, collectionsNav];

export function AppShell({ children }: { children: ReactNode }) {
  const { indexStatus, session, signOut } = useApp();
  const location = useLocation();
  const isReader = location.pathname.startsWith("/read/");
  const remote = !isTauri();
  const showIndexCounts = Boolean(
    indexStatus &&
      !(
        remote &&
        !indexStatus.indexing &&
        indexStatus.seriesCount === 0 &&
        indexStatus.chapterCount === 0
      ),
  );

  if (isReader) {
    return <div className="flex h-dvh min-h-0 w-full flex-col overflow-hidden bg-bg">{children}</div>;
  }

  return (
    <div className="flex h-dvh min-h-0 w-full flex-1 overflow-hidden bg-bg">
      <aside className="hidden h-full w-56 shrink-0 flex-col border-r border-border bg-surface min-[720px]:flex">
        <div className="px-5 py-6">
          <h1 className="text-xl font-semibold tracking-tight text-text">Shelf</h1>
          <p className="mt-1 text-xs text-muted">Your personal library</p>
        </div>
        <nav className="flex flex-1 flex-col gap-1 px-3">
          {[...primary, ...more.filter((i) => !(remote && i.to === "/settings"))].map(
            ({ to, label, icon: Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition-colors",
                    isActive
                      ? "bg-accent/15 text-accent"
                      : "text-muted hover:bg-elevated hover:text-text",
                  )
                }
              >
                <Icon size={18} />
                {label}
              </NavLink>
            ),
          )}
        </nav>
        {indexStatus && (
          <div className="border-t border-border px-4 py-4 text-xs text-muted">
            {showIndexCounts && (
              <div className="flex items-center gap-2">
                <Clock size={14} />
                {indexStatus.seriesCount} series · {indexStatus.chapterCount} chapters
              </div>
            )}
            {indexStatus.indexing && (
              <p className="mt-1 text-accent">
                Updating library · {indexStatus.pendingJobs} {indexStatus.pendingJobs === 1 ? "task" : "tasks"} remaining
                {indexStatus.processedJobs > 0 && ` · ${indexStatus.processedJobs} completed`}
              </p>
            )}
            {!indexStatus.indexing && indexStatus.failedJobs > 0 && (
              <p className="mt-1 text-red-300">
                {indexStatus.failedJobs} {indexStatus.failedJobs === 1 ? "task" : "tasks"} failed · Rescan to retry
              </p>
            )}
            {remote && session && (
              <div className={cn("flex items-center justify-between gap-2", showIndexCounts && "mt-3")}>
                <p className="truncate">{session.displayName}</p>
                <button type="button" className="text-accent hover:text-accent-hover" onClick={() => void signOut()}>
                  Sign out
                </button>
              </div>
            )}
          </div>
        )}
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-border bg-bg px-4 pb-3 pt-[calc(0.75rem+env(safe-area-inset-top))] min-[720px]:hidden">
          <h1 className="text-base font-semibold">Shelf</h1>
          {remote ? (
            <button type="button" className="text-sm text-muted" onClick={() => void signOut()}>
              Sign out
            </button>
          ) : (
            <NavLink to="/settings" className="text-muted">
              <Settings size={18} />
            </NavLink>
          )}
        </header>
        <main className="min-h-0 flex-1 overflow-hidden pb-[calc(4.75rem+env(safe-area-inset-bottom))] min-[720px]:pb-0">
          {children}
        </main>
      </div>

      <nav
        aria-label="Primary"
        className="fixed inset-x-0 bottom-0 z-30 flex border-t border-border bg-surface pb-[env(safe-area-inset-bottom)] min-[720px]:hidden"
      >
        {mobileNav.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              cn(
                "flex flex-1 flex-col items-center gap-0.5 px-1 py-2 text-center text-[11px] leading-tight",
                isActive ? "text-accent" : "text-muted",
              )
            }
          >
            <Icon size={18} />
            {label}
          </NavLink>
        ))}
      </nav>
    </div>
  );
}
