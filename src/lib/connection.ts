export const PREFERRED_LOCAL_KEY = "shelf_preferred_local_base";
export const CLOUD_HOSTNAME_KEY = "shelf_cloud_hostname";
export const LAST_GOOD_LOCAL_KEY = "shelf_last_good_local";
export const AUTH_ORIGIN_KEY = "shelf_auth_origin";

/** PWA / non-Apple clients: LAN then Cloudflare. The iOS shell tries Bonjour first. */
export type ConnectionKind = "local" | "cloud" | "offline";

export interface ConnectionState {
  kind: ConnectionKind;
  origin: string;
  switched: boolean;
}

const HEALTH_TIMEOUT_MS = 1600;

function storageGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storageSet(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore quota / private mode */
  }
}

export function normalizeOrigin(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  try {
    const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
    const url = new URL(withScheme);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    return url.origin;
  } catch {
    return null;
  }
}

export function isLoopbackHost(host: string): boolean {
  return host === "localhost" || host === "127.0.0.1" || host === "::1" || host === "[::1]";
}

export function isLanHost(host: string): boolean {
  const name = host.trim().toLowerCase().replace(/\.$/, "");
  if (!name) return false;
  if (isLoopbackHost(name)) return true;
  if (name.endsWith(".local")) return true;
  const ipv4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(name);
  if (!ipv4) return false;
  const octets = ipv4.slice(1).map((part) => Number(part));
  if (octets.some((n) => n > 255)) return false;
  const [a, b] = octets;
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
}

export function classifyOrigin(origin: string): Exclude<ConnectionKind, "offline"> {
  try {
    const url = new URL(origin);
    if (url.protocol === "http:" && isLanHost(url.hostname)) return "local";
    return "cloud";
  } catch {
    return "cloud";
  }
}

export function persistCurrentOrigin(origin = typeof window === "undefined" ? "" : window.location.origin) {
  const normalized = normalizeOrigin(origin);
  if (!normalized) return;
  if (classifyOrigin(normalized) === "local") {
    storageSet(PREFERRED_LOCAL_KEY, normalized);
    storageSet(LAST_GOOD_LOCAL_KEY, normalized);
  } else {
    try {
      storageSet(CLOUD_HOSTNAME_KEY, new URL(normalized).hostname);
    } catch {
      storageSet(CLOUD_HOSTNAME_KEY, normalized);
    }
  }
}

export function rememberCloudHostname(hostname: string) {
  const origin = normalizeOrigin(hostname);
  if (!origin) return;
  try {
    storageSet(CLOUD_HOSTNAME_KEY, new URL(origin).hostname);
  } catch {
    storageSet(CLOUD_HOSTNAME_KEY, hostname.trim());
  }
}

export function rememberLocalOrigin(url: string) {
  const origin = normalizeOrigin(url);
  if (!origin) return;
  storageSet(PREFERRED_LOCAL_KEY, origin);
}

export function cloudOriginFromStorage(): string | null {
  const stored = storageGet(CLOUD_HOSTNAME_KEY);
  if (!stored) return null;
  return normalizeOrigin(stored.startsWith("http") ? stored : `https://${stored}`);
}

export function localCandidates(): string[] {
  const values = [storageGet(LAST_GOOD_LOCAL_KEY), storageGet(PREFERRED_LOCAL_KEY)];
  const out: string[] = [];
  for (const value of values) {
    const origin = value ? normalizeOrigin(value) : null;
    if (origin && !out.includes(origin)) out.push(origin);
  }
  return out;
}

export function candidateOrigins(currentOrigin: string): string[] {
  const current = normalizeOrigin(currentOrigin);
  const locals = localCandidates();
  const cloud = cloudOriginFromStorage();
  const ordered: string[] = [];
  const push = (value: string | null) => {
    if (!value || ordered.includes(value)) return;
    ordered.push(value);
  };
  for (const local of locals) push(local);
  if (current && classifyOrigin(current) === "local") push(current);
  if (current && classifyOrigin(current) === "cloud") push(current);
  push(cloud);
  return ordered;
}

export async function probeHealth(baseUrl: string, timeoutMs = HEALTH_TIMEOUT_MS): Promise<boolean> {
  const origin = normalizeOrigin(baseUrl);
  if (!origin) return false;
  const controller = new AbortController();
  const timer = globalThis.setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${origin}/api/health`, {
      method: "GET",
      credentials: "omit",
      cache: "no-store",
      signal: controller.signal,
    });
    if (!response.ok) return false;
    const body = (await response.json().catch(() => null)) as { status?: string; localUrl?: string } | null;
    if (body?.localUrl) rememberLocalOrigin(body.localUrl);
    return body?.status === "ok" || body == null;
  } catch {
    return false;
  } finally {
    globalThis.clearTimeout(timer);
  }
}

/**
 * Non-Apple clients cannot browse Bonjour. They try remembered LAN URLs, then Cloudflare.
 * Safari/PWA pages on HTTPS cannot probe http:// LAN origins (mixed content).
 * From a LAN HTTP page we can probe Cloudflare HTTPS and fall back.
 */
export function probeableOrigins(currentOrigin: string): string[] {
  const current = normalizeOrigin(currentOrigin) ?? currentOrigin;
  const httpsPage = current.startsWith("https:");
  return candidateOrigins(current).filter((origin) => {
    if (origin === current) return true;
    if (httpsPage && origin.startsWith("http:")) return false;
    return true;
  });
}

export async function resolveActiveOrigin(currentOrigin: string): Promise<ConnectionState> {
  const current = normalizeOrigin(currentOrigin) ?? currentOrigin;
  const candidates = probeableOrigins(current);
  for (const origin of candidates) {
    if (await probeHealth(origin)) {
      persistCurrentOrigin(origin);
      return {
        kind: classifyOrigin(origin),
        origin,
        switched: origin !== current,
      };
    }
  }
  return { kind: "offline", origin: current, switched: false };
}

export function navigateToOrigin(origin: string) {
  if (typeof window === "undefined") return;
  const next = normalizeOrigin(origin);
  if (!next || next === window.location.origin) return;
  const hash = window.location.hash || "";
  window.location.replace(`${next}/${hash}`);
}

export function originSwitchNeedsLogin(currentOrigin: string): boolean {
  const previous = storageGet(AUTH_ORIGIN_KEY);
  return Boolean(previous && previous !== currentOrigin);
}

export function markAuthOrigin(origin: string) {
  storageSet(AUTH_ORIGIN_KEY, origin);
}
