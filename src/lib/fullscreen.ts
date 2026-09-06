import { isTauri } from "./api";

/**
 * The app's WKWebView does not grant element fullscreen, so `requestFullscreen()` on a
 * DOM node silently rejects inside the Mac app. The desktop build drives the native
 * window instead; only browser-based thin clients use the DOM Fullscreen API.
 */
export async function applyFullscreen(next: boolean, element: HTMLElement | null): Promise<boolean> {
  if (isTauri()) {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    await win.setFullscreen(next);
    return await win.isFullscreen();
  }

  if (next) {
    if (!element) return false;
    await element.requestFullscreen();
  } else if (document.fullscreenElement) {
    await document.exitFullscreen();
  }
  return Boolean(document.fullscreenElement);
}

export async function readFullscreen(): Promise<boolean> {
  if (isTauri()) {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return await getCurrentWindow().isFullscreen();
  }
  return Boolean(document.fullscreenElement);
}
