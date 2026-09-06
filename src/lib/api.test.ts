import assert from "node:assert/strict";
import { beforeEach, describe, it } from "node:test";
import { clearSession, getDeviceId, getSession, invokeErrorMessage, setSession } from "./api.ts";

class LocalStorageMock {
  private store = new Map<string, string>();

  get length() {
    return this.store.size;
  }

  clear() {
    this.store.clear();
  }

  getItem(key: string) {
    return this.store.get(key) ?? null;
  }

  key(index: number) {
    return Array.from(this.store.keys())[index] ?? null;
  }

  removeItem(key: string) {
    this.store.delete(key);
  }

  setItem(key: string, value: string) {
    this.store.set(key, value);
  }
}

const storage = new LocalStorageMock();

Object.defineProperty(globalThis, "localStorage", {
  value: storage,
  configurable: true,
});

describe("chapter media urls", () => {
  it("builds HTTP file and page urls outside Tauri", async () => {
    const { chapterFileUrl, chapterPageUrl } = await import("./api.ts");
    assert.equal(chapterFileUrl(12), "/api/chapters/12/file");
    assert.equal(chapterPageUrl(12, 2, 1400), "/api/chapters/12/page/2?width=1400");
    assert.equal(chapterPageUrl(12, 2, 1400, 3), "/api/chapters/12/page/2?width=1400&r=3");
  });

  it("builds HTTP segment urls and clamps the render width", async () => {
    const { chapterTileUrl } = await import("./api.ts");
    assert.equal(chapterTileUrl(12, 1, 7, 1200), "/api/chapters/12/tile/1/7?width=1200");
    assert.equal(chapterTileUrl(12, 0, 0, 40), "/api/chapters/12/tile/0/0?width=100");
    assert.equal(chapterTileUrl(12, 0, 0, 99999), "/api/chapters/12/tile/0/0?width=2560");
  });

  it("converts data urls into blob object urls", async () => {
    const { objectUrlFromPageData } = await import("./api.ts");
    const url = objectUrlFromPageData(
      "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
    );
    assert.match(url, /^blob:/);
  });
});

describe("session storage migration", () => {
  beforeEach(() => {
    storage.clear();
  });

  it("reads legacy session keys and backfills the new keys", () => {
    storage.setItem("preview_session", "legacy-session");
    storage.setItem("preview_device", "legacy-device");

    assert.equal(getSession(), "legacy-session");
    assert.equal(storage.getItem("shelf_session"), "legacy-session");
    assert.equal(getDeviceId(), "legacy-device");
    assert.equal(storage.getItem("shelf_device"), "legacy-device");
  });

  it("prefers new keys when both are present", () => {
    storage.setItem("preview_session", "legacy-session");
    storage.setItem("shelf_session", "new-session");
    storage.setItem("preview_device", "legacy-device");
    storage.setItem("shelf_device", "new-device");

    assert.equal(getSession(), "new-session");
    assert.equal(getDeviceId(), "new-device");
  });

  it("writes and clears both key sets", () => {
    setSession("session-token", "device-token");

    assert.equal(storage.getItem("shelf_session"), "session-token");
    assert.equal(storage.getItem("preview_session"), "session-token");
    assert.equal(storage.getItem("shelf_device"), "device-token");
    assert.equal(storage.getItem("preview_device"), "device-token");

    clearSession();

    assert.equal(storage.getItem("shelf_session"), null);
    assert.equal(storage.getItem("preview_session"), null);
    assert.equal(storage.getItem("shelf_device"), null);
    assert.equal(storage.getItem("preview_device"), null);
  });
});

describe("invokeErrorMessage", () => {
  it("reads Tauri string and object payloads", () => {
    assert.equal(invokeErrorMessage("port in use", "fallback"), "port in use");
    assert.equal(invokeErrorMessage(new Error("Set an owner password"), "fallback"), "Set an owner password");
    assert.equal(invokeErrorMessage({ message: "cloudflared is not installed" }, "fallback"), "cloudflared is not installed");
    assert.equal(invokeErrorMessage({ error: "token missing" }, "fallback"), "token missing");
    assert.equal(invokeErrorMessage(null, "Could not update cloud access."), "Could not update cloud access.");
  });
});