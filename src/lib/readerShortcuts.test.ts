import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { resolveReaderShortcut } from "./readerShortcuts.ts";

function key(
  partial: Partial<{ key: string; shiftKey: boolean; metaKey: boolean; ctrlKey: boolean; altKey: boolean }>,
) {
  return {
    key: "a",
    shiftKey: false,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    ...partial,
  };
}

describe("resolveReaderShortcut", () => {
  it("maps reader keys", () => {
    assert.deepEqual(resolveReaderShortcut(key({ key: "Escape" })), { type: "back" });
    assert.deepEqual(resolveReaderShortcut(key({ key: "f" })), { type: "fullscreen" });
    assert.deepEqual(resolveReaderShortcut(key({ key: "T" })), { type: "overlay" });
    assert.deepEqual(resolveReaderShortcut(key({ key: "v" })), {
      type: "variant",
      direction: "forward",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: "V", shiftKey: true })), {
      type: "variant",
      direction: "backward",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: "ArrowRight" })), {
      type: "scroll",
      direction: "down",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: "ArrowLeft" })), {
      type: "scroll",
      direction: "up",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: " " })), {
      type: "scroll",
      direction: "down",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: "Home" })), {
      type: "scroll",
      direction: "home",
    });
    assert.deepEqual(resolveReaderShortcut(key({ key: "End" })), {
      type: "scroll",
      direction: "end",
    });
  });

  it("ignores modified keys so system shortcuts stay intact", () => {
    assert.equal(resolveReaderShortcut(key({ key: "t", metaKey: true })), null);
    assert.equal(resolveReaderShortcut(key({ key: "f", ctrlKey: true })), null);
    assert.equal(resolveReaderShortcut(key({ key: "v", altKey: true })), null);
  });

  it("ignores unrelated keys", () => {
    assert.equal(resolveReaderShortcut(key({ key: "Enter" })), null);
    assert.equal(resolveReaderShortcut(key({ key: "g" })), null);
  });
});
