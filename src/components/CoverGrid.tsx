import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Heart, Play } from "lucide-react";
import { api, revokeObjectUrl } from "../lib/api";
import { cn, coverFallback, formatProgress, initials } from "../lib/utils";
import { detectMediaKindFromSeries, isVideoItem } from "../types";
import type { Series } from "../types";

export function CoverImage({
  seriesId,
  title,
  className,
}: {
  seriesId: number;
  title: string;
  className?: string;
}) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    let nextUrl: string | null = null;
    api.getCoverImage(seriesId).then((u) => {
      if (cancelled) {
        revokeObjectUrl(u);
        return;
      }
      nextUrl = u;
      setUrl((prev) => {
        if (prev && prev !== u) revokeObjectUrl(prev);
        return u;
      });
    });
    return () => {
      cancelled = true;
      revokeObjectUrl(nextUrl);
    };
  }, [seriesId]);

  return (
    <div
      className={cn("relative h-full w-full overflow-hidden bg-elevated", className)}
      style={!url ? { background: coverFallback(title) } : undefined}
    >
      {url ? (
        <img src={url} alt={title} className="h-full w-full object-cover" loading="lazy" />
      ) : (
        <span className="absolute inset-0 flex items-center justify-center text-lg font-semibold text-white/50">
          {initials(title)}
        </span>
      )}
    </div>
  );
}

function OffsetMedia({
  series,
  layout,
  children,
}: {
  series: Series;
  layout: "portrait" | "landscape";
  children?: ReactNode;
}) {
  return (
    <div className="shelf-card">
      <div className="shelf-card-shadow" />
      <div className={cn("shelf-card-face", layout === "landscape" ? "shelf-card-face--landscape" : "shelf-card-face--portrait")}>
        <CoverImage seriesId={series.id} title={series.title} />
        {children}
        {series.favorite && (
          <div className="absolute right-2 top-2 z-10 rounded-md bg-black p-1 text-red-400">
            <Heart size={12} fill="currentColor" />
          </div>
        )}
        {series.progressPercent > 0 && (
          <div className="absolute inset-x-0 bottom-0 z-10 h-0.5 bg-black">
            <div className="h-full bg-accent" style={{ width: `${Math.min(100, series.progressPercent)}%` }} />
          </div>
        )}
      </div>
    </div>
  );
}

export function SeriesCard({ series, showProgress = true }: { series: Series; showProgress?: boolean }) {
  const mediaKind = detectMediaKindFromSeries(series);
  const isVideo = mediaKind.sourceFormat === "mp4";
  const countLabel = isVideo
    ? `${series.chapterCount} ${series.chapterCount === 1 ? "episode" : "episodes"}`
    : `${series.chapterCount} ${series.chapterCount === 1 ? "chapter" : "chapters"}`;

  return (
    <Link to={`/series/${series.id}`} className="group flex h-full min-h-0 flex-col">
      <OffsetMedia series={series} layout={isVideo ? "landscape" : "portrait"}>
        {isVideo && (
          <div className="absolute inset-0 z-10 flex items-center justify-center">
            <div className="flex h-9 w-9 items-center justify-center rounded-full bg-black text-white">
              <Play size={14} fill="currentColor" className="ml-0.5" />
            </div>
          </div>
        )}
        {!isVideo && series.unreadCount > 0 && (
          <div className="absolute left-2 top-2 z-10 rounded-md bg-accent px-1.5 py-0.5 text-[10px] font-medium text-bg">
            {series.unreadCount} new
          </div>
        )}
      </OffsetMedia>
      <div className="mt-2 flex min-h-[2.75rem] flex-col">
        <h3 className="line-clamp-2 text-[13px] font-medium leading-snug text-text">{series.title}</h3>
        <p className="mt-auto pt-1 text-[11px] text-muted">
          {countLabel}
          {showProgress ? ` · ${formatProgress(series.progressPercent, isVideo)}` : ""}
        </p>
      </div>
    </Link>
  );
}

export function CoverSkeleton({ layout = "portrait" }: { layout?: "portrait" | "landscape" }) {
  return (
    <div>
      <div className={cn("skeleton rounded-xl", layout === "landscape" ? "aspect-video" : "aspect-[2/3]")} />
      <div className="skeleton mt-2 h-3 w-3/4" />
      <div className="skeleton mt-1 h-2 w-1/2" />
    </div>
  );
}

function MediaGrid({
  series,
  layout,
}: {
  series: Series[];
  layout: "portrait" | "landscape";
}) {
  const columnsClass = layout === "landscape"
    ? "grid grid-cols-1 gap-5 sm:grid-cols-2 xl:grid-cols-3"
    : "grid grid-cols-3 gap-x-4 gap-y-6 md:grid-cols-5 xl:grid-cols-6";
  return (
    <div className={columnsClass}>
      {series.map((item) => (
        <SeriesCard key={item.id} series={item} />
      ))}
    </div>
  );
}

export function CoverGrid({
  series,
  loading,
  empty,
}: {
  series: Series[];
  loading?: boolean;
  empty?: ReactNode;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const columns = 6;
  const useVirtual = !loading && series.length >= 60;
  const rows = Math.ceil(series.length / columns);
  const virtualizer = useVirtualizer({
    count: useVirtual ? rows : 0,
    getScrollElement: () => parentRef.current,
    estimateSize: () => {
      const width = parentRef.current?.clientWidth ?? 800;
      const cardWidth = Math.max(96, (width - (columns - 1) * 16) / columns);
      return cardWidth * 1.5 + 64;
    },
    overscan: 4,
  });

  if (loading) {
    return (
      <div className="grid grid-cols-3 gap-x-4 gap-y-6 md:grid-cols-5 xl:grid-cols-6">
        {Array.from({ length: 10 }).map((_, i) => (
          <CoverSkeleton key={i} />
        ))}
      </div>
    );
  }
  if (series.length === 0) return <>{empty}</>;
  if (!useVirtual) {
    return <MediaGrid series={series} layout="portrait" />;
  }

  return (
    <div ref={parentRef} className="h-[70vh] overflow-auto">
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((row) => {
          const start = row.index * columns;
          const slice = series.slice(start, start + columns);
          return (
            <div
              key={row.key}
              ref={virtualizer.measureElement}
              data-index={row.index}
              className="absolute left-0 grid w-full grid-cols-6 gap-x-4 gap-y-6"
              style={{ transform: `translateY(${row.start}px)` }}
            >
              {slice.map((s) => (
                <SeriesCard key={s.id} series={s} />
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function splitMedia(series: Series[]) {
  const reading: Series[] = [];
  const watching: Series[] = [];
  for (const item of series) {
    if (isVideoItem(item)) watching.push(item);
    else reading.push(item);
  }
  return { reading, watching };
}

function groupedByName(series: Series[]) {
  const ungrouped = series.filter((s) => !s.groupName);
  const groups = new Map<string, Series[]>();
  for (const item of series) {
    if (item.groupName) {
      const list = groups.get(item.groupName) ?? [];
      list.push(item);
      groups.set(item.groupName, list);
    }
  }
  const groupNames = Array.from(groups.keys()).sort((a, b) => a.localeCompare(b));
  return { ungrouped, groups, groupNames };
}

export function GroupedCoverGrid({
  series,
  loading,
  empty,
}: {
  series: Series[];
  loading?: boolean;
  empty?: ReactNode;
}) {
  if (loading) {
    return (
      <div className="grid grid-cols-3 gap-x-4 gap-y-6 md:grid-cols-5 xl:grid-cols-6">
        {Array.from({ length: 10 }).map((_, i) => (
          <CoverSkeleton key={i} />
        ))}
      </div>
    );
  }
  if (series.length === 0) return <>{empty}</>;

  const { reading, watching } = splitMedia(series);
  const readingGroups = groupedByName(reading);
  const watchingGroups = groupedByName(watching);

  return (
    <div className="space-y-10">
      {reading.length > 0 && (
        <section>
          {watching.length > 0 && (
            <div className="mb-4 border-b border-border pb-2">
              <h2 className="text-sm font-semibold tracking-tight">Bookshelf</h2>
              <p className="mt-0.5 text-[11px] text-muted">Manga, comics, and documents</p>
            </div>
          )}
          <div className="space-y-8">
            {readingGroups.ungrouped.length > 0 && <MediaGrid series={readingGroups.ungrouped} layout="portrait" />}
            {readingGroups.groupNames.map((name) => {
              const items = readingGroups.groups.get(name)!;
              return (
                <section key={`read-${name}`}>
                  <div className="mb-3 flex items-baseline gap-2 border-b border-border/60 pb-2">
                    <h3 className="text-sm font-semibold text-text">{name}</h3>
                    <span className="text-[11px] text-muted">
                      {items.length} {items.length === 1 ? "title" : "titles"}
                    </span>
                  </div>
                  <MediaGrid series={items} layout="portrait" />
                </section>
              );
            })}
          </div>
        </section>
      )}

      {watching.length > 0 && (
        <section>
          <div className="mb-4 border-b border-border pb-2">
            <h2 className="text-sm font-semibold tracking-tight">Watch</h2>
            <p className="mt-0.5 text-[11px] text-muted">Local video</p>
          </div>
          <div className="space-y-8">
            {watchingGroups.ungrouped.length > 0 && <MediaGrid series={watchingGroups.ungrouped} layout="landscape" />}
            {watchingGroups.groupNames.map((name) => {
              const items = watchingGroups.groups.get(name)!;
              return (
                <section key={`watch-${name}`}>
                  <div className="mb-3 flex items-baseline gap-2 border-b border-border/60 pb-2">
                    <h3 className="text-sm font-semibold text-text">{name}</h3>
                    <span className="text-[11px] text-muted">
                      {items.length} {items.length === 1 ? "title" : "titles"}
                    </span>
                  </div>
                  <MediaGrid series={items} layout="landscape" />
                </section>
              );
            })}
          </div>
        </section>
      )}
    </div>
  );
}

export function ShelfRow({
  title,
  series,
  layout,
}: {
  title: string;
  series: Series[];
  layout?: "portrait" | "landscape";
}) {
  if (series.length === 0) return null;
  const video = layout === "landscape" || series.every((item) => isVideoItem(item));
  return (
    <section className="mb-8">
      <h2 className="mb-4 text-sm font-semibold">{title}</h2>
      <div className="no-scrollbar flex gap-4 overflow-x-auto pb-2">
        {series.map((item) => (
          <Link
            key={item.id}
            to={`/series/${item.id}`}
            className={cn("shrink-0", video ? "w-44 md:w-52" : "w-[5.75rem] md:w-24")}
          >
            <OffsetMedia series={item} layout={video ? "landscape" : "portrait"} />
            <p className="mt-2 line-clamp-2 min-h-[2rem] text-[11px] leading-snug text-muted">{item.title}</p>
          </Link>
        ))}
      </div>
    </section>
  );
}
