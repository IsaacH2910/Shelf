import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api, isThinReader, isTimeoutError, isTauri } from "../lib/api";
import { Button } from "../components/Button";
import { Chip } from "../components/Chip";
import { EmptyState } from "../components/EmptyState";
import { CoverGrid, GroupedCoverGrid } from "../components/CoverGrid";
import { Input } from "../components/Input";
import { StatusMessage } from "../components/StatusMessage";
import type { Collection, Series } from "../types";

export function CollectionsPage() {
  const [collections, setCollections] = useState<Collection[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [series, setSeries] = useState<Series[]>([]);
  const [name, setName] = useState("");
  const [loadingCollections, setLoadingCollections] = useState(true);
  const [collectionsError, setCollectionsError] = useState<string | null>(null);
  const [loadingSeries, setLoadingSeries] = useState(false);
  const [seriesError, setSeriesError] = useState<string | null>(null);
  const collectionsLoadVersion = useRef(0);
  const seriesLoadVersion = useRef(0);

  const reload = useCallback(async () => {
    const requestId = ++collectionsLoadVersion.current;
    setLoadingCollections(true);
    setCollectionsError(null);
    try {
      const items = await api.listCollections();
      if (requestId !== collectionsLoadVersion.current) return;
      setCollections(items);
    } catch (error) {
      if (requestId !== collectionsLoadVersion.current) return;
      setCollections([]);
      setCollectionsError(
        isTimeoutError(error)
          ? "Collections took too long to load. Try again."
          : "Could not load collections right now.",
      );
    } finally {
      if (requestId !== collectionsLoadVersion.current) return;
      setLoadingCollections(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    if (selected == null) {
      seriesLoadVersion.current += 1;
      setSeries([]);
      setSeriesError(null);
      setLoadingSeries(false);
      return;
    }
    const requestId = ++seriesLoadVersion.current;
    setLoadingSeries(true);
    setSeriesError(null);
    api
      .listSeries({ collectionId: selected })
      .then((items) => {
        if (requestId !== seriesLoadVersion.current) return;
        setSeries(items);
      })
      .catch((error) => {
        if (requestId !== seriesLoadVersion.current) return;
        setSeries([]);
        setSeriesError(
          isTimeoutError(error)
            ? "This collection took too long to load. Try again."
            : "Could not load this collection right now.",
        );
      })
      .finally(() => {
        if (requestId !== seriesLoadVersion.current) return;
        setLoadingSeries(false);
      });
  }, [selected]);

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="border-b border-border px-4 py-4 md:px-8">
        <h1 className="text-2xl font-semibold">Collections</h1>
      </header>
      <div className="flex-1 overflow-y-auto px-4 py-6 md:px-8">
        {isThinReader() && (
          <StatusMessage className="mb-6">
            Collections can be browsed here, but creating or editing them stays on the Mac app.
          </StatusMessage>
        )}
        {collectionsError && (
          <StatusMessage
            className="mb-6"
            tone="error"
            live
            action={
              <Button variant="ghost" size="sm" className="px-0 text-accent hover:bg-transparent hover:text-accent-hover" onClick={() => void reload()}>
                Retry
              </Button>
            }
          >
            {collectionsError}
          </StatusMessage>
        )}
        {isTauri() && (
          <form
            className="mb-6 flex gap-2"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!name.trim()) return;
              await api.createCollection(name.trim());
              setName("");
              reload();
            }}
          >
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="New collection"
              className="h-11 flex-1 rounded-xl"
            />
            <Button variant="primary" className="rounded-xl">Create</Button>
          </form>
        )}
        {loadingCollections ? (
          <div className="grid gap-3">
            <div className="skeleton h-10 rounded-xl" />
            <div className="skeleton h-10 rounded-xl" />
            <div className="skeleton h-10 rounded-xl" />
          </div>
        ) : collections.length === 0 ? (
          <EmptyState title="No collections" description="Group series into shelves you care about." />
        ) : (
          <div className="mb-8 flex flex-wrap gap-2">
            {collections.map((c) => (
              <Chip
                key={c.id}
                onClick={() => setSelected(c.id)}
                active={selected === c.id}
                aria-pressed={selected === c.id}
                aria-label={`Open collection ${c.name}`}
                className="text-sm"
              >
                {c.name} · {c.seriesCount}
              </Chip>
            ))}
          </div>
        )}
        {selected != null && (
          <>
            {isTauri() && (
              <Button
                variant="ghost"
                size="sm"
                aria-label="Delete selected collection"
                className="mb-4 px-0 text-xs text-red-400 hover:bg-transparent hover:text-red-300"
                onClick={() => api.deleteCollection(selected).then(() => { setSelected(null); reload(); })}
              >
                Delete collection
              </Button>
            )}
            {seriesError ? (
              <StatusMessage tone="error" live>
                {seriesError}
              </StatusMessage>
            ) : loadingSeries ? (
              <CoverGrid series={[]} loading />
            ) : (
              <GroupedCoverGrid
                series={series}
                empty={<p className="text-sm text-muted">No series in this collection. Add them from a series page.</p>}
              />
            )}
          </>
        )}
        <p className="mt-8 text-xs text-muted">
          Tip: open a <Link to="/library" className="text-accent">series</Link> to assign collections.
        </p>
      </div>
    </div>
  );
}
