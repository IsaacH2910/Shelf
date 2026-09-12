import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useSearchParams } from "react-router-dom";
import { FolderPlus, RefreshCw } from "lucide-react";
import { api, isTauri, isTimeoutError, pickFolder } from "../lib/api";
import { GroupedCoverGrid, ShelfRow } from "../components/CoverGrid";
import { EmptyState } from "../components/EmptyState";
import { SearchBar, SortFilter, FilterChips } from "../components/SearchBar";
import { StatusMessage } from "../components/StatusMessage";
import { Button } from "../components/Button";
import { Chip } from "../components/Chip";
import { Surface } from "../components/Surface";
import { useApp } from "../context/AppContext";
import { detectMediaKindFromSeries, isVideoItem, type Collection, type LibraryRoot, type Series } from "../types";
import { resolveLibraryPresence } from "../lib/libraryState";

export function HomePage() {
  const { refreshIndexStatus, indexStatus } = useApp();
  const [recent, setRecent] = useState<Series[]>([]);
  const [favorites, setFavorites] = useState<Series[]>([]);
  const [library, setLibrary] = useState<Series[]>([]);
  const [roots, setRoots] = useState<LibraryRoot[] | null>(null);
  const [loadedSeriesCount, setLoadedSeriesCount] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const loadVersion = useRef(0);
  const initialLoad = useRef(true);

  const loadHome = useCallback(async (showSpinner = false) => {
    const requestId = ++loadVersion.current;
    if (showSpinner || initialLoad.current) setLoading(true);
    setError(null);
    try {
      const [r, f, lib, folderList] = await Promise.all([
        api.recentlyAdded(),
        api.listSeries({ favoritesOnly: true, sort: "last_read" }),
        api.listSeries({ sort: "last_read" }),
        isTauri() ? api.listLibraryRoots() : Promise.resolve([] as LibraryRoot[]),
      ]);
      if (requestId !== loadVersion.current) return;
      setRecent(r);
      setFavorites(f);
      setLibrary(lib.slice(0, 15));
      setLoadedSeriesCount(lib.length);
      setRoots(folderList);
    } catch (error) {
      if (requestId !== loadVersion.current) return;
      setError(isTimeoutError(error) ? "Loading your library timed out. Try again." : "Could not load your library right now.");
    } finally {
      if (requestId !== loadVersion.current) return;
      initialLoad.current = false;
      setLoading(false);
      refreshIndexStatus();
    }
  }, [refreshIndexStatus]);

  useEffect(() => {
    void loadHome(true);
  }, [loadHome]);

  useEffect(() => {
    if (!isTauri() || loading || loadedSeriesCount == null || indexStatus == null) return;
    if (indexStatus.seriesCount !== loadedSeriesCount) {
      void loadHome(false);
    }
  }, [indexStatus?.seriesCount, indexStatus?.indexing, loadedSeriesCount, loading, loadHome]);

  const recentReading = recent.filter((item) => !isVideoItem(item));
  const recentWatching = recent.filter((item) => isVideoItem(item));
  const favoriteReading = favorites.filter((item) => !isVideoItem(item));
  const favoriteWatching = favorites.filter((item) => isVideoItem(item));
  const libraryReading = library.filter((item) => !isVideoItem(item));
  const libraryWatching = library.filter((item) => isVideoItem(item));
  const itemCount = library.length + recent.length + favorites.length;
  const presence = resolveLibraryPresence({
    loading,
    error,
    itemCount,
    rootCount: roots?.length ?? (isTauri() ? null : 1),
    indexing: Boolean(indexStatus?.indexing),
    statusKnown: indexStatus != null,
  });
  const hour = new Date().getHours();
  const greeting = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 md:px-8">
      <div className="animate-in mx-auto max-w-6xl">
        <h1 className="text-2xl font-bold tracking-tight md:text-3xl">{greeting}</h1>
        <p className="mt-1 text-sm text-muted">What should you read or watch?</p>

        {presence === "loading" ? (
          <div className="mt-8 grid gap-4">
            <div className="skeleton h-36" />
            <div className="flex gap-3">
              {Array.from({ length: 6 }).map((_, i) => (
                <div key={i} className="skeleton h-28 w-20" />
              ))}
            </div>
          </div>
        ) : presence === "error" ? (
          <div className="mt-8">
            <EmptyState
              title="Library temporarily unavailable"
              description={error ?? "Could not load your library right now."}
              action={
                <button
                  onClick={() => void loadHome(true)}
                  className="inline-flex items-center gap-2 bg-accent px-4 py-2 text-sm font-medium text-bg hover:bg-accent-hover"
                >
                  Retry
                </button>
              }
            />
          </div>
        ) : (
          <>
            {presence === "indexing" ? (
              <EmptyState
                title="Indexing your library"
                description="Shelf is scanning the folders you added. Titles will appear here as they are found."
              />
            ) : presence === "empty-no-folders" ? (
              <EmptyState
                title="Your library is empty"
                description="Add a folder with manga PDFs, document PDFs, or local video files. Shelf only indexes them on your Mac."
                action={
                  isTauri() ? (
                    <AddFolderButton onAdded={() => void loadHome(true)} />
                  ) : (
                    <p className="text-xs text-muted">Ask the Mac app to add a library folder.</p>
                  )
                }
              />
            ) : presence === "empty-no-files" ? (
              <EmptyState
                title="No supported files yet"
                description="The added folders do not currently contain PDF or MP4 files in a series folder. Nested season folders are indexed as their own titles."
                action={isTauri() ? <AddFolderButton onAdded={() => void loadHome(true)} /> : undefined}
              />
            ) : null}
            <div className="mt-10">
              <ShelfRow title="Recently added to bookshelf" series={recentReading} />
              <ShelfRow title="Recently added videos" series={recentWatching} layout="landscape" />
              <ShelfRow title="Favorite books" series={favoriteReading} />
              <ShelfRow title="Favorite videos" series={favoriteWatching} layout="landscape" />
              <ShelfRow title="Library" series={libraryReading} />
              <ShelfRow title="Videos" series={libraryWatching} layout="landscape" />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function AddFolderButton({ onAdded }: { onAdded?: () => void }) {
  const { refreshIndexStatus } = useApp();

  const add = async () => {
    const path = await pickFolder();
    if (path) {
      await api.addLibraryRoot(path);
      await refreshIndexStatus();
      onAdded?.();
    }
  };
  return (
    <Button
      onClick={add}
      variant="primary"
    >
      <FolderPlus size={16} />
      Add Folder
    </Button>
  );
}

export function LibraryPage() {
  const { indexStatus, refreshIndexStatus } = useApp();
  const [series, setSeries] = useState<Series[]>([]);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState("title");
  const [contentType, setContentType] = useState<"all" | "manga" | "document" | "video">("all");
  const [collections, setCollections] = useState<Collection[]>([]);
  const [collectionId, setCollectionId] = useState<number | undefined>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [collectionsError, setCollectionsError] = useState<string | null>(null);
  const [roots, setRoots] = useState<LibraryRoot[] | null>(null);
  const loadVersion = useRef(0);
  const collectionsLoadVersion = useRef(0);
  const initialLoad = useRef(true);

  const load = useCallback(async () => {
    const requestId = ++loadVersion.current;
    if (initialLoad.current) setLoading(true);
    setError(null);
    try {
      const [items, folderList] = await Promise.all([
        api.listSeries({
          search: search || undefined,
          collectionId,
          sort,
          contentType,
        }),
        isTauri() ? api.listLibraryRoots() : Promise.resolve([] as LibraryRoot[]),
      ]);
      if (requestId !== loadVersion.current) return;
      setSeries(items);
      setRoots(folderList);
    } catch (error) {
      if (requestId !== loadVersion.current) return;
      setError(isTimeoutError(error) ? "Library results timed out. Try again." : "Could not load library results.");
    } finally {
      if (requestId !== loadVersion.current) return;
      initialLoad.current = false;
      setLoading(false);
      refreshIndexStatus();
    }
  }, [search, sort, collectionId, contentType, refreshIndexStatus]);

  const loadCollections = useCallback(async () => {
    const requestId = ++collectionsLoadVersion.current;
    try {
      const items = await api.listCollections();
      if (requestId !== collectionsLoadVersion.current) return;
      setCollections(items);
      setCollectionsError(null);
    } catch (error) {
      if (requestId !== collectionsLoadVersion.current) return;
      setCollections([]);
      setCollectionsError(isTimeoutError(error) ? "Collections took too long to load." : "Collections are unavailable right now.");
    }
  }, []);

  useEffect(() => {
    const t = setTimeout(load, search ? 200 : 0);
    return () => clearTimeout(t);
  }, [load, search, indexStatus?.seriesCount, indexStatus?.chapterCount, indexStatus?.indexing]);

  useEffect(() => {
    void loadCollections();
  }, [loadCollections]);

  const handleAddFolder = async () => {
    const path = await pickFolder();
    if (path) {
      await api.addLibraryRoot(path);
      await load();
    }
  };

  const visibleSeries = useMemo(
    () =>
      series.filter((item) => {
        if (contentType === "all") return true;
        const kind = detectMediaKindFromSeries(item).contentType;
        if (contentType === "manga") return kind === "manga" || kind === "manhwa" || kind === "comic";
        return kind === contentType;
      }),
    [series, contentType],
  );
  const filtered = Boolean(search || collectionId || contentType !== "all");
  const presence = resolveLibraryPresence({
    loading,
    error,
    itemCount: visibleSeries.length,
    rootCount: roots?.length ?? (isTauri() ? null : 1),
    indexing: Boolean(indexStatus?.indexing),
    statusKnown: indexStatus != null,
  });
  const emptyState = filtered && presence !== "loading" && presence !== "error" ? (
    <EmptyState
      title="No matching media"
      description="Try another filter or add a folder with manga PDFs, document PDFs, or local videos."
      action={isTauri() ? <AddFolderButton onAdded={() => void load()} /> : undefined}
    />
  ) : presence === "indexing" ? (
    <EmptyState title="Indexing your library" description="Titles will show up here as Shelf finishes scanning your folders." />
  ) : presence === "empty-no-folders" ? (
    <EmptyState
      title="Your library is empty"
      description="Add a folder with manga PDFs, document PDFs, or local videos."
      action={isTauri() ? <AddFolderButton onAdded={() => void load()} /> : undefined}
    />
  ) : presence === "empty-no-files" ? (
    <EmptyState
      title="No supported files yet"
      description="The added folders do not currently contain PDF or MP4 files inside a series folder."
      action={isTauri() ? <AddFolderButton onAdded={() => void load()} /> : undefined}
    />
  ) : presence === "error" ? null : (
    <EmptyState
      title="No titles to show"
      description="Add a folder with manga PDFs, document PDFs, or local videos."
      action={isTauri() ? <AddFolderButton onAdded={() => void load()} /> : undefined}
    />
  );

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="flex shrink-0 items-center justify-between gap-4 border-b border-border px-4 py-4 md:px-8">
        <div>
          <h1 className="text-2xl font-semibold">Library</h1>
          <p className="text-sm text-muted">{series.length} series</p>
        </div>
        {isTauri() && (
          <div className="flex gap-2">
            <Button
              onClick={() => api.rescanLibrary().then(load)}
              variant="secondary"
            >
              <RefreshCw size={16} />
              Rescan
            </Button>
            <Button
              onClick={handleAddFolder}
              variant="primary"
            >
              <FolderPlus size={16} />
              Add Folder
            </Button>
          </div>
        )}
      </header>
      <div className="flex-1 overflow-y-auto px-4 py-6 md:px-8">
        {error && (
          <StatusMessage
            className="mb-4"
            tone="error"
            live
            action={
              <button onClick={() => void load()} className="text-accent hover:text-accent-hover">
                Retry
              </button>
            }
          >
            {error}
          </StatusMessage>
        )}
        {indexStatus?.indexing && (
          <StatusMessage className="mb-4" live>
            Updating your library: {indexStatus.pendingJobs} {indexStatus.pendingJobs === 1 ? "task" : "tasks"} remaining
            {indexStatus.processedJobs > 0 && `, ${indexStatus.processedJobs} completed`}. You can keep reading.
          </StatusMessage>
        )}
        {indexStatus && !indexStatus.indexing && indexStatus.failedJobs > 0 && (
          <StatusMessage
            tone="error"
            live
            className="mb-4"
            action={
              <Button variant="ghost" size="sm" className="text-accent" onClick={() => void api.rescanLibrary().then(load)}>
                Rescan
              </Button>
            }
          >
            {indexStatus.failedJobs} background {indexStatus.failedJobs === 1 ? "task" : "tasks"} could not finish. Your existing library is still available.
          </StatusMessage>
        )}
        <div className="mb-4 flex gap-3">
          <div className="flex-1">
            <SearchBar value={search} onChange={setSearch} />
          </div>
          <SortFilter sort={sort} onSortChange={setSort} />
        </div>
        <div className="mb-6">
          <FilterChips
            contentType={contentType}
            onContentType={setContentType}
          />
          {collections.length > 0 && (
            <div className="no-scrollbar mt-2 flex gap-2 overflow-x-auto">
              {collections.map((c) => (
                <Chip
                  key={c.id}
                  onClick={() => setCollectionId(collectionId === c.id ? undefined : c.id)}
                  active={collectionId === c.id}
                >
                  {c.name}
                </Chip>
              ))}
            </div>
          )}
          {collectionsError && (
            <StatusMessage
              tone="error"
              live
              className="mt-3"
              action={
                <button type="button" onClick={() => void loadCollections()} className="text-accent hover:text-accent-hover">
                  Retry
                </button>
              }
            >
              {collectionsError} Library results are still available.
            </StatusMessage>
          )}
        </div>
        <GroupedCoverGrid
          series={visibleSeries}
          loading={loading}
          empty={emptyState}
        />
      </div>
    </div>
  );
}

export function SearchPage() {
  const [search, setSearch] = useState("");
  const [series, setSeries] = useState<Series[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loadVersion = useRef(0);

  useEffect(() => {
    const t = setTimeout(() => {
      if (!search) {
        loadVersion.current += 1;
        setSeries([]);
        setError(null);
        setLoading(false);
        return;
      }
      const requestId = ++loadVersion.current;
      setLoading(true);
      setError(null);
      api
        .listSeries({ search })
        .then((items) => {
          if (requestId !== loadVersion.current) return;
          setSeries(items);
        })
        .catch((error) => {
          if (requestId !== loadVersion.current) return;
          setError(isTimeoutError(error) ? "Search timed out. Try a shorter query or retry." : "Search is temporarily unavailable.");
        })
        .finally(() => {
          if (requestId !== loadVersion.current) return;
          setLoading(false);
        });
    }, 200);
    return () => clearTimeout(t);
  }, [search]);

  return (
    <PageFrame title="Search">
      <SearchBar value={search} onChange={setSearch} autoFocus />
      <div className="mt-6">
        {loading ? (
          <StatusMessage>Searching...</StatusMessage>
        ) : error ? (
          <StatusMessage tone="error" live>
            {error}
          </StatusMessage>
        ) : search && series.length === 0 ? (
          <EmptyState title="No matches" description="Try a title, tag, collection, or chapter name." />
        ) : (
          <GroupedCoverGrid series={series} />
        )}
      </div>
    </PageFrame>
  );
}

export function ActivityPage() {
  const [searchParams] = useSearchParams();
  const requestedSection = searchParams.get("section");
  const [favoriteSeries, setFavoriteSeries] = useState<Series[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const loadVersion = useRef(0);

  const loadActivity = useCallback(async () => {
    const requestId = ++loadVersion.current;
    setLoading(true);
    setError(null);
    try {
      const fav = await api.listSeries({ favoritesOnly: true, sort: "last_read" });
      if (requestId !== loadVersion.current) return;
      setFavoriteSeries(fav);
    } catch (error) {
      if (requestId !== loadVersion.current) return;
      setError(isTimeoutError(error) ? "Activity took too long to load. Try again." : "Could not load activity.");
    } finally {
      if (requestId !== loadVersion.current) return;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadActivity();
  }, [loadActivity]);

  useEffect(() => {
    if (loading || !requestedSection) return;
    document.getElementById(requestedSection)?.scrollIntoView({ block: "start" });
  }, [loading, requestedSection]);

  return (
    <PageFrame title="Activity">
      {loading ? (
        <div className="grid gap-4">
          <div className="skeleton h-24 rounded-xl" />
          <div className="skeleton h-56 rounded-xl" />
        </div>
      ) : error ? (
        <EmptyState
          title="Activity unavailable"
          description={error}
          action={
            <button
              onClick={() => void loadActivity()}
              className="inline-flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg hover:bg-accent-hover"
            >
              Retry
            </button>
          }
        />
      ) : (
        <div className="space-y-6">
          <section id="favorites" className="scroll-mt-4">
            {favoriteSeries.length > 0 ? (
              <ShelfRow title="Favorites" series={favoriteSeries.slice(0, 18)} />
            ) : (
              <Surface className="text-sm text-muted">
                No favorites yet. Heart a series to pin it.
              </Surface>
            )}
          </section>
        </div>
      )}
    </PageFrame>
  );
}

function PageFrame({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="border-b border-border px-4 py-4 md:px-8">
        <h1 className="text-2xl font-semibold">{title}</h1>
      </header>
      <div className="flex-1 overflow-y-auto px-4 py-6 md:px-8">{children}</div>
    </div>
  );
}
