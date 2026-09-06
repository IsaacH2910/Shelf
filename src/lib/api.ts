import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type {
  ActiveSession,
  AppSettings,
  Chapter,
  Collection,
  ContinueReadingItem,
  EngineInfo,
  HouseholdUser,
  IndexStatus,
  LibraryRoot,
  OcrRegion,
  PageImage,
  PageTile,
  ReadingProgress,
  RemoteSettings,
  Series,
  SeriesQuery,
  SeriesUpdate,
  SessionInfo,
  Tag,
} from "../types";

const SESSION_KEY = "shelf_session";
const LEGACY_SESSION_KEY = "preview_session";
const DEVICE_KEY = "shelf_device";
const LEGACY_DEVICE_KEY = "preview_device";
const DEFAULT_GET_TIMEOUT_MS = 15000;
const DEFAULT_GET_RETRIES = 1;

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isThinReader(): boolean {
  return !isTauri();
}

export function macAppOnlyMessage(feature: string): string {
  return `${feature} is available on the Mac app.`;
}

export function revokeObjectUrl(url: string | null | undefined) {
  if (!url || !url.startsWith("blob:")) return;
  URL.revokeObjectURL(url);
}

export function isTimeoutError(error: unknown): boolean {
  return invokeErrorMessage(error, "") === "Request timed out";
}

export function invokeErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object") {
    const rec = error as { message?: unknown; error?: unknown };
    if (typeof rec.message === "string" && rec.message.trim()) return rec.message;
    if (typeof rec.error === "string" && rec.error.trim()) return rec.error;
  }
  return fallback;
}

export function getSession(): string | null {
  try {
    const token = localStorage.getItem(SESSION_KEY) ?? localStorage.getItem(LEGACY_SESSION_KEY);
    if (token && !localStorage.getItem(SESSION_KEY)) {
      localStorage.setItem(SESSION_KEY, token);
    }
    return token;
  } catch {
    return null;
  }
}

export function setSession(token: string, deviceId: string) {
  localStorage.setItem(SESSION_KEY, token);
  localStorage.setItem(LEGACY_SESSION_KEY, token);
  localStorage.setItem(DEVICE_KEY, deviceId);
  localStorage.setItem(LEGACY_DEVICE_KEY, deviceId);
}

export function clearSession() {
  localStorage.removeItem(SESSION_KEY);
  localStorage.removeItem(LEGACY_SESSION_KEY);
  localStorage.removeItem(DEVICE_KEY);
  localStorage.removeItem(LEGACY_DEVICE_KEY);
}

export function getDeviceId(): string {
  try {
    const deviceId = localStorage.getItem(DEVICE_KEY) ?? localStorage.getItem(LEGACY_DEVICE_KEY);
    if (deviceId && !localStorage.getItem(DEVICE_KEY)) {
      localStorage.setItem(DEVICE_KEY, deviceId);
    }
    return deviceId ?? "desktop";
  } catch {
    return "desktop";
  }
}

class AuthError extends Error {
  constructor() {
    super("Unauthorized");
    this.name = "AuthError";
  }
}

export class CapabilityError extends Error {
  constructor(feature: string) {
    super(macAppOnlyMessage(feature));
    this.name = "CapabilityError";
  }
}

function requireMacApp(feature: string): never {
  throw new CapabilityError(feature);
}

function buildSessionHeaders(init: RequestInit = {}) {
  const headers = new Headers(init.headers);
  const token = getSession();
  if (token) headers.set("Authorization", `Bearer ${token}`);
  if (
    init.body &&
    !headers.has("Content-Type") &&
    !(init.body instanceof FormData) &&
    !(init.body instanceof Blob) &&
    !(init.body instanceof ArrayBuffer)
  ) {
    headers.set("Content-Type", "application/json");
  }
  return headers;
}

function notifyUnauthorized() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event("shelf:unauthorized"));
}

async function fetchWithPolicy(
  path: string,
  init: RequestInit = {},
  options: { timeoutMs?: number; retries?: number } = {},
): Promise<Response> {
  const timeoutMs = options.timeoutMs ?? 0;
  const retries = options.retries ?? 0;

  for (let attempt = 0; attempt <= retries; attempt += 1) {
    const controller = new AbortController();
    const timeoutId = timeoutMs
      ? globalThis.setTimeout(() => controller.abort(), timeoutMs)
      : null;

    try {
      const response = await fetch(path, {
        ...init,
        credentials: "include",
        headers: buildSessionHeaders(init),
        signal: controller.signal,
      });
      if (response.status === 401 && !path.includes("/api/auth/login")) {
        clearSession();
        notifyUnauthorized();
      }
      return response;
    } catch (error) {
      const retryable = error instanceof TypeError || (error instanceof Error && error.name === "AbortError");
      if (!retryable || attempt === retries) {
        if (error instanceof Error && error.name === "AbortError") {
          throw new Error("Request timed out");
        }
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 300 * (attempt + 1)));
    } finally {
      if (timeoutId != null) globalThis.clearTimeout(timeoutId);
    }
  }

  throw new Error("Request failed");
}

async function http<T>(
  path: string,
  init: RequestInit = {},
  options?: { timeoutMs?: number; retries?: number },
): Promise<T> {
  const res = await fetchWithPolicy(path, init, options);
  if (res.status === 401) throw new AuthError();
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || res.statusText);
  }
  if (res.status === 204) return undefined as T;
  const ct = res.headers.get("content-type") ?? "";
  if (ct.includes("application/json") || path.includes("/api/")) {
    return res.json() as Promise<T>;
  }
  return undefined as T;
}

function httpGet<T>(path: string, options?: { timeoutMs?: number; retries?: number }) {
  return http<T>(
    path,
    { method: "GET" },
    {
      timeoutMs: options?.timeoutMs ?? DEFAULT_GET_TIMEOUT_MS,
      retries: options?.retries ?? DEFAULT_GET_RETRIES,
    },
  );
}

async function getBlobObjectUrl(
  path: string,
  options?: { timeoutMs?: number; retries?: number },
): Promise<{ url: string | null; response: Response }> {
  const response = await fetchWithPolicy(
    path,
    { method: "GET" },
    {
      timeoutMs: options?.timeoutMs ?? DEFAULT_GET_TIMEOUT_MS,
      retries: options?.retries ?? DEFAULT_GET_RETRIES,
    },
  );
  if (response.status === 401) throw new AuthError();
  if (!response.ok) {
    return { url: null, response };
  }
  const blob = await response.blob();
  return { url: URL.createObjectURL(blob), response };
}

async function pickFolderTauri(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({ directory: true, multiple: false });
  if (typeof result === "string") return result;
  return null;
}

export async function pickFolder(): Promise<string | null> {
  if (isTauri()) return pickFolderTauri();
  return null;
}

export function chapterFileUrl(chapterId: number): string {
  if (isTauri()) {
    return convertFileSrc(String(chapterId), "shelf-media");
  }
  return `/api/chapters/${chapterId}/file`;
}

export function chapterPageUrl(chapterId: number, pageIndex: number, width: number, cacheBust = 0): string {
  const w = Math.min(2560, Math.max(100, Math.round(width)));
  if (isTauri()) {
    const key = cacheBust > 0
      ? `page-${chapterId}-${pageIndex}-${w}-${cacheBust}`
      : `page-${chapterId}-${pageIndex}-${w}`;
    return convertFileSrc(key, "shelf-media");
  }
  const bust = cacheBust > 0 ? `&r=${cacheBust}` : "";
  return `/api/chapters/${chapterId}/page/${pageIndex}?width=${w}${bust}`;
}

/** WKWebView often refuses giant data: URLs and custom-protocol <img> src. Blob URLs work. */
export function objectUrlFromPageData(dataUrl: string): string {
  if (dataUrl.startsWith("blob:") || dataUrl.startsWith("http:") || dataUrl.startsWith("https:")) {
    return dataUrl;
  }
  if (!dataUrl.startsWith("data:")) return dataUrl;
  const comma = dataUrl.indexOf(",");
  if (comma < 0) return dataUrl;
  const header = dataUrl.slice(0, comma);
  const payload = dataUrl.slice(comma + 1);
  const mime = /data:([^;,]+)/.exec(header)?.[1] ?? "image/webp";
  const binary = atob(payload);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type: mime }));
}

export function chapterTileUrl(
  chapterId: number,
  pageIndex: number,
  tileIndex: number,
  width: number,
): string {
  const w = Math.min(2560, Math.max(100, Math.round(width)));
  if (isTauri()) {
    return convertFileSrc(`tile-${chapterId}-${pageIndex}-${tileIndex}-${w}`, "shelf-media");
  }
  return `/api/chapters/${chapterId}/tile/${pageIndex}/${tileIndex}?width=${w}`;
}

export async function loadTileObjectUrl(
  chapterId: number,
  pageIndex: number,
  tileIndex: number,
  width: number,
): Promise<string> {
  try {
    const tile = await api.getChapterTile(chapterId, pageIndex, tileIndex, width);
    return objectUrlFromPageData(tile.dataUrl);
  } catch (error) {
    if (!isTauri()) throw error;
    const response = await fetch(chapterTileUrl(chapterId, pageIndex, tileIndex, width));
    if (!response.ok) {
      const detail = (await response.text()).trim();
      throw new Error(detail || (error instanceof Error ? error.message : "Could not render this section."));
    }
    return URL.createObjectURL(await response.blob());
  }
}

export async function loadPageObjectUrl(chapterId: number, pageIndex: number, width: number): Promise<string> {
  try {
    const page = await api.getPageImage(chapterId, pageIndex, width);
    return objectUrlFromPageData(page.dataUrl);
  } catch (error) {
    if (!isTauri()) throw error;
    try {
      const response = await fetch(chapterPageUrl(chapterId, pageIndex, width));
      if (!response.ok) {
        const detail = (await response.text()).trim();
        throw new Error(detail || `Could not render this page (${response.status}).`);
      }
      return URL.createObjectURL(await response.blob());
    } catch (fallbackError) {
      if (error instanceof Error && error.message && error.message !== "prefetch") {
        throw error;
      }
      throw fallbackError instanceof Error ? fallbackError : new Error("Could not render this page.");
    }
  }
}

export async function login(username: string, password: string) {
  const deviceName = navigator.userAgent.includes("iPad")
    ? "iPad"
    : navigator.userAgent.includes("iPhone")
      ? "iPhone"
      : "Browser";
  const res = await fetch("/api/auth/login", {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password, deviceName }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Could not sign in" }));
    throw new Error(body.error || "Could not sign in");
  }
  const body = (await res.json()) as SessionInfo & { token?: string };
  if (body.token) {
    setSession(body.token, getDeviceId());
  } else {
    clearSession();
  }
  return body;
}

export async function fetchSessionInfo(): Promise<SessionInfo> {
  return httpGet<SessionInfo>("/api/auth/session");
}

export async function logoutRemote() {
  try {
    await http<void>("/api/auth/logout", { method: "POST" });
  } catch {
    /* still clear locally */
  }
  clearSession();
}

export async function createLibraryFolder(rootId: number, name: string) {
  const res = await fetchWithPolicy("/api/library/folders", {
    method: "POST",
    body: JSON.stringify({ rootId, name }),
  });
  if (res.status === 401) throw new AuthError();
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Could not create folder" }));
    throw new Error(body.error || "Could not create folder");
  }
}

export async function uploadChapterFile(
  seriesId: number,
  file: File,
  onProgress?: (offset: number, size: number) => void,
) {
  const started = await http<{ uploadId: string; offset: number; size: number }>(
    `/api/library/series/${seriesId}/files`,
    {
      method: "POST",
      body: JSON.stringify({ seriesId, fileName: file.name, size: file.size }),
    },
  );
  const chunkSize = 4 * 1024 * 1024;
  let offset = started.offset;
  while (offset < file.size) {
    const end = Math.min(file.size, offset + chunkSize);
    const chunk = file.slice(offset, end);
    const status = await http<{ uploadId: string; offset: number; size: number }>(
      `/api/uploads/${started.uploadId}`,
      {
        method: "PUT",
        headers: {
          "Content-Range": `bytes ${offset}-${end - 1}/${file.size}`,
          "Content-Type": "application/octet-stream",
        },
        body: chunk,
      },
      { timeoutMs: 120000, retries: 2 },
    );
    offset = status.offset;
    onProgress?.(offset, file.size);
  }
  const res = await fetchWithPolicy(`/api/uploads/${started.uploadId}/complete`, { method: "POST" }, {
    timeoutMs: 60000,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Could not finish upload" }));
    throw new Error(body.error || "Could not finish upload");
  }
}

export function chapterDownloadUrl(chapterId: number): string {
  return `/api/chapters/${chapterId}/file?download=1`;
}

function tauriApi() {
  return {
    addLibraryRoot: (path: string) => invoke<LibraryRoot>("add_library_root", { path }),
    listLibraryRoots: () => invoke<LibraryRoot[]>("list_library_roots"),
    removeLibraryRoot: (id: number) => invoke<void>("remove_library_root", { id }),
    listSeries: (q: SeriesQuery = {}) =>
      invoke<Series[]>("list_series", {
        search: q.search,
        favoritesOnly: q.favoritesOnly,
        sort: q.sort,
        unreadOnly: q.unreadOnly,
        collectionId: q.collectionId,
        readingMode: q.readingMode,
        contentType: q.contentType === "all" ? undefined : q.contentType,
      }),
    getSeries: (id: number) => invoke<Series>("get_series", { id }),
    updateSeries: (id: number, update: SeriesUpdate) => invoke<void>("update_series", { id, update }),
    listChapters: (seriesId: number) => invoke<Chapter[]>("list_chapters", { seriesId }),
    getChapter: (id: number) => invoke<Chapter>("get_chapter", { id }),
    getPageCount: (id: number) => invoke<number>("get_page_count", { id }),
    getAdjacentChapter: (chapterId: number, next: boolean) =>
      invoke<Chapter | null>("get_adjacent_chapter", { chapterId, next }),
    getPageImage: (chapterId: number, pageIndex: number, targetWidth: number) =>
      invoke<PageImage>("get_page_image", { chapterId, pageIndex, targetWidth }),
    getChapterTiles: (chapterId: number, targetWidth: number) =>
      invoke<PageTile[]>("get_chapter_tiles", { chapterId, targetWidth }),
    getChapterTile: (chapterId: number, pageIndex: number, tileIndex: number, targetWidth: number) =>
      invoke<PageImage>("get_chapter_tile", { chapterId, pageIndex, tileIndex, targetWidth }),
    prefetchPages: (chapterId: number, pages: number[], targetWidth: number) =>
      invoke<void>("prefetch_pages", { chapterId, pages, targetWidth }),
    saveProgress: (progress: ReadingProgress) => invoke<void>("save_progress", { progress }),
    getProgress: (chapterId: number) => invoke<ReadingProgress | null>("get_progress", { chapterId }),
    continueReading: () => invoke<ContinueReadingItem[]>("continue_reading"),
    readingHistory: () => invoke<ContinueReadingItem[]>("reading_history"),
    clearReadingHistory: () => invoke<void>("clear_reading_history"),
    recentlyAdded: () => invoke<Series[]>("recently_added"),
    getCoverImage: (seriesId: number) => invoke<string | null>("get_cover_image", { seriesId }),
    getIndexStatus: () => invoke<IndexStatus>("get_index_status"),
    getSettings: () => invoke<AppSettings>("get_settings"),
    listOcrEngines: () => invoke<EngineInfo[]>("list_ocr_engines"),
    listTranslators: () => invoke<EngineInfo[]>("list_translators"),
    getPageTranslation: (chapterId: number, pageIndex: number) =>
      invoke<OcrRegion[]>("get_page_translation", { chapterId, pageIndex }),
    saveTranslationEdits: (chapterId: number, pageIndex: number, regions: OcrRegion[]) =>
      invoke<void>("save_translation_edits", { chapterId, pageIndex, regions }),
    updateAppSettings: (cacheSizeMb?: number, ocrEngineId?: string, translatorId?: string) =>
      invoke<void>("update_app_settings", { cacheSizeMb, ocrEngineId, translatorId }),
    clearCache: () => invoke<void>("clear_cache"),
    rescanLibrary: () => invoke<void>("rescan_library"),
    listCollections: () => invoke<Collection[]>("list_collections"),
    createCollection: (name: string) => invoke<Collection>("create_collection", { name }),
    deleteCollection: (id: number) => invoke<void>("delete_collection", { id }),
    setCollectionItem: (collectionId: number, seriesId: number, add: boolean) =>
      invoke<void>("set_collection_item", { collectionId, seriesId, add }),
    seriesCollections: (seriesId: number) => invoke<number[]>("series_collections", { seriesId }),
    listTags: () => invoke<Tag[]>("list_tags"),
    setSeriesTags: (seriesId: number, tags: string[]) => invoke<void>("set_series_tags", { seriesId, tags }),
    setOwnerPassword: (password: string) => invoke<void>("set_owner_password", { password }),
    listUsers: () => invoke<HouseholdUser[]>("list_users"),
    createUser: (request: {
      username: string;
      displayName: string;
      password: string;
      accessAll: boolean;
      seriesIds: number[];
    }) => invoke<HouseholdUser>("create_user", { request }),
    updateUser: (
      id: number,
      request: {
        displayName?: string;
        password?: string;
        accessAll?: boolean;
        disabled?: boolean;
        seriesIds?: number[];
      },
    ) => invoke<HouseholdUser>("update_user", { id, request }),
    listUserGrants: (userId: number) => invoke<number[]>("list_user_grants", { userId }),
    listSessions: () => invoke<ActiveSession[]>("list_sessions"),
    revokeSession: (id: number) => invoke<void>("revoke_session", { id }),
    setRemoteCredentials: (hostname: string, token?: string) =>
      invoke<RemoteSettings>("set_remote_credentials", { hostname, token }),
    setRemoteConfig: (enabled: boolean) =>
      invoke<RemoteSettings>("set_remote_config", { enabled }),
  };
}

function httpApi() {
  const qs = (q: SeriesQuery) => {
    const p = new URLSearchParams();
    if (q.search) p.set("search", q.search);
    if (q.favoritesOnly) p.set("favorites", "true");
    if (q.sort) p.set("sort", q.sort);
    if (q.unreadOnly) p.set("unread", "true");
    if (q.collectionId) p.set("collection_id", String(q.collectionId));
    if (q.readingMode) p.set("reading_mode", q.readingMode);
    if (q.contentType && q.contentType !== "all") p.set("content_type", q.contentType);
    const s = p.toString();
    return s ? `?${s}` : "";
  };

  return {
    addLibraryRoot: async () => requireMacApp("Adding folders"),
    listLibraryRoots: async () => [] as LibraryRoot[],
    removeLibraryRoot: async () => requireMacApp("Removing folders"),
    listSeries: (q: SeriesQuery = {}) => httpGet<Series[]>(`/api/library/series${qs(q)}`),
    getSeries: (id: number) => httpGet<Series>(`/api/library/series/${id}`),
    updateSeries: async () => requireMacApp("Editing series details"),
    listChapters: (seriesId: number) => httpGet<Chapter[]>(`/api/library/series/${seriesId}/chapters`),
    getChapter: (id: number) => httpGet<Chapter>(`/api/chapters/${id}`),
    getPageCount: (id: number) => httpGet<number>(`/api/chapters/${id}/page-count`),
    getAdjacentChapter: (chapterId: number, next: boolean) =>
      httpGet<Chapter | null>(`/api/chapters/${chapterId}/adjacent?next=${next}`),
    getPageImage: async (chapterId: number, pageIndex: number, targetWidth: number) => {
      const { url, response } = await getBlobObjectUrl(`/api/chapters/${chapterId}/page/${pageIndex}?width=${targetWidth}`);
      if (!response.ok || !url) throw new Error("Failed to load page");
      const width = parseInt(response.headers.get("x-image-width") ?? "0", 10) || targetWidth;
      const height = parseInt(response.headers.get("x-image-height") ?? "0", 10) || Math.round(targetWidth * 1.4);
      return { dataUrl: url, width, height, pageIndex, fromCache: true } satisfies PageImage;
    },
    getChapterTiles: (chapterId: number, targetWidth: number) =>
      httpGet<PageTile[]>(`/api/chapters/${chapterId}/tiles?width=${targetWidth}`),
    getChapterTile: async (
      chapterId: number,
      pageIndex: number,
      tileIndex: number,
      targetWidth: number,
    ) => {
      const { url, response } = await getBlobObjectUrl(
        `/api/chapters/${chapterId}/tile/${pageIndex}/${tileIndex}?width=${targetWidth}`,
      );
      if (!response.ok || !url) throw new Error("Failed to load page section");
      const width = parseInt(response.headers.get("x-image-width") ?? "0", 10) || targetWidth;
      const height = parseInt(response.headers.get("x-image-height") ?? "0", 10) || targetWidth;
      return { dataUrl: url, width, height, pageIndex, fromCache: true } satisfies PageImage;
    },
    prefetchPages: async () => undefined,
    saveProgress: (progress: ReadingProgress) =>
      http<void>(`/api/chapters/${progress.chapterId}/progress`, {
        method: "POST",
        body: JSON.stringify({ ...progress, deviceId: getDeviceId() }),
      }),
    getProgress: (chapterId: number) => httpGet<ReadingProgress | null>(`/api/chapters/${chapterId}/progress`),
    continueReading: () => httpGet<ContinueReadingItem[]>("/api/library/continue"),
    readingHistory: () => httpGet<ContinueReadingItem[]>("/api/library/history"),
    clearReadingHistory: () => http<void>("/api/library/history", { method: "DELETE" }),
    recentlyAdded: () => httpGet<Series[]>("/api/library/recent"),
    getCoverImage: async (seriesId: number) => {
      const { url } = await getBlobObjectUrl(`/api/covers/${seriesId}`);
      return url;
    },
    getIndexStatus: async () =>
      ({ indexing: false, pendingJobs: 0, processedJobs: 0, failedJobs: 0, seriesCount: 0, chapterCount: 0 }) as IndexStatus,
    getSettings: async () =>
      ({
        cacheSizeMb: 0,
        cacheUsedMb: 0,
        defaultReadingMode: "auto",
        remote: {
          enabled: true,
          mode: "named",
          running: true,
          status: "remote",
          restartCount: 0,
          hasToken: true,
          cloudflaredInstalled: true,
        },
        remoteLoginReady: true,
      }) as AppSettings,
    listOcrEngines: async () => [] as EngineInfo[],
    listTranslators: async () => [] as EngineInfo[],
    getPageTranslation: (chapterId: number, pageIndex: number) =>
      httpGet<OcrRegion[]>(`/api/chapters/${chapterId}/translation/${pageIndex}`),
    saveTranslationEdits: async () => requireMacApp("Saving translation edits"),
    updateAppSettings: async () => requireMacApp("Changing settings"),
    clearCache: async () => requireMacApp("Clearing cache"),
    rescanLibrary: async () => requireMacApp("Rescanning the library"),
    listCollections: () => httpGet<Collection[]>("/api/library/collections"),
    createCollection: async () => requireMacApp("Creating collections"),
    deleteCollection: async () => requireMacApp("Deleting collections"),
    setCollectionItem: async () => requireMacApp("Editing collections"),
    seriesCollections: async () => [] as number[],
    listTags: async () => [] as Tag[],
    setSeriesTags: async () => requireMacApp("Editing tags"),
    setOwnerPassword: async () => requireMacApp("Changing the owner password"),
    listUsers: async () => [] as HouseholdUser[],
    createUser: async () => requireMacApp("Managing household accounts"),
    updateUser: async () => requireMacApp("Managing household accounts"),
    listUserGrants: async () => [] as number[],
    listSessions: async () => [] as ActiveSession[],
    revokeSession: async () => requireMacApp("Managing sessions"),
    setRemoteCredentials: async () => requireMacApp("Saving cloud credentials"),
    setRemoteConfig: async () => requireMacApp("Changing remote access"),
  };
}

function client() {
  return isTauri() ? tauriApi() : httpApi();
}

const activeClient = client();
const apiMemberCache = new Map<PropertyKey, unknown>();

export const api: ReturnType<typeof tauriApi> = new Proxy({} as ReturnType<typeof tauriApi>, {
  get(_target, prop, _receiver) {
    if (apiMemberCache.has(prop)) {
      return apiMemberCache.get(prop);
    }

    const value = activeClient[prop as keyof typeof activeClient];
    const cachedValue = typeof value === "function" ? value.bind(activeClient) : value;
    apiMemberCache.set(prop, cachedValue);
    return cachedValue;
  },
});

export { AuthError };
