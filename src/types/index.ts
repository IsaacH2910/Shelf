export type ReadingMode = "auto" | "webtoon" | "paged_ltr" | "paged_rtl";
export type VariantPreference = "primary" | "uncensored" | "revised" | "bonus" | "alternate";
export type PageDirection = "ltr" | "rtl";
export type OverlayMode = "off" | "on" | "both";
export type FitMode = "width" | "height";
export type ViewerKind = "manga" | "document" | "video";
export type ContentType = "manga" | "manhwa" | "comic" | "book" | "document" | "video";
export type SourceFormat = "pdf" | "mp4";

export interface MediaKindResult {
  contentType: ContentType;
  sourceFormat: SourceFormat;
  isSupported: boolean;
}

export interface LibraryItem {
  id: number;
  title: string;
  contentType: ContentType;
  sourceFormat: SourceFormat;
  path: string;
  progressPercent: number;
  coverUrl?: string | null;
}

export interface LibraryRoot {
  id: number;
  path: string;
  addedAt: string;
}

export interface Series {
  id: number;
  rootId: number;
  folderPath: string;
  title: string;
  coverPath?: string;
  contentType?: ContentType;
  sourceFormat?: SourceFormat;
  readingMode: ReadingMode;
  variantPreference?: VariantPreference;
  pageDirection: PageDirection;
  favorite: boolean;
  description?: string;
  chapterCount: number;
  unreadCount: number;
  progressPercent: number;
  lastReadAt?: string;
  createdAt: string;
  updatedAt: string;
  groupName?: string | null;
  tags: string[];
}

export interface Chapter {
  id: number;
  seriesId: number;
  filePath: string;
  title: string;
  chapterNumber?: number;
  volumeNumber?: number;
  sortKey: number;
  pageCount: number;
  progressPercent: number;
  lastPageIndex: number;
  createdAt: string;
  updatedAt: string;
  missing: boolean;
}

export interface ReadingProgress {
  chapterId: number;
  pageIndex: number;
  scrollOffset: number;
  positionSeconds?: number;
  durationSeconds?: number;
  percent: number;
  updatedAt: string;
  deviceId?: string;
}

export interface ContinueReadingItem {
  series: Series;
  chapter: Chapter;
  progress: ReadingProgress;
}

export interface PageImage {
  dataUrl: string;
  width: number;
  height: number;
  pageIndex: number;
  fromCache: boolean;
}

/**
 * One stacked segment of a page. A webtoon PDF page can be tens of thousands of pixels
 * tall, which no browser will display as a single image, so a chapter is read as a flat
 * list of these stacked flush against each other.
 */
export interface PageTile {
  pageIndex: number;
  tileIndex: number;
  width: number;
  height: number;
}

export interface PageSegments {
  pageIndex: number;
  tiles: PageTile[];
}

/**
 * Groups a chapter's flat segment manifest into per-page runs, preserving order.
 *
 * The reader wraps each run in one container so translation regions, whose coordinates
 * are relative to the whole page, can be positioned across all of that page's segments.
 */
export function groupTilesByPage(tiles: PageTile[]): PageSegments[] {
  const pages: PageSegments[] = [];
  for (const tile of tiles) {
    const current = pages[pages.length - 1];
    if (current && current.pageIndex === tile.pageIndex) {
      current.tiles.push(tile);
    } else {
      pages.push({ pageIndex: tile.pageIndex, tiles: [tile] });
    }
  }
  return pages;
}

export interface SeriesUpdate {
  title?: string;
  description?: string;
  readingMode?: ReadingMode;
  variantPreference?: VariantPreference;
  pageDirection?: PageDirection;
  favorite?: boolean;
}

export interface OcrRegion {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  text: string;
  translatedText?: string;
  vertical: boolean;
  hidden?: boolean;
}

export type TunnelMode = "named";

export interface RemoteSettings {
  enabled: boolean;
  mode: TunnelMode;
  publicUrl?: string;
  configuredHostname?: string;
  hasToken: boolean;
  cloudflaredInstalled: boolean;
  running: boolean;
  status: string;
  qrDataUrl?: string;
  restartCount: number;
}

export type NearbyDuration = "until_off" | "15m" | "60m";

export interface LanSettings {
  enabled: boolean;
  duration: NearbyDuration;
  expiresAt?: string;
  localUrl?: string;
  hostname?: string;
  ip?: string;
  port: number;
  advertised: boolean;
  qrDataUrl?: string;
}

export interface AppSettings {
  cacheSizeMb: number;
  cacheUsedMb: number;
  defaultReadingMode: ReadingMode;
  remote: RemoteSettings;
  lan: LanSettings;
  ocrEngineId?: string;
  translatorId?: string;
  remoteLoginReady: boolean;
}

export type UserRole = "owner" | "member";

export interface SessionInfo {
  userId: number;
  username: string;
  displayName: string;
  role: UserRole;
  accessAll: boolean;
  expiresAt: string;
}

export interface HouseholdUser {
  id: number;
  username: string;
  displayName: string;
  role: UserRole;
  accessAll: boolean;
  disabled: boolean;
  hasPassword: boolean;
  createdAt: string;
  lockedUntil?: string;
}

export interface ActiveSession {
  id: number;
  userId: number;
  username: string;
  deviceLabel: string;
  createdAt: string;
  lastUsedAt: string;
  idleExpiresAt: string;
}

export interface EngineInfo {
  id: string;
  name: string;
  available: boolean;
}

export interface IndexStatus {
  indexing: boolean;
  pendingJobs: number;
  processedJobs: number;
  failedJobs: number;
  seriesCount: number;
  chapterCount: number;
}

export interface Collection {
  id: number;
  name: string;
  createdAt: string;
  seriesCount: number;
}

export interface Tag {
  id: number;
  name: string;
}

export interface PairedDevice {
  id: number;
  name: string;
  createdAt: string;
  lastSeen?: string;
  revoked: boolean;
}

export interface SeriesQuery {
  search?: string;
  favoritesOnly?: boolean;
  sort?: string;
  unreadOnly?: boolean;
  collectionId?: number;
  readingMode?: string;
  contentType?: "all" | "manga" | "document" | "video";
}

export function detectMediaKind(path: string): MediaKindResult {
  const lower = path.toLowerCase();

  if (lower.endsWith(".pdf")) {
    const fileName = path.split(/[\\/]/).pop() ?? path;
    const label = fileName.toLowerCase();
    const mangaHints =
      /(^|[\/\-_ ])(ch|chapter|chap|vol|volume|episode|ep)[\-_ ]?\d|\b[0-9]{1,3}\b|第\s*\d|[話话回]/.test(label);
    return {
      contentType: mangaHints ? "manga" : "document",
      sourceFormat: "pdf",
      isSupported: true,
    };
  }

  if (lower.endsWith(".mp4")) {
    return {
      contentType: "video",
      sourceFormat: "mp4",
      isSupported: true,
    };
  }

  return {
    contentType: "document",
    sourceFormat: "pdf",
    isSupported: false,
  };
}

export function isSupportedLibraryFile(path: string): boolean {
  return detectMediaKind(path).isSupported;
}

export function detectMediaKindFromSeries(series: Pick<Series, "folderPath" | "title"> & Partial<Pick<Series, "contentType" | "sourceFormat">>): MediaKindResult {
  if (series.sourceFormat === "mp4" || series.contentType === "video") {
    return {
      contentType: series.contentType && series.contentType !== "video" ? series.contentType : "video",
      sourceFormat: "mp4",
      isSupported: true,
    };
  }
  if (series.contentType && series.sourceFormat) {
    return {
      contentType: series.contentType,
      sourceFormat: series.sourceFormat,
      isSupported: true,
    };
  }
  const source = `${series.folderPath}/${series.title}`.toLowerCase();
  if (source.includes("video") || source.includes("movies") || source.includes("anime") || source.includes("episode")) {
    return { contentType: "video", sourceFormat: "mp4", isSupported: true };
  }
  if (source.includes("book") || source.includes("document") || source.includes("notes") || source.includes("reference")) {
    return { contentType: "document", sourceFormat: "pdf", isSupported: true };
  }
  return detectMediaKind(series.folderPath || series.title);
}

export function isVideoItem(series: Pick<Series, "folderPath" | "title"> & Partial<Pick<Series, "contentType" | "sourceFormat">>, filePath?: string): boolean {
  if (filePath?.toLowerCase().endsWith(".mp4")) return true;
  return detectMediaKindFromSeries(series).sourceFormat === "mp4";
}

export function detectViewerKindFromItem(
  chapter: Pick<Chapter, "filePath">,
  series: Pick<Series, "folderPath" | "title"> & Partial<Pick<Series, "contentType" | "sourceFormat">>,
): ViewerKind {
  const path = chapter.filePath || "";
  const lower = path.toLowerCase();
  // File extension is the source of truth so a stale series type cannot send
  // a PDF into the video player or an MP4 into PDFium.
  if (lower.endsWith(".mp4")) return "video";
  if (lower.endsWith(".pdf")) {
    return "manga";
  }
  if (series.sourceFormat === "mp4" || series.contentType === "video") return "video";
  return detectViewerKind(path || series.title);
}

export function contentTypeLabel(type: ContentType): string {
  switch (type) {
    case "manga":
      return "Manga";
    case "manhwa":
      return "Manhwa";
    case "comic":
      return "Comic";
    case "book":
      return "Book";
    case "document":
      return "Document";
    case "video":
      return "Video";
  }
}

export function detectViewerKind(pathOrLabel: string): ViewerKind {
  const lower = pathOrLabel.toLowerCase();
  if (lower.endsWith(".mp4")) return "video";
  if (lower.endsWith(".pdf")) {
    return "manga";
  }
  return lower.includes("video") || lower.includes("movie") || lower.includes("episode") ? "video" : "manga";
}

export function effectiveReadingMode(series: Series): "webtoon" | "paged_ltr" | "paged_rtl" {
  if (series.readingMode === "webtoon") return "webtoon";
  if (series.readingMode === "paged_ltr") return "paged_ltr";
  if (series.readingMode === "paged_rtl") return "paged_rtl";
  if (series.pageDirection === "ltr") return "webtoon";
  return "paged_rtl";
}

export function readingModeLabel(mode: ReadingMode): string {
  switch (mode) {
    case "auto":
      return "Auto";
    case "webtoon":
      return "Webtoon";
    case "paged_ltr":
      return "Paged (LTR)";
    case "paged_rtl":
      return "Paged (RTL)";
  }
}

export function variantPreferenceLabel(preference: VariantPreference): string {
  switch (preference) {
    case "uncensored":
      return "Uncensored";
    case "revised":
      return "Revised";
    case "bonus":
      return "Bonus";
    case "alternate":
      return "Alternate";
    default:
      return "Primary";
  }
}

const variantPreferenceOrder: VariantPreference[] = ["primary", "uncensored", "revised", "bonus", "alternate"];

export function nextVariantPreference(preference: VariantPreference): VariantPreference {
  const index = variantPreferenceOrder.indexOf(preference);
  if (index < 0 || index === variantPreferenceOrder.length - 1) {
    return variantPreferenceOrder[0]!;
  }
  return variantPreferenceOrder[index + 1]!;
}

export function previousVariantPreference(preference: VariantPreference): VariantPreference {
  const index = variantPreferenceOrder.indexOf(preference);
  if (index <= 0) {
    return variantPreferenceOrder[variantPreferenceOrder.length - 1]!;
  }
  return variantPreferenceOrder[index - 1]!;
}

export function chapterGroupKey(chapter: Pick<Chapter, "chapterNumber" | "volumeNumber" | "id">): string {
  if (typeof chapter.chapterNumber === "number") {
    return `ch:${chapter.chapterNumber.toFixed(3)}`;
  }
  if (typeof chapter.volumeNumber === "number") {
    return `vol:${chapter.volumeNumber}`;
  }
  return `single:${chapter.id}`;
}

export function chapterVariantPreference(title: string): VariantPreference {
  const lower = title.toLowerCase();
  if (
    lower.includes("无修正") ||
    lower.includes("無修正") ||
    lower.includes("无码") ||
    lower.includes("無碼") ||
    lower.includes("去码") ||
    lower.includes("去碼")
  ) {
    return "uncensored";
  }
  if (
    lower.includes("修正") ||
    lower.includes("修復") ||
    lower.includes("修复") ||
    lower.includes("revised") ||
    lower.includes("fix")
  ) {
    return "revised";
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
    return "bonus";
  }
  return "alternate";
}

export function chapterVariantLabel(title: string): string {
  return variantPreferenceLabel(chapterVariantPreference(title));
}

function choosePreferredVariant(group: Chapter[], preference: VariantPreference): Chapter {
  const primary = group[0]!;
  if (preference === "primary") {
    return primary;
  }
  const variants = group.slice(1);
  if (!variants.length) {
    return primary;
  }
  if (preference === "alternate") {
    return variants[0]!;
  }
  const match = variants.find((chapter) => chapterVariantPreference(chapter.title) === preference);
  return match ?? primary;
}

export function preferredChapterSequence(chapters: Chapter[], preference: VariantPreference): Chapter[] {
  const visible = chapters.filter((c) => !c.missing);
  const byKey = new Map<string, Chapter[]>();
  visible.forEach((chapter) => {
    const key = chapterGroupKey(chapter);
    const existing = byKey.get(key);
    if (existing) existing.push(chapter);
    else byKey.set(key, [chapter]);
  });

  return Array.from(byKey.values())
    .map((group) => [...group].sort((a, b) => a.sortKey - b.sortKey || a.title.localeCompare(b.title)))
    .sort((a, b) => a[0]!.sortKey - b[0]!.sortKey || a[0]!.title.localeCompare(b[0]!.title))
    .map((group) => choosePreferredVariant(group, preference));
}

export function currentChapterVariantPreference(chapter: Chapter, chapters: Chapter[]): VariantPreference {
  const key = chapterGroupKey(chapter);
  const group = chapters
    .filter((c) => !c.missing && chapterGroupKey(c) === key)
    .sort((a, b) => a.sortKey - b.sortKey || a.title.localeCompare(b.title));
  if (!group.length || group[0]!.id === chapter.id) {
    return "primary";
  }
  return chapterVariantPreference(chapter.title);
}

export function untranslatedCjkNotice(regions: OcrRegion[]): boolean {
  return regions.some((r) => {
    // Hiragana or katakana letters. Japanese punctuation such as ・ must not count:
    // Vision used to emit it on Chinese lines and this notice then fired on manhua.
    const jpKr = /[\u3040-\u309f\u30a1-\u30fa\uac00-\ud7af]/.test(r.text);
    if (!jpKr) return false;
    return !r.translatedText || r.translatedText === r.text;
  });
}

export function pageFromScroll(scrollTop: number, heights: number[], gap = 0): number {
  let y = 0;
  let page = 0;
  for (let i = 0; i < heights.length; i++) {
    if (scrollTop + 8 < y + heights[i]) {
      page = i;
      break;
    }
    y += heights[i] + gap;
    page = i;
  }
  return page;
}
