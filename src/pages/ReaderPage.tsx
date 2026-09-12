import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type RefObject, type SetStateAction } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  Languages,
  Maximize,
  Minimize,
  MoreHorizontal,
  X,
} from "lucide-react";
import { api, chapterFileUrl, getDeviceId, isTimeoutError, loadTileObjectUrl, revokeObjectUrl } from "../lib/api";
import { applyFullscreen, readFullscreen } from "../lib/fullscreen";
import { isTypingTarget, resolveReaderShortcut } from "../lib/readerShortcuts";
import { Select } from "../components/Select";
import { clamp } from "../lib/utils";
import { StatusMessage } from "../components/StatusMessage";
import { RegionEditDialog, TranslationOverlay } from "../components/TranslationOverlay";
import {
  chapterGroupKey,
  chapterVariantLabel,
  currentChapterVariantPreference,
  detectViewerKindFromItem,
  groupTilesByPage,
  nextVariantPreference,
  previousVariantPreference,
  preferredChapterSequence,
  untranslatedCjkNotice,
  variantPreferenceLabel,
  type Chapter,
  type OcrRegion,
  type OverlayMode,
  type PageTile,
  type Series,
  type VariantPreference,
} from "../types";

const PREFETCH_WINDOW = 10;
const readerIconButton = "reader-btn rounded-lg p-2";
const readerTextButton = "reader-btn rounded-lg px-2 py-1.5 text-xs";

type ChapterOption = {
  chapter: Chapter;
  label: string;
};

function buildChapterOptions(chapters: Chapter[]): ChapterOption[] {
  const visible = chapters.filter((c) => !c.missing);
  const byKey = new Map<string, Chapter[]>();
  visible.forEach((chapter) => {
    const key = chapterGroupKey(chapter);
    const existing = byKey.get(key);
    if (existing) existing.push(chapter);
    else byKey.set(key, [chapter]);
  });

  const grouped = Array.from(byKey.values())
    .map((group) => [...group].sort((a, b) => a.sortKey - b.sortKey || a.title.localeCompare(b.title)))
    .sort((a, b) => a[0]!.sortKey - b[0]!.sortKey || a[0]!.title.localeCompare(b[0]!.title));

  const options: ChapterOption[] = [];
  grouped.forEach((group) => {
    const primary = group[0]!;
    const variants = group.slice(1);
    options.push({
      chapter: primary,
      label: variants.length > 0 ? `${primary.title} (Primary, +${variants.length} alt)` : `${primary.title} (Primary)`,
    });
    variants.forEach((variant) => {
      options.push({
        chapter: variant,
        label: `  ${variant.title} (${chapterVariantLabel(variant.title)})`,
      });
    });
  });

  return options;
}

function stripRenderWidth() {
  const ratio = typeof window === "undefined" ? 1 : Math.min(1.5, window.devicePixelRatio || 1);
  const base = typeof window === "undefined" ? 1080 : window.innerWidth;
  return Math.min(1200, Math.max(720, Math.round(base * ratio)));
}

/** How far outside the viewport a segment starts loading. */
const SEGMENT_PRELOAD_MARGIN = "1200px 0px";

/**
 * One stacked segment of the strip. The segment reserves its exact height from the
 * manifest before any pixels arrive, so the strip never reflows and scroll restoration
 * lands in the right place. Pixels are fetched only once the segment nears the viewport.
 */
function StripSegment({
  chapterId,
  tile,
  width,
  onReady,
}: {
  chapterId: number;
  tile: PageTile;
  width: number;
  onReady: () => void;
}) {
  const holderRef = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(false);
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    if (visible) return;
    const holder = holderRef.current;
    if (!holder || typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setVisible(true);
      },
      { root: holder.closest(".reader-strip"), rootMargin: SEGMENT_PRELOAD_MARGIN },
    );
    observer.observe(holder);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    let objectUrl: string | null = null;
    setError(null);

    void loadTileObjectUrl(chapterId, tile.pageIndex, tile.tileIndex, width)
      .then((url) => {
        if (cancelled) {
          revokeObjectUrl(url);
          return;
        }
        objectUrl = url;
        setSrc(url);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        const message = cause instanceof Error ? cause.message : "";
        setError(
          isTimeoutError(cause)
            ? "Loading this section timed out."
            : message && message !== "prefetch"
              ? message
              : "This section could not be rendered.",
        );
      });

    return () => {
      cancelled = true;
      revokeObjectUrl(objectUrl);
    };
  }, [visible, chapterId, tile.pageIndex, tile.tileIndex, width, nonce]);

  return (
    <div
      ref={holderRef}
      data-page={tile.pageIndex}
      data-tile={tile.tileIndex}
      className="relative w-full bg-black"
      style={{ aspectRatio: `${tile.width} / ${tile.height}` }}
    >
      {src && !error ? (
        <img
          src={src}
          alt=""
          className="reader-segment absolute inset-0 h-full w-full"
          decoding="async"
          onLoad={onReady}
          onError={() => setError("This section could not be rendered.")}
        />
      ) : null}

      {error ? (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted">
          <p>{error}</p>
          <button
            type="button"
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-bg"
            onClick={() => {
              setSrc(null);
              setNonce((value) => value + 1);
            }}
          >
            Retry
          </button>
        </div>
      ) : null}
    </div>
  );
}

function VideoViewer({
  chapter,
  series,
  onBack,
  fullscreen,
  toggleFullscreen,
  containerRef,
  hudVisible,
  showHud,
}: {
  chapter: Chapter;
  series: Series;
  onBack: () => void;
  fullscreen: boolean;
  toggleFullscreen: () => Promise<void>;
  containerRef: RefObject<HTMLDivElement | null>;
  hudVisible: boolean;
  showHud: () => void;
}) {
  const src = chapterFileUrl(chapter.id);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const restoredProgress = useRef(false);
  const positionRef = useRef(0);
  const durationRef = useRef(0);
  const [videoError, setVideoError] = useState<string | null>(chapter.filePath ? null : "This video file is unavailable.");

  useEffect(() => {
    if (!chapter) return;
    setVideoError(chapter.filePath ? null : "This video file is unavailable.");
    restoredProgress.current = false;
    void api.getProgress(chapter.id).then((progress) => {
      const video = videoRef.current;
      if (!video || !progress?.positionSeconds) return;
      const position = Math.max(0, progress.positionSeconds);
      positionRef.current = position;
      if (video.readyState >= 1) video.currentTime = position;
      restoredProgress.current = true;
    }).catch(() => {
      restoredProgress.current = true;
    });
  }, [chapter]);

  const saveVideoProgress = useCallback(async () => {
    if (!chapter || !restoredProgress.current || durationRef.current <= 0) return;
    await api.saveProgress({
      chapterId: chapter.id,
      pageIndex: 0,
      scrollOffset: 0,
      positionSeconds: positionRef.current,
      durationSeconds: durationRef.current,
      percent: clamp((positionRef.current / durationRef.current) * 100, 0, 100),
      updatedAt: new Date().toISOString(),
      deviceId: getDeviceId(),
    });
  }, [chapter]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      void saveVideoProgress().catch(() => {});
    }, 4000);
    return () => {
      window.clearInterval(interval);
      void saveVideoProgress().catch(() => {});
    };
  }, [saveVideoProgress]);

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      className="reader-shell relative h-dvh min-h-0 w-full overflow-hidden bg-black outline-none"
      onMouseMove={showHud}
    >
      <header
        className={`absolute inset-x-0 top-0 z-20 flex items-center justify-between bg-gradient-to-b from-bg/90 to-transparent px-4 pb-3 pt-[calc(0.75rem+env(safe-area-inset-top))] transition-opacity duration-300 ${
          hudVisible ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
      >
        <div className="flex min-w-0 items-center gap-3">
          <button onClick={onBack} className={readerIconButton} aria-label="Close video viewer">
            <X size={20} />
          </button>
          <div className="min-w-0">
            <p className="truncate text-sm font-medium text-text">{series.title}</p>
            <p className="truncate text-xs text-muted">{chapter.title}</p>
          </div>
        </div>
        <button
          onClick={() => void toggleFullscreen()}
          aria-label={fullscreen ? "Exit fullscreen" : "Enter fullscreen"}
          className={readerIconButton}
        >
          {fullscreen ? <Minimize size={18} /> : <Maximize size={18} />}
        </button>
      </header>

      <video
        ref={videoRef}
        className="absolute inset-0 h-full w-full bg-black object-contain"
        src={src}
        preload="metadata"
        controls
        playsInline
        controlsList="nofullscreen"
        onError={() => {
          setVideoError("This video could not be loaded. The file may be missing or use an unsupported codec.");
        }}
        onLoadedMetadata={(e) => {
          const nextDuration = e.currentTarget.duration || 0;
          durationRef.current = nextDuration;
          if (!restoredProgress.current) {
            e.currentTarget.currentTime = positionRef.current;
            restoredProgress.current = true;
          }
        }}
        onTimeUpdate={(e) => {
          positionRef.current = e.currentTarget.currentTime || 0;
        }}
      />

      {videoError && (
        <div className="absolute inset-x-4 bottom-[calc(5rem+env(safe-area-inset-bottom))] z-20 mx-auto max-w-lg">
          <StatusMessage
            tone="error"
            live
            action={
              <button
                type="button"
                onClick={() => {
                  setVideoError(null);
                  videoRef.current?.load();
                }}
                className="text-accent hover:text-accent-hover"
              >
                Retry
              </button>
            }
          >
            {videoError}
          </StatusMessage>
        </div>
      )}
    </div>
  );
}

function MangaViewer({
  chapter,
  series,
  chapters,
  currentPage,
  hudVisible,
  setHudVisible,
  fullscreen,
  toggleFullscreen,
  zoom,
  setZoom,
  overlayMode,
  setOverlayMode,
  regions,
  setRegions,
  editing,
  setEditing,
  translating,
  translationError,
  retryTranslation,
  onTranslationError,
  endOfChapter,
  setEndOfChapter,
  containerRef,
  webtoonRef,
  goChapter,
  navigate,
  showHud,
  handleWebtoonScroll,
  progressPct,
  pageWidth,
  tiles,
  onSeek,
  onPageReady,
  onUseCurrentVariantPreference,
  applyingPreference,
  currentVariantPreference,
}: {
  chapter: Chapter;
  series: Series;
  chapters: Chapter[];
  currentPage: number;
  hudVisible: boolean;
  setHudVisible: Dispatch<SetStateAction<boolean>>;
  fullscreen: boolean;
  toggleFullscreen: () => Promise<void>;
  zoom: number;
  setZoom: Dispatch<SetStateAction<number>>;
  overlayMode: OverlayMode;
  setOverlayMode: (value: OverlayMode | ((prev: OverlayMode) => OverlayMode)) => void;
  regions: OcrRegion[];
  setRegions: (value: OcrRegion[]) => void;
  editing: OcrRegion | null;
  setEditing: (value: OcrRegion | null) => void;
  translating: boolean;
  translationError: string | null;
  retryTranslation: () => void;
  onTranslationError: (message: string) => void;
  endOfChapter: boolean;
  setEndOfChapter: (value: boolean) => void;
  containerRef: RefObject<HTMLDivElement | null>;
  webtoonRef: RefObject<HTMLDivElement | null>;
  goChapter: (next: boolean) => Promise<void>;
  navigate: (path: string, opts?: { replace?: boolean }) => void;
  showHud: () => void;
  handleWebtoonScroll: () => void;
  progressPct: number;
  pageWidth: number;
  tiles: PageTile[];
  onSeek: (percent: number) => void;
  onPageReady: () => void;
  onUseCurrentVariantPreference: () => Promise<void>;
  applyingPreference: boolean;
  currentVariantPreference: VariantPreference;
}) {
  const chapterOptions = buildChapterOptions(chapters);
  const pageGroups = useMemo(() => groupTilesByPage(tiles), [tiles]);

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      className="reader-shell relative h-dvh min-h-0 w-full overflow-hidden bg-black outline-none"
      onMouseMove={showHud}
      onClick={() => setHudVisible((v) => !v)}
    >
      <div
        className={`absolute inset-x-0 top-0 z-20 flex items-center justify-between bg-gradient-to-b from-bg/90 to-transparent px-4 pb-3 pt-[calc(0.75rem+env(safe-area-inset-top))] transition-opacity duration-300 ${
          hudVisible ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3">
          <button onClick={() => navigate(`/series/${series.id}`)} className={readerIconButton} aria-label="Close reader">
            <X size={20} />
          </button>
          <div className="min-w-0">
            <p className="truncate text-sm font-medium text-text">{series.title}</p>
            <p className="truncate text-xs text-muted">{chapter.title}</p>
          </div>
        </div>
        <div className="flex min-w-0 items-center justify-end gap-1 sm:gap-2">
          <Select
            value={chapter.id}
            onChange={(e) => navigate(`/read/${e.target.value}`, { replace: true })}
            aria-label="Select chapter"
            className="reader-select max-w-[7rem] truncate px-2 py-1 text-xs sm:max-w-[10rem]"
          >
            {chapterOptions.map((option) => (
              <option key={option.chapter.id} value={option.chapter.id}>
                {option.label}
              </option>
            ))}
          </Select>
          <button onClick={() => goChapter(false)} className={readerTextButton} aria-label="Open previous chapter">
            Prev
          </button>
          <button onClick={() => goChapter(true)} className={readerTextButton} aria-label="Open next chapter">
            Next
          </button>
          <button
            onClick={() => void onUseCurrentVariantPreference()}
            className={`${readerTextButton} hidden md:inline-flex`}
            aria-label={`Set ${variantPreferenceLabel(currentVariantPreference)} as preferred variant`}
            title={`Use ${variantPreferenceLabel(currentVariantPreference)} for next/prev chapter navigation in this series (V next, Shift+V previous)`}
            disabled={applyingPreference}
          >
            {applyingPreference ? "Saving..." : `Prefer ${variantPreferenceLabel(currentVariantPreference)}`}
          </button>
          <button
            onClick={() => setOverlayMode((m) => (m === "off" ? "on" : m === "on" ? "both" : "off"))}
            className={`${readerIconButton} hidden md:inline-flex ${overlayMode === "off" ? "text-muted" : "text-accent"}`}
            aria-label="Toggle translation overlay"
            aria-pressed={overlayMode !== "off"}
            title="Toggle translation (T)"
          >
            <Languages size={18} />
          </button>
          <button
            onClick={() => void toggleFullscreen()}
            aria-label={fullscreen ? "Exit fullscreen" : "Enter fullscreen"}
            className={`${readerIconButton} hidden md:inline-flex`}
          >
            {fullscreen ? <Minimize size={18} /> : <Maximize size={18} />}
          </button>
          <details className="relative md:hidden">
            <summary className={`${readerIconButton} list-none`} aria-label="More reader controls">
              <MoreHorizontal size={18} />
            </summary>
            <div className="absolute right-0 top-11 z-40 grid min-w-44 gap-1 rounded-lg border border-border bg-elevated p-1 shadow-xl">
              <button
                type="button"
                onClick={() => void onUseCurrentVariantPreference()}
                disabled={applyingPreference}
                className="reader-btn rounded-md px-3 py-2 text-left text-xs"
              >
                {applyingPreference ? "Saving..." : `Prefer ${variantPreferenceLabel(currentVariantPreference)}`}
              </button>
              <button
                type="button"
                onClick={() => setOverlayMode((m) => (m === "off" ? "on" : m === "on" ? "both" : "off"))}
                className="reader-btn rounded-md px-3 py-2 text-left text-xs"
                aria-pressed={overlayMode !== "off"}
              >
                {overlayMode === "off" ? "Show translation" : "Hide translation"}
              </button>
              <button
                type="button"
                onClick={() => void toggleFullscreen()}
                className="reader-btn rounded-md px-3 py-2 text-left text-xs"
              >
                {fullscreen ? "Exit fullscreen" : "Enter fullscreen"}
              </button>
            </div>
          </details>
        </div>
      </div>

      <div
        ref={webtoonRef}
        tabIndex={-1}
        className="reader-strip absolute inset-0 overflow-y-auto overscroll-contain outline-none"
        onScroll={handleWebtoonScroll}
        onClick={(e) => {
          e.stopPropagation();
          webtoonRef.current?.focus({ preventScroll: true });
        }}
      >
        <div
          className="mx-auto leading-[0]"
          style={{ width: `${Math.min(100, 100 * zoom)}%`, maxWidth: `${48 * zoom}rem` }}
        >
          {pageGroups.map((page) => (
            <div key={`${chapter.id}-${page.pageIndex}`} className="relative">
              {page.tiles.map((tile) => (
                <StripSegment
                  key={`${chapter.id}-${tile.pageIndex}-${tile.tileIndex}`}
                  chapterId={chapter.id}
                  tile={tile}
                  width={pageWidth}
                  onReady={onPageReady}
                />
              ))}
              {page.pageIndex === currentPage ? (
                <TranslationOverlay regions={regions} mode={overlayMode} onEdit={setEditing} />
              ) : null}
            </div>
          ))}
        </div>
      </div>

      {endOfChapter && (
        <div className="absolute inset-x-0 bottom-[calc(6rem+env(safe-area-inset-bottom))] z-30 mx-auto w-max rounded-xl border border-border bg-surface px-5 py-3 text-sm shadow-lg">
          End of chapter
          <button onClick={() => goChapter(true)} className="ml-3 text-accent" aria-label="Open next chapter">
            Next chapter {"->"}
          </button>
          <button onClick={() => setEndOfChapter(false)} className="ml-3 text-muted" aria-label="Stay on this chapter">
            Stay
          </button>
        </div>
      )}

      {translating && overlayMode !== "off" && (
        <div className="absolute right-4 top-[calc(4rem+env(safe-area-inset-top))] z-30 text-xs text-muted">Translating...</div>
      )}

      {translationError && overlayMode !== "off" && (
        <StatusMessage
          tone="error"
          live
          action={
            <button
              type="button"
              onClick={retryTranslation}
              className="text-accent hover:text-accent-hover"
            >
              Retry
            </button>
          }
          className="absolute inset-x-4 top-16 z-30 mx-auto max-w-md text-xs"
        >
          {translationError}
        </StatusMessage>
      )}

      {overlayMode !== "off" && !translating && !translationError && untranslatedCjkNotice(regions) && (
        <div className="absolute inset-x-0 top-16 z-30 mx-auto w-max max-w-[min(28rem,90vw)] rounded-lg border border-border bg-surface/95 px-3 py-2 text-center text-xs text-muted">
          Apple Translation is not available here. Japanese and Korean stay as OCR text; Simplified
          Chinese still converts locally.
        </div>
      )}

      {editing && (
        <RegionEditDialog
          region={editing}
          onClose={() => setEditing(null)}
          onSave={async (updated) => {
            const previous = regions;
            const next = regions.map((r) => (r.id === updated.id ? updated : r));
            setRegions(next);
            setEditing(null);
            try {
              await api.saveTranslationEdits(chapter.id, currentPage, next);
            } catch {
              setRegions(previous);
              onTranslationError("Could not save this translation edit. Your change was not saved.");
            }
          }}
        />
      )}

      <div
        className={`absolute inset-x-0 bottom-0 z-20 bg-gradient-to-t from-bg/90 to-transparent px-4 pt-4 pb-[calc(1rem+env(safe-area-inset-bottom))] transition-opacity duration-300 ${
          hudVisible ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mx-auto flex max-w-2xl items-center gap-4">
          <input
            type="range"
            aria-label="Reading position"
            min={0}
            max={100}
            step={0.1}
            value={clamp(progressPct, 0, 100)}
            onChange={(e) => onSeek(parseFloat(e.target.value))}
            className="flex-1 accent-accent"
          />
          <span className="min-w-[3rem] text-right text-xs text-muted">{Math.round(progressPct)}%</span>
          <label className="flex items-center gap-1 text-xs text-muted">
            {Math.round(zoom * 100)}%
            <input type="range" aria-label="Zoom level" min={1} max={2} step={0.1} value={zoom} onChange={(e) => setZoom(parseFloat(e.target.value))} className="w-16 accent-accent" />
          </label>
        </div>
      </div>
    </div>
  );
}

export function ReaderPage() {
  const { chapterId } = useParams<{ chapterId: string }>();
  const navigate = useNavigate();
  const cid = parseInt(chapterId ?? "0", 10);

  const [chapter, setChapter] = useState<Chapter | null>(null);
  const [series, setSeries] = useState<Series | null>(null);
  const [chapters, setChapters] = useState<Chapter[]>([]);
  const [currentPage, setCurrentPage] = useState(0);
  const [hudVisible, setHudVisible] = useState(true);
  const [fullscreen, setFullscreen] = useState(false);
  const [scrollOffset, setScrollOffset] = useState(0);
  const [scrollPct, setScrollPct] = useState(0);
  const [zoom, setZoom] = useState(1);
  const [overlayMode, setOverlayMode] = useState<OverlayMode>("off");
  const [regions, setRegions] = useState<OcrRegion[]>([]);
  const [editing, setEditing] = useState<OcrRegion | null>(null);
  const [translating, setTranslating] = useState(false);
  const [translationError, setTranslationError] = useState<string | null>(null);
  const [translationRetry, setTranslationRetry] = useState(0);
  const [endOfChapter, setEndOfChapter] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [applyingPreference, setApplyingPreference] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [noticeTone, setNoticeTone] = useState<"success" | "error">("success");
  const [pageWidth, setPageWidth] = useState(() => stripRenderWidth());
  const [tiles, setTiles] = useState<PageTile[]>([]);

  const hudTimer = useRef<number | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const webtoonRef = useRef<HTMLDivElement>(null);
  const loadGen = useRef(0);
  const restoredScroll = useRef(false);
  const pendingScroll = useRef(0);

  const primaryChapters = useMemo(
    () => preferredChapterSequence(chapters, series?.variantPreference ?? "primary"),
    [chapters, series?.variantPreference],
  );
  const activeChapterPreference = useMemo(
    () => (chapter ? currentChapterVariantPreference(chapter, chapters) : "primary"),
    [chapter, chapters],
  );

  const showHud = useCallback(() => {
    setHudVisible(true);
    if (hudTimer.current) window.clearTimeout(hudTimer.current);
    hudTimer.current = window.setTimeout(() => setHudVisible(false), 2800);
  }, []);

  useEffect(() => {
    showHud();
    return () => {
      if (hudTimer.current) window.clearTimeout(hudTimer.current);
    };
  }, [showHud]);

  const tryRestoreScroll = useCallback(() => {
    const el = webtoonRef.current;
    if (!el || restoredScroll.current) return;
    const target = pendingScroll.current;
    if (target <= 0) {
      restoredScroll.current = true;
      return;
    }
    if (el.scrollHeight > target + el.clientHeight * 0.2) {
      el.scrollTop = target;
      restoredScroll.current = true;
    }
  }, []);

  useEffect(() => {
    if (!cid) return;
    loadGen.current += 1;
    const gen = loadGen.current;
    restoredScroll.current = false;
    pendingScroll.current = 0;
    setLoadError(null);
    setChapter(null);
    setSeries(null);
    setRegions([]);
    setEndOfChapter(false);
    setScrollPct(0);
    setCurrentPage(0);
    setTiles([]);
    const width = stripRenderWidth();
    setPageWidth(width);

    (async () => {
      try {
        const ch = await api.getChapter(cid);
        const s = await api.getSeries(ch.seriesId);
        const list = await api.listChapters(ch.seriesId);
        if (gen !== loadGen.current) return;
        const viewerKind = detectViewerKindFromItem(ch, s);
        if (viewerKind === "video") {
          setChapter(ch);
          setSeries(s);
          setChapters(list);
          return;
        }
        const pageCount = Math.max(ch.pageCount, 1);
        const resolvedChapter = { ...ch, pageCount };
        setChapter(resolvedChapter);
        setSeries(s);
        setChapters(list);
        void api.getProgress(cid).then((progress) => {
          if (gen !== loadGen.current) return;
          const startPage = progress?.pageIndex ?? 0;
          setCurrentPage(startPage);
          pendingScroll.current = progress?.scrollOffset ?? 0;
          setScrollOffset(pendingScroll.current);
          restoredScroll.current = false;
          tryRestoreScroll();
        }).catch(() => {});
        if (ch.filePath.toLowerCase().endsWith(".pdf")) {
          void api.getPageCount(cid).then((count) => {
            if (gen !== loadGen.current || count <= 0) return;
            setChapter((prev) => (prev && prev.id === cid ? { ...prev, pageCount: count } : prev));
          }).catch(() => {});

          // The manifest is derived from page dimensions alone, so it returns before any
          // page is rasterized and lets the strip lay itself out immediately.
          try {
            const manifest = await api.getChapterTiles(cid, width);
            if (gen !== loadGen.current) return;
            setTiles(manifest);
            tryRestoreScroll();
          } catch (error) {
            if (gen !== loadGen.current) return;
            const detail = error instanceof Error ? error.message : "";
            setLoadError(detail || "Could not read the pages in this file.");
            return;
          }
        }
        const warmCount = resolvedChapter.pageCount <= 20 ? resolvedChapter.pageCount : Math.min(PREFETCH_WINDOW, resolvedChapter.pageCount);
        const warm = Array.from({ length: warmCount }, (_, index) => index);
        if (warm.length) {
          api.prefetchPages(cid, warm, width).catch(() => {});
        }
      } catch (error) {
        if (gen !== loadGen.current) return;
        const detail = error instanceof Error ? error.message : "";
        setLoadError(
          isTimeoutError(error)
            ? "Loading this chapter timed out. Please try again."
            : detail || "Could not load this chapter. Please try again.",
        );
      }
    })();
  }, [cid]);

  useEffect(() => {
    if (overlayMode === "off" || !cid) {
      setTranslationError(null);
      return;
    }
    let cancelled = false;
    setTranslating(true);
    setTranslationError(null);
    api
      .getPageTranslation(cid, currentPage)
      .then((r) => {
        if (!cancelled) setRegions(r);
      })
      .catch((error) => {
        if (!cancelled) {
          setRegions([]);
          setTranslationError(isTimeoutError(error) ? "Translation timed out. Try again." : "Could not load translation for this page.");
        }
      })
      .finally(() => {
        if (!cancelled) setTranslating(false);
      });
    return () => {
      cancelled = true;
    };
  }, [overlayMode, cid, currentPage, translationRetry]);

  const saveProgress = useCallback(async () => {
    if (!chapter) return;
    try {
      await api.saveProgress({
        chapterId: cid,
        pageIndex: currentPage,
        scrollOffset,
        percent: clamp(scrollPct, 0, 100),
        updatedAt: new Date().toISOString(),
        deviceId: getDeviceId(),
      });
      setSaveError(null);
    } catch {
      setSaveError("Progress is not syncing right now. Retrying...");
    }
  }, [chapter, cid, currentPage, scrollOffset, scrollPct]);

  useEffect(() => {
    const interval = setInterval(saveProgress, 4000);
    return () => {
      clearInterval(interval);
      saveProgress();
    };
  }, [saveProgress]);

  useEffect(() => {
    if (!notice) return;
    const timeout = window.setTimeout(() => setNotice(null), 2200);
    return () => window.clearTimeout(timeout);
  }, [notice]);

  const toggleFullscreen = useCallback(async () => {
    const next = !fullscreen;
    try {
      setFullscreen(await applyFullscreen(next, containerRef.current));
    } catch {
      // Report the real state rather than the state we hoped for.
      setFullscreen(await readFullscreen().catch(() => false));
      setNoticeTone("error");
      setNotice(next ? "Could not enter fullscreen." : "Could not exit fullscreen.");
    }
  }, [fullscreen]);

  // The window can leave fullscreen without us (green button, Esc, macOS gestures).
  useEffect(() => {
    const sync = () => {
      void readFullscreen()
        .then(setFullscreen)
        .catch(() => {});
    };
    sync();
    document.addEventListener("fullscreenchange", sync);
    window.addEventListener("resize", sync);
    return () => {
      document.removeEventListener("fullscreenchange", sync);
      window.removeEventListener("resize", sync);
    };
  }, []);

  const seekToPercent = useCallback((percent: number) => {
    const el = webtoonRef.current;
    if (!el) return;
    const max = Math.max(0, el.scrollHeight - el.clientHeight);
    el.scrollTop = (clamp(percent, 0, 100) / 100) * max;
    setScrollPct(clamp(percent, 0, 100));
    showHud();
  }, [showHud]);

  const goPage = useCallback((delta: number) => {
    const el = webtoonRef.current;
    if (!el) return;
    if (delta > 0 && el.scrollTop + el.clientHeight >= el.scrollHeight - 24) {
      setEndOfChapter(true);
      showHud();
      return;
    }
    const page = Math.max(80, el.clientHeight * 0.9);
    el.scrollTop += page * Math.sign(delta || 1);
    showHud();
  }, [showHud]);

  const jumpStrip = useCallback((edge: "home" | "end") => {
    const el = webtoonRef.current;
    if (!el) return;
    el.scrollTop = edge === "home" ? 0 : el.scrollHeight;
    showHud();
  }, [showHud]);

  useEffect(() => {
    if (!chapter) return;
    const frame = window.requestAnimationFrame(() => {
      (webtoonRef.current ?? containerRef.current)?.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [chapter?.id]);

  useEffect(() => {
    if (hudVisible) return;
    const active = document.activeElement;
    if (!(active instanceof HTMLElement)) return;
    if (!active.matches("input, textarea, select")) return;
    if (!containerRef.current?.contains(active)) return;
    active.blur();
    (webtoonRef.current ?? containerRef.current)?.focus({ preventScroll: true });
  }, [hudVisible]);

  const goChapter = async (next: boolean) => {
    if (chapter) {
      const currentGroup = chapterGroupKey(chapter);
      const currentIndex = primaryChapters.findIndex((c) => chapterGroupKey(c) === currentGroup);
      if (currentIndex >= 0) {
        const targetIndex = next ? currentIndex + 1 : currentIndex - 1;
        const target = primaryChapters[targetIndex];
        if (target) {
          navigate(`/read/${target.id}`, { replace: true });
          return;
        }
        return;
      }
    }

    const adj = await api.getAdjacentChapter(cid, next);
    if (adj) navigate(`/read/${adj.id}`, { replace: true });
  };

  const useCurrentVariantPreference = useCallback(async () => {
    if (!series || !chapter) return;
    const preference = currentChapterVariantPreference(chapter, chapters);
    if ((series.variantPreference ?? "primary") === preference) return;
    setApplyingPreference(true);
    try {
      await api.updateSeries(series.id, { variantPreference: preference });
      setSeries((prev) => (prev ? { ...prev, variantPreference: preference } : prev));
      setNoticeTone("success");
      setNotice(`Default version set to ${variantPreferenceLabel(preference)}.`);
    } catch {
      setNoticeTone("error");
      setNotice("Could not update preferred version right now.");
    } finally {
      setApplyingPreference(false);
    }
  }, [series, chapter, chapters]);

  const cycleVariantPreference = useCallback(async (direction: "forward" | "backward" = "forward") => {
    if (!series) return;
    const next = direction === "backward"
      ? previousVariantPreference(series.variantPreference ?? "primary")
      : nextVariantPreference(series.variantPreference ?? "primary");
    setApplyingPreference(true);
    try {
      await api.updateSeries(series.id, { variantPreference: next });
      setSeries((prev) => (prev ? { ...prev, variantPreference: next } : prev));
      setNoticeTone("success");
      setNotice(`Default version set to ${variantPreferenceLabel(next)}.`);
    } catch {
      setNoticeTone("error");
      setNotice("Could not update preferred version right now.");
    } finally {
      setApplyingPreference(false);
    }
  }, [series]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;
      const action = resolveReaderShortcut(e);
      if (!action) return;
      const video = Boolean(chapter && series && detectViewerKindFromItem(chapter, series) === "video");
      if (video && (action.type === "scroll" || action.type === "overlay" || action.type === "variant")) {
        return;
      }
      if (e.repeat && action.type !== "scroll") return;
      e.preventDefault();
      if (action.type === "back") {
        if (editing) {
          setEditing(null);
          return;
        }
        if (fullscreen) {
          void toggleFullscreen();
          return;
        }
        if (series?.id) navigate(`/series/${series.id}`);
        return;
      }
      if (action.type === "fullscreen") {
        void toggleFullscreen();
        return;
      }
      if (action.type === "overlay") {
        setOverlayMode((m) => (m === "off" ? "on" : m === "on" ? "both" : "off"));
        return;
      }
      if (action.type === "variant") {
        void cycleVariantPreference(action.direction);
        return;
      }
      if (action.direction === "home" || action.direction === "end") {
        jumpStrip(action.direction);
        return;
      }
      goPage(action.direction === "down" ? 1 : -1);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [
    chapter,
    cycleVariantPreference,
    editing,
    fullscreen,
    goPage,
    jumpStrip,
    navigate,
    series,
    toggleFullscreen,
  ]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      setZoom((z) => clamp(z + (e.deltaY < 0 ? 0.1 : -0.1), 1, 2.5));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  const handleWebtoonScroll = () => {
    const el = webtoonRef.current;
    if (!el || !chapter) return;
    const max = Math.max(1, el.scrollHeight - el.clientHeight);
    const percent = (el.scrollTop / max) * 100;
    setScrollOffset(el.scrollTop);
    setScrollPct(clamp(percent, 0, 100));
    showHud();
    const top = el.getBoundingClientRect().top + 8;
    const rows = el.querySelectorAll<HTMLElement>("[data-page]");
    let page = currentPage;
    rows.forEach((row) => {
      const rect = row.getBoundingClientRect();
      if (rect.top <= top && rect.bottom > top) {
        page = Number(row.dataset.page) || 0;
      }
    });
    if (page !== currentPage) setCurrentPage(page);
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 24) setEndOfChapter(true);
    else if (el.scrollTop + el.clientHeight < el.scrollHeight - 80) setEndOfChapter(false);
  };

  if (loadError) {
    return (
      <div className="reader-shell flex h-dvh items-center justify-center px-6">
        <div className="max-w-md rounded-xl border border-border bg-surface p-5 text-center">
          <p className="text-base font-semibold text-text">Chapter unavailable</p>
          <p className="mt-2 text-sm text-muted">{loadError}</p>
          <button
            onClick={() => navigate(0)}
            aria-label="Retry loading chapter"
            className="mt-4 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg hover:bg-accent-hover"
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  if (!chapter || !series) {
    return <div className="reader-shell flex h-dvh items-center justify-center text-muted">Loading chapter...</div>;
  }

  const viewerKind = detectViewerKindFromItem(chapter, series);
  const saveErrorBanner = saveError ? (
    <StatusMessage
      tone="error"
      live
      action={
        <button
          type="button"
          onClick={() => void saveProgress()}
          className="text-accent hover:text-accent-hover"
        >
          Retry
        </button>
      }
      className="fixed right-4 top-[calc(1rem+env(safe-area-inset-top))] z-40 px-3 py-2 text-xs"
    >
      {saveError}
    </StatusMessage>
  ) : null;
  const noticeBanner = notice ? (
    <StatusMessage tone={noticeTone} live className="pointer-events-none fixed right-4 top-[calc(4rem+env(safe-area-inset-top))] z-40 px-3 py-2 text-xs">
      {notice}
    </StatusMessage>
  ) : null;

  if (viewerKind === "video") {
    return (
      <>
        {saveErrorBanner}
        {noticeBanner}
        <VideoViewer
          chapter={chapter}
          series={series}
          onBack={() => navigate(`/series/${series.id}`)}
          fullscreen={fullscreen}
          toggleFullscreen={toggleFullscreen}
          containerRef={containerRef}
          hudVisible={hudVisible}
          showHud={showHud}
        />
      </>
    );
  }

  return (
    <>
      {saveErrorBanner}
      {noticeBanner}
      <MangaViewer
        chapter={chapter}
        series={series}
        chapters={chapters}
        currentPage={currentPage}
        hudVisible={hudVisible}
        setHudVisible={setHudVisible}
        fullscreen={fullscreen}
        toggleFullscreen={toggleFullscreen}
        zoom={zoom}
        setZoom={setZoom}
        overlayMode={overlayMode}
        setOverlayMode={setOverlayMode}
        regions={regions}
        setRegions={setRegions}
        editing={editing}
        setEditing={setEditing}
        translating={translating}
        translationError={translationError}
        retryTranslation={() => setTranslationRetry((value) => value + 1)}
        onTranslationError={setTranslationError}
        endOfChapter={endOfChapter}
        setEndOfChapter={setEndOfChapter}
        containerRef={containerRef}
        webtoonRef={webtoonRef}
        goChapter={goChapter}
        navigate={navigate}
        showHud={showHud}
        handleWebtoonScroll={handleWebtoonScroll}
        progressPct={scrollPct}
        pageWidth={pageWidth}
        tiles={tiles}
        onSeek={seekToPercent}
        onPageReady={tryRestoreScroll}
        onUseCurrentVariantPreference={useCurrentVariantPreference}
        applyingPreference={applyingPreference}
        currentVariantPreference={activeChapterPreference}
      />
    </>
  );
}
