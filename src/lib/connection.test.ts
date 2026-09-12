import assert from "node:assert/strict";
import { beforeEach, describe, it } from "node:test";
import {
  AUTH_ORIGIN_KEY,
  CLOUD_HOSTNAME_KEY,
  LAST_GOOD_LOCAL_KEY,
  PREFERRED_LOCAL_KEY,
  candidateOrigins,
  classifyOrigin,
  isLanHost,
  normalizeOrigin,
  originSwitchNeedsLogin,
  persistCurrentOrigin,
  probeableOrigins,
  rememberCloudHostname,
  rememberLocalOrigin,
} from "./connection.ts";

class LocalStorageMock {
  private store = new Map<string, string>();

  getItem(key: string) {
    return this.store.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.store.set(key, value);
  }

  removeItem(key: string) {
    this.store.delete(key);
  }

  clear() {
    this.store.clear();
  }
}

const storage = new LocalStorageMock();
Object.defineProperty(globalThis, "localStorage", {
  value: storage,
  configurable: true,
});

describe("connection helpers", () => {
  beforeEach(() => {
    storage.clear();
  });

  it("classifies lan and cloud origins", () => {
    assert.equal(classifyOrigin("http://studio.local:7834"), "local");
    assert.equal(classifyOrigin("http://192.168.1.20:7834"), "local");
    assert.equal(classifyOrigin("https://shelf.example.com"), "cloud");
    assert.equal(isLanHost("10.0.0.8"), true);
    assert.equal(isLanHost("172.16.4.2"), true);
    assert.equal(isLanHost("8.8.8.8"), false);
    assert.equal(isLanHost("isaacs-macbook.local"), true);
  });

  it("normalizes hostnames and urls", () => {
    assert.equal(normalizeOrigin("https://shelf.example.com/reader"), "https://shelf.example.com");
    assert.equal(normalizeOrigin("shelf.example.com"), "https://shelf.example.com");
    assert.equal(normalizeOrigin("http://Studio.local:7834/"), "http://studio.local:7834");
    assert.equal(normalizeOrigin(""), null);
  });

  it("probes last-good local before advertised local then cloud", () => {
    storage.setItem(LAST_GOOD_LOCAL_KEY, "http://192.168.1.20:7834");
    storage.setItem(PREFERRED_LOCAL_KEY, "http://studio.local:7834");
    storage.setItem(CLOUD_HOSTNAME_KEY, "shelf.example.com");
    assert.deepEqual(candidateOrigins("https://shelf.example.com"), [
      "http://192.168.1.20:7834",
      "http://studio.local:7834",
      "https://shelf.example.com",
    ]);
  });

  it("does not ask an https page to probe http lan origins", () => {
    rememberLocalOrigin("http://studio.local:7834");
    rememberCloudHostname("shelf.example.com");
    assert.deepEqual(probeableOrigins("https://shelf.example.com"), ["https://shelf.example.com"]);
    assert.deepEqual(probeableOrigins("http://studio.local:7834"), [
      "http://studio.local:7834",
      "https://shelf.example.com",
    ]);
  });

  it("persists the current origin into the matching slot", () => {
    persistCurrentOrigin("http://studio.local:7834");
    assert.equal(storage.getItem(PREFERRED_LOCAL_KEY), "http://studio.local:7834");
    persistCurrentOrigin("https://shelf.example.com");
    assert.equal(storage.getItem(CLOUD_HOSTNAME_KEY), "shelf.example.com");
  });

  it("detects an origin change that needs a new login", () => {
    storage.setItem(AUTH_ORIGIN_KEY, "http://studio.local:7834");
    assert.equal(originSwitchNeedsLogin("https://shelf.example.com"), true);
    assert.equal(originSwitchNeedsLogin("http://studio.local:7834"), false);
  });
});
