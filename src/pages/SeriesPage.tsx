import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Heart, Play } from "lucide-react";
import { Button } from "../components/Button";
import { api, isThinReader, isTauri, isTimeoutError, uploadChapterFile, chapterDownloadUrl } from "../lib/api";
import { CoverImage } from "../components/CoverGrid";
import { StatusMessage } from "../components/StatusMessage";
import { Surface } from "../components/Surface";
import { cn, formatProgress } from "../lib/utils";
import {
  detectMediaKindFromSeries,
  effectiveReadingMode,
  isVideoItem,
  readingModeLabel,
  type Chapter,
  type Collection,
  type ReadingMode,
  type Series,
  type VariantPreference,
} from "../types";

type ChapterGroup = {
  key: string;
  primary: Chapter;
  variants: Chapter[];
};

function variant_label(title: string): string {
  const lower = title.toLowerCase();
  if (
    lower.includes("无修正") ||
    lower.includes("無修正") ||
    lower.includes("无码") ||
    lower.includes("無碼") ||
    lower.includes("去码") ||
    lower.includes("去碼")
  ) {
    return "Alt • Uncensored";
  }
  if (
    lower.includes("修正") ||
    lower.includes("修復") ||
    lower.includes("修复") ||
    lower.includes("revised") ||
    lower.includes("fix")
  ) {
    return "Alt • Revised";
  }
  if (
    lower.includes("補圖") ||
    lower.includes("补图") ||
    lower.includes("extra") ||
    lower.includes("bonus") ||
    lower.includes("番外") ||
    lower.includes("特別") ||
    lower.includes("特别") ||
    lower.includes("後記") ||
    lower.includes("后记") ||
    lower.includes("外傳") ||
    lower.includes("外传")
  ) {
    return "Alt • Bonus";
  }
  return "Alt";
}

function chapter_group_key(chapter: Chapter): string {
  if (typeof chapter.chapterNumber === "number") {
    return `ch:${chapter.chapterNumber.toFixed(3)}`;
  }
  if (typeof chapter.volumeNumber === "number") {
    return `vol:${chapter.volumeNumber}`;
  }
  return `single:${chapter.id}`;
}

function build_chapter_groups(chapters: Chapter[], sortNewest: boolean): ChapterGroup[] {
  const visible = chapters.filter((c) => !c.missing);
  const byKey = new Map<string, Chapter[]>();

  visible.forEach((chapter) => {
    const key = chapter_group_key(chapter);
    const existing = byKey.get(key);
    if (existing) {
      existing.push(chapter);
    } else {
      byKey.set(key, [chapter]);
    }
  });

  const groups: ChapterGroup[] = Array.from(byKey.entries()).map(([key, list]) => {
    const sorted = [...list].sort((a, b) => a.sortKey - b.sortKey || a.title.localeCompare(b.title));
    return {
      key,
      primary: sorted[0],
      variants: sorted.slice(1),
    };
  });

  groups.sort((a, b) => {
    const delta = a.primary.sortKey - b.primary.sortKey;
    if (delta !== 0) {
      return sortNewest ? -delta : delta;
    }
    return a.primary.title.localeCompare(b.primary.title);
  });

  return groups;
}

export function SeriesPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [series, setSeries] = useState<Series | null>(null);
  const [chapters, setChapters] = useState<Chapter[]>([]);
  const [sortNewest, setSortNewest] = useState(false);
  const [editingDesc, setEditingDesc] = useState(false);
  const [desc, setDesc] = useState("");
  const [tagInput, setTagInput] = useState("");
  const [collections, setCollections] = useState<Collection[]>([]);
  const [inCollections, setInCollections] = useState<number[]>([]);
  const [expandedVariants, setExpandedVariants] = useState<Record<string, boolean>>({});
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const reload = async (seriesId: number) => {
    setLoading(true);
    setLoadError(null);
    setSaveError(null);
    try {
      const [s, c, cols, ids] = await Promise.all([
        api.getSeries(seriesId),
        api.listChapters(seriesId),
        api.listCollections(),
        api.seriesCollections(seriesId),
      ]);
      setSeries(s);
      setChapters(c);
      setDesc(s.description ?? "");
      setTagInput((s.tags ?? []).join(", "));
      setCollections(cols);
      setInCollections(ids);
    } catch (error) {
      setLoadError(isTimeoutError(error) ? "Loading this series timed out. Try again." : "Could not load this series right now.");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!id) return;
    void reload(parseInt(id, 10));
    setExpandedVariants({});
  }, [id]);

  if (!series && loading) {
    return (
      <div className="flex h-full items-center justify-center px-6">
        <StatusMessage>Loading series...</StatusMessage>
      </div>
    );
  }

  if (!series) {
    return (
      <div className="flex h-full items-center justify-center px-6">
        <StatusMessage
          tone="error"
          live
          action={
            <Button variant="ghost" size="sm" className="text-accent" onClick={() => id && void reload(parseInt(id, 10))}>
              Retry
            </Button>
          }
        >
          {loadError ?? "This series could not be loaded."}
        </StatusMessage>
      </div>
    );
  }

  const chapterGroups = build_chapter_groups(chapters, sortNewest);
  const continueChapter =
    chapters.find((c) => !c.missing && c.progressPercent > 0 && c.progressPercent < 99) ??
    chapters.find((c) => !c.missing && c.progressPercent === 0) ??
    chapters.find((c) => !c.missing);
  const startChapter = chapters.find((c) => !c.missing);
  const isVideo = isVideoItem(series);
  const isMangaLike = ["manga", "manhwa", "comic"].includes(detectMediaKindFromSeries(series).contentType);
  const mode = effectiveReadingMode(series);

  const toggleFavorite = async () => {
    try {
      await api.updateSeries(series.id, { favorite: !series.favorite });
      setSeries({ ...series, favorite: !series.favorite });
      setSaveError(null);
    } catch {
      setSaveError("Could not update favorite. Your change was not saved.");
    }
  };

  const setReadingMode = async (m: ReadingMode) => {
    try {
      await api.updateSeries(series.id, { readingMode: m });
      setSeries({ ...series, readingMode: m });
      setSaveError(null);
    } catch {
      setSaveError("Could not update reading mode. Your change was not saved.");
    }
  };

  const setVariantPreference = async (preference: VariantPreference) => {
    try {
      await api.updateSeries(series.id, { variantPreference: preference });
      setSeries({ ...series, variantPreference: preference });
      setSaveError(null);
    } catch {
      setSaveError("Could not update version preference. Your change was not saved.");
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="flex shrink-0 items-center gap-4 border-b border-border px-4 py-4 md:px-6">
        <Button onClick={() => navigate(-1)} variant="ghost" size="sm" className="p-2" aria-label="Go back">
          <ArrowLeft size={20} />
        </Button>
        <h1 className="truncate text-xl font-semibold">{series.title}</h1>
      </header>

      <div className="flex-1 overflow-y-auto">
        {saveError && (
          <StatusMessage tone="error" live className="mx-4 mt-4 md:mx-6">
            {saveError}
          </StatusMessage>
        )}
        <div className="flex flex-col gap-6 px-4 py-8 md:flex-row md:gap-8 md:px-6">
          <div className={cn("shrink-0", isVideo ? "w-full max-w-xl md:w-[28rem]" : "w-40 md:w-48")}>
            <div className="shelf-card">
              <div className="shelf-card-shadow" />
              <div className={cn("shelf-card-face", isVideo ? "shelf-card-face--landscape" : "shelf-card-face--portrait")}>
                <CoverImage seriesId={series.id} title={series.title} />
              </div>
            </div>
          </div>
          <div className="flex-1">
            <div className="flex flex-wrap gap-3">
              {continueChapter && (
                <Link
                  to={`/read/${continueChapter.id}`}
                  className="inline-flex items-center gap-2 rounded-lg bg-accent px-5 py-2.5 text-sm font-medium text-bg hover:bg-accent-hover"
                >
                  <Play size={16} />
                  {isVideo
                    ? continueChapter.progressPercent > 0 ? "Continue Watching" : "Play"
                    : continueChapter.progressPercent > 0 ? "Continue Reading" : "Start Reading"}
                </Link>
              )}
              {startChapter && continueChapter?.id !== startChapter.id && (
                <Link
                  to={`/read/${startChapter.id}`}
                  className="inline-flex items-center rounded-lg border border-border px-4 py-2.5 text-sm text-muted hover:bg-border/40 hover:text-text"
                >
                  {isVideo ? "Play from start" : "Start from beginning"}
                </Link>
              )}
              {!isThinReader() && (
                <Button
                  onClick={toggleFavorite}
                  variant={series.favorite ? "danger" : "secondary"}
                  aria-pressed={series.favorite}
                  aria-label={series.favorite ? "Remove from favorites" : "Add to favorites"}
                  className={cn(
                    "px-4 py-2.5",
                    series.favorite ? "border-red-500/50 text-red-400" : "border-border text-muted",
                  )}
                >
                  <Heart size={16} fill={series.favorite ? "currentColor" : "none"} />
                  Favorite
                </Button>
              )}
              {isThinReader() && (
                <>
                  <input
                    ref={fileInputRef}
                    type="file"
                    accept=".pdf,.mp4,application/pdf,video/mp4"
                    className="hidden"
                    onChange={async (event) => {
                      const file = event.target.files?.[0];
                      event.target.value = "";
                      if (!file) return;
                      setUploading(true);
                      setSaveError(null);
                      try {
                        await uploadChapterFile(series.id, file);
                        await reload(series.id);
                      } catch (error) {
                        setSaveError(error instanceof Error ? error.message : "Upload failed.");
                      } finally {
                        setUploading(false);
                      }
                    }}
                  />
                  <Button
                    variant="secondary"
                    className="px-4 py-2.5"
                    disabled={uploading}
                    onClick={() => fileInputRef.current?.click()}
                  >
                    {uploading ? "Uploading…" : "Upload file"}
                  </Button>
                </>
              )}
            </div>

            <div className="mt-3 flex flex-wrap items-center gap-2 text-sm text-muted">
              <span>{series.chapterCount} {isVideo ? (series.chapterCount === 1 ? "episode" : "episodes") : (series.chapterCount === 1 ? "chapter" : "chapters")}</span>
              <span>·</span>
              <span>{formatProgress(series.progressPercent, isVideo)}</span>
            </div>

            <div className="mt-4 flex flex-wrap gap-2">
              {(series.tags ?? []).map((t) => (
                <span key={t} className="rounded-full bg-elevated px-2.5 py-0.5 text-xs text-muted">
                  {t}
                </span>
              ))}
            </div>

            {isThinReader() && (
              <StatusMessage className="mt-6">
                Reading stays fully available here. Editing series details, tags, and collections stays on the Mac app.
              </StatusMessage>
            )}

            {isTauri() && (
              <Surface className="mt-6 bg-elevated" padded={false}>
                <div className="px-4 py-4 md:px-5">
                  <p className="mb-3 text-xs font-medium uppercase tracking-[0.12em] text-muted">Details</p>
                  {editingDesc ? (
                    <div>
                      <label htmlFor="series-description" className="mb-1 block text-xs text-muted">
                        Description
                      </label>
                      <textarea
                        id="series-description"
                        value={desc}
                        onChange={(e) => setDesc(e.target.value)}
                        className="h-24 w-full rounded-lg border border-border bg-surface p-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
                      />
                      <Button
                        variant="primary"
                        size="sm"
                        className="mt-3"
                        onClick={async () => {
                          try {
                            await api.updateSeries(series.id, { description: desc });
                            setSeries({ ...series, description: desc });
                            setEditingDesc(false);
                            setSaveError(null);
                          } catch {
                            setSaveError("Could not save the description. Your change was not saved.");
                          }
                        }}
                      >
                        Save description
                      </Button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      onClick={() => setEditingDesc(true)}
                      aria-label={series.description ? "Edit series description" : "Add series description"}
                      className="text-left text-muted transition-colors hover:text-text"
                    >
                      {series.description || "Add a description..."}
                    </button>
                  )}
                </div>

                <div className="border-t border-border px-4 py-4 md:px-5">
                  <label htmlFor="series-tags" className="text-xs text-muted">Tags (comma separated)</label>
                  <input
                    id="series-tags"
                    aria-label="Series tags"
                    value={tagInput}
                    onChange={(e) => setTagInput(e.target.value)}
                    onBlur={async () => {
                      try {
                        await api.setSeriesTags(
                          series.id,
                          tagInput.split(",").map((t) => t.trim()).filter(Boolean),
                        );
                        await reload(series.id);
                      } catch {
                        setSaveError("Could not save tags. Your change was not saved.");
                      }
                    }}
                    className="mt-1 w-full rounded-lg border border-border bg-surface px-3 py-2 text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
                  />
                </div>

                {collections.length > 0 && (
                  <div className="border-t border-border px-4 py-4 md:px-5">
                    <p className="mb-2 text-xs text-muted">Collections</p>
                    <div className="flex flex-wrap gap-2">
                      {collections.map((c) => {
                        const on = inCollections.includes(c.id);
                        return (
                          <Button
                            key={c.id}
                            onClick={async () => {
                              try {
                                await api.setCollectionItem(c.id, series.id, !on);
                                await reload(series.id);
                              } catch {
                                setSaveError(`Could not update the ${c.name} collection. Your change was not saved.`);
                              }
                            }}
                            variant={on ? "primary" : "secondary"}
                            size="sm"
                            aria-pressed={on}
                            aria-label={`${on ? "Remove from" : "Add to"} collection ${c.name}`}
                            className={cn("rounded-full", !on && "text-muted")}
                          >
                            {c.name}
                          </Button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </Surface>
            )}

            {!isVideo && (
            <Surface className="mt-6 bg-elevated" padded={false}>
              <div className="px-4 py-4 md:px-5">
                {isMangaLike ? (
                  <>
                    <p className="mb-2 text-xs font-medium uppercase tracking-[0.12em] text-muted">Reader</p>
                    <p className="text-sm text-muted">
                      Chapters open as one continuous strip. Scroll through the whole chapter; pages sit flush with no gaps.
                    </p>
                  </>
                ) : (
                  <>
                    <p className="mb-2 text-xs font-medium uppercase tracking-[0.12em] text-muted">Reading mode</p>
                    <div className="text-sm">
                      <span className="text-muted">Current: </span>
                      <span>{readingModeLabel(series.readingMode)}</span>
                      {series.readingMode === "auto" && (
                        <span className="text-muted"> (detected: {mode})</span>
                      )}
                    </div>
                    {!isThinReader() && (
                    <div className="mt-3 flex flex-wrap gap-2">
                      {(["auto", "webtoon", "paged_ltr", "paged_rtl"] as ReadingMode[]).map((m) => (
                        <Button
                          key={m}
                          onClick={() => setReadingMode(m)}
                          variant={series.readingMode === m ? "primary" : "secondary"}
                          size="sm"
                          aria-pressed={series.readingMode === m}
                          aria-label={`Set reading mode to ${readingModeLabel(m)}`}
                          className={cn("rounded-full", series.readingMode !== m && "text-muted")}
                        >
                          {readingModeLabel(m)}
                        </Button>
                      ))}
                    </div>
                    )}
                  </>
                )}
                {!isThinReader() && (
                <div className="mt-4">
                  <label htmlFor="variant-preference" className="text-xs text-muted">Version preference</label>
                  <p className="mt-1 text-xs text-muted">
                    Primary is the default. Choose an alternate type to prefer it when available.
                  </p>
                  <select
                    id="variant-preference"
                    aria-label="Variant preference"
                    value={series.variantPreference ?? "primary"}
                    onChange={(e) => setVariantPreference(e.target.value as VariantPreference)}
                    className="mt-2 w-full rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
                  >
                    <option value="primary">Primary</option>
                    <option value="uncensored">Prefer Uncensored</option>
                    <option value="revised">Prefer Revised</option>
                    <option value="bonus">Prefer Bonus</option>
                    <option value="alternate">Prefer Any Alternate</option>
                  </select>
                </div>
                )}
              </div>
            </Surface>
            )}
          </div>
        </div>

        <div className="border-t border-border px-4 py-6 md:px-6">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-lg font-semibold">{isVideo ? "Episodes" : "Chapters"} ({chapters.length})</h2>
            <Button onClick={() => setSortNewest(!sortNewest)} variant="ghost" size="sm" className="px-0 hover:bg-transparent">
              {sortNewest ? "Oldest first" : "Newest first"}
            </Button>
          </div>
          <div className="grid gap-2">
            {chapterGroups.map((group) => {
              const chapter = group.primary;
              const hasVariants = group.variants.length > 0;
              const expanded = Boolean(expandedVariants[group.key]);

              return (
                <div key={group.key} className="space-y-2">
                  <Link
                    to={`/read/${chapter.id}`}
                    className="flex items-center justify-between rounded-xl bg-elevated px-4 py-3 hover:bg-border/40"
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-3">
                      {chapter.progressPercent < 1 && (
                        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <p className="text-sm font-medium">{chapter.title}</p>
                          {!isVideo && (
                            <span className="rounded-md bg-accent/15 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-accent">
                              Primary
                            </span>
                          )}
                          {!isVideo && hasVariants && (
                            <span className="rounded-md bg-surface px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-muted">
                              +{group.variants.length} alt
                            </span>
                          )}
                        </div>
                        <p className="text-xs text-muted">
                          {isVideo
                            ? formatProgress(chapter.progressPercent, true)
                            : chapter.pageCount > 0
                              ? `${chapter.pageCount} pages`
                              : "Chapter"}
                        </p>
                        <div className="mt-2 h-1 max-w-56 overflow-hidden rounded-full bg-border">
                          <div
                            className="h-full bg-accent"
                            style={{ width: `${Math.min(100, Math.max(0, chapter.progressPercent))}%` }}
                          />
                        </div>
                      </div>
                    </div>
                    <span className="ml-3 shrink-0 text-xs text-muted">{formatProgress(chapter.progressPercent, isVideo)}</span>
                  </Link>
                  {isThinReader() && (
                    <a
                      href={chapterDownloadUrl(chapter.id)}
                      className="block px-4 text-xs text-accent hover:text-accent-hover"
                    >
                      Download {chapter.title}
                    </a>
                  )}

                  {!isVideo && hasVariants && (
                    <div className="flex flex-wrap items-center gap-3 pl-4">
                      <Link
                        to={`/read/${group.variants[0].id}`}
                        className="text-xs text-muted underline-offset-2 hover:text-text hover:underline"
                        aria-label={`Open quick alternate version for ${chapter.title}`}
                      >
                        Quick open alt
                      </Link>
                      <button
                        type="button"
                        onClick={() =>
                          setExpandedVariants((prev) => ({
                            ...prev,
                            [group.key]: !prev[group.key],
                          }))
                        }
                        className="text-xs text-accent hover:text-accent-hover"
                        aria-expanded={expanded}
                        aria-label={`${expanded ? "Hide" : "Show"} ${group.variants.length} alternate versions for ${chapter.title}`}
                      >
                        {expanded ? "Hide" : "Show"} {group.variants.length} other version{group.variants.length === 1 ? "" : "s"}
                      </button>
                    </div>
                  )}

                  {!isVideo && expanded &&
                    group.variants.map((variant) => (
                      <Link
                        key={variant.id}
                        to={`/read/${variant.id}`}
                        className="ml-4 flex items-center justify-between rounded-xl border border-border/70 bg-surface px-4 py-2.5 hover:bg-elevated"
                      >
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <p className="text-sm text-text">{variant.title}</p>
                            <span className="rounded-md bg-elevated px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-muted">
                              {variant_label(variant.title)}
                            </span>
                          </div>
                          <p className="text-xs text-muted">
                            {variant.pageCount > 0 ? `${variant.pageCount} pages` : "…"}
                          </p>
                          <div className="mt-2 h-1 max-w-56 overflow-hidden rounded-full bg-border">
                            <div
                              className="h-full bg-accent"
                              style={{ width: `${Math.min(100, Math.max(0, variant.progressPercent))}%` }}
                            />
                          </div>
                        </div>
                        <span className="ml-3 shrink-0 text-xs text-muted">{formatProgress(variant.progressPercent)}</span>
                      </Link>
                    ))}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
