import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { resolveLibraryPresence } from "./libraryState.ts";

describe("resolveLibraryPresence", () => {
  it("shows ready whenever items exist, even during a refresh", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: true,
        error: null,
        itemCount: 3,
        rootCount: 1,
        indexing: true,
      }),
      "ready",
    );
  });

  it("keeps loading until the first result arrives", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: true,
        error: null,
        itemCount: 0,
        rootCount: null,
        indexing: false,
      }),
      "loading",
    );
  });

  it("does not treat an in-progress scan as an empty library", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: false,
        error: null,
        itemCount: 0,
        rootCount: 1,
        indexing: true,
      }),
      "indexing",
    );
  });

  it("distinguishes no folders from folders that contain no media", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: false,
        error: null,
        itemCount: 0,
        rootCount: 0,
        indexing: false,
      }),
      "empty-no-folders",
    );
    assert.equal(
      resolveLibraryPresence({
        loading: false,
        error: null,
        itemCount: 0,
        rootCount: 2,
        indexing: false,
      }),
      "empty-no-files",
    );
  });

  it("keeps load failures separate from empty", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: false,
        error: "timeout",
        itemCount: 0,
        rootCount: 1,
        indexing: false,
      }),
      "error",
    );
  });

  it("treats missing index status as loading, not empty", () => {
    assert.equal(
      resolveLibraryPresence({
        loading: false,
        error: null,
        itemCount: 0,
        rootCount: 1,
        indexing: false,
        statusKnown: false,
      }),
      "loading",
    );
  });
});
