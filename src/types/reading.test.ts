import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  chapterVariantPreference,
  currentChapterVariantPreference,
  detectMediaKind,
  detectMediaKindFromSeries,
  detectViewerKindFromItem,
  effectiveReadingMode,
  groupTilesByPage,
  isSupportedLibraryFile,
  nextVariantPreference,
  previousVariantPreference,
  pageFromScroll,
  preferredChapterSequence,
  untranslatedCjkNotice,
  type Chapter,
  type Series,
} from "./index.ts";

function series(partial: Partial<Series>): Series {
  return {
    id: 1,
    rootId: 1,
    folderPath: "/lib/s",
    title: "Test",
    readingMode: "auto",
    pageDirection: "rtl",
    favorite: false,
    chapterCount: 1,
    unreadCount: 0,
    progressPercent: 0,
    createdAt: "",
    updatedAt: "",
    tags: [],
    ...partial,
  };
}

function chapter(partial: Partial<Chapter>): Chapter {
  return {
    id: 1,
    seriesId: 1,
    filePath: "/lib/s/ch001.pdf",
    title: "Chapter 1",
    chapterNumber: 1,
    sortKey: 1,
    pageCount: 1,
    progressPercent: 0,
    lastPageIndex: 0,
    createdAt: "",
    updatedAt: "",
    missing: false,
    ...partial,
  };
}

describe("effectiveReadingMode", () => {
  it("honors an explicit webtoon mode", () => {
    assert.equal(effectiveReadingMode(series({ readingMode: "webtoon" })), "webtoon");
  });

  it("treats Auto + LTR as webtoon after analysis", () => {
    assert.equal(
      effectiveReadingMode(series({ readingMode: "auto", pageDirection: "ltr" })),
      "webtoon",
    );
  });

  it("treats Auto + RTL as paged RTL manga", () => {
    assert.equal(
      effectiveReadingMode(series({ readingMode: "auto", pageDirection: "rtl" })),
      "paged_rtl",
    );
  });
});

describe("pageFromScroll", () => {
  it("maps scroll position to the visible page with no inter-page gap", () => {
    const heights = [1000, 1000, 1000];
    assert.equal(pageFromScroll(0, heights), 0);
    assert.equal(pageFromScroll(991, heights, 0), 0);
    assert.equal(pageFromScroll(1000, heights, 0), 1);
    assert.equal(pageFromScroll(1010, heights), 1);
    assert.equal(pageFromScroll(2500, heights), 2);
  });
});

describe("groupTilesByPage", () => {
  const tile = (pageIndex: number, tileIndex: number) => ({
    pageIndex,
    tileIndex,
    width: 1200,
    height: 2400,
  });

  it("groups a flat manifest into per-page runs so overlays can span a whole page", () => {
    const groups = groupTilesByPage([
      tile(0, 0),
      tile(0, 1),
      tile(0, 2),
      tile(1, 0),
      tile(1, 1),
    ]);

    assert.equal(groups.length, 2);
    assert.deepEqual(
      groups.map((g) => [g.pageIndex, g.tiles.length]),
      [
        [0, 3],
        [1, 2],
      ],
    );
  });

  it("keeps every segment and preserves manifest order", () => {
    const manifest = [tile(0, 0), tile(1, 0), tile(1, 1), tile(2, 0)];
    const groups = groupTilesByPage(manifest);

    assert.equal(
      groups.reduce((total, group) => total + group.tiles.length, 0),
      manifest.length,
    );
    assert.deepEqual(
      groups.flatMap((g) => g.tiles.map((t) => t.tileIndex)),
      [0, 0, 1, 0],
    );
  });

  it("returns nothing for an empty manifest", () => {
    assert.deepEqual(groupTilesByPage([]), []);
  });
});

describe("untranslatedCjkNotice", () => {
  it("flags Japanese OCR that was not translated", () => {
    assert.equal(
      untranslatedCjkNotice([{ id: "1", x: 0, y: 0, width: 1, height: 1, text: "こんにちは", vertical: false }]),
      true,
    );
  });

  it("ignores Japanese punctuation that Vision used to sprinkle on Chinese", () => {
    assert.equal(
      untranslatedCjkNotice([{ id: "1", x: 0, y: 0, width: 1, height: 1, text: "…・・呃・", vertical: false }]),
      false,
    );
  });
});

describe("media classification", () => {
  it("labels numbered CJK chapter PDFs as manga", () => {
    assert.equal(detectMediaKind("/library/槍彈辯駁/第01话.pdf").contentType, "manga");
  });

  it("labels mp4 files as video content and mp4 format", () => {
    assert.deepEqual(detectMediaKind("/library/Videos/Lecture.mp4"), {
      contentType: "video",
      sourceFormat: "mp4",
      isSupported: true,
    });
  });

  it("trusts persisted series media classification", () => {
    assert.equal(
      detectMediaKindFromSeries(series({ contentType: "video", sourceFormat: "mp4" })).contentType,
      "video",
    );
  });

  it("recognizes supported library files while rejecting unsupported files", () => {
    assert.equal(isSupportedLibraryFile("/library/One Piece/Chapter 001.pdf"), true);
    assert.equal(isSupportedLibraryFile("/library/Videos/Lecture.mp4"), true);
    assert.equal(isSupportedLibraryFile("/library/notes.txt"), false);
  });
});

describe("variant preference selection", () => {
  it("cycles variant preference order and wraps to primary", () => {
    assert.equal(nextVariantPreference("primary"), "uncensored");
    assert.equal(nextVariantPreference("uncensored"), "revised");
    assert.equal(nextVariantPreference("revised"), "bonus");
    assert.equal(nextVariantPreference("bonus"), "alternate");
    assert.equal(nextVariantPreference("alternate"), "primary");
  });

  it("cycles variant preference backward and wraps to alternate", () => {
    assert.equal(previousVariantPreference("primary"), "alternate");
    assert.equal(previousVariantPreference("alternate"), "bonus");
    assert.equal(previousVariantPreference("bonus"), "revised");
    assert.equal(previousVariantPreference("revised"), "uncensored");
    assert.equal(previousVariantPreference("uncensored"), "primary");
  });

  it("detects known variant types from chapter titles", () => {
    assert.equal(chapterVariantPreference("Vol 1 Ch 2 無修正"), "uncensored");
    assert.equal(chapterVariantPreference("Vol 1 Ch 2 修正"), "revised");
    assert.equal(chapterVariantPreference("Vol 1 Ch 2 番外"), "bonus");
    assert.equal(chapterVariantPreference("Vol 1 Ch 2 alt mirror"), "alternate");
  });

  it("prefers configured variant within each chapter group and falls back to primary", () => {
    const chapters = [
      chapter({ id: 1, title: "Ch 1", chapterNumber: 1, sortKey: 1000 }),
      chapter({ id: 2, title: "Ch 1 無修正", chapterNumber: 1, sortKey: 1010 }),
      chapter({ id: 3, title: "Ch 2", chapterNumber: 2, sortKey: 2000 }),
      chapter({ id: 4, title: "Ch 2 修正", chapterNumber: 2, sortKey: 2010 }),
      chapter({ id: 5, title: "Ch 3", chapterNumber: 3, sortKey: 3000 }),
    ];

    assert.deepEqual(
      preferredChapterSequence(chapters, "uncensored").map((c) => c.id),
      [2, 3, 5],
    );
    assert.deepEqual(
      preferredChapterSequence(chapters, "revised").map((c) => c.id),
      [1, 4, 5],
    );
    assert.deepEqual(
      preferredChapterSequence(chapters, "primary").map((c) => c.id),
      [1, 3, 5],
    );
  });

  it("identifies whether current chapter is primary or alternate", () => {
    const chapters = [
      chapter({ id: 10, title: "Ch 10", chapterNumber: 10, sortKey: 10000 }),
      chapter({ id: 11, title: "Ch 10 修正", chapterNumber: 10, sortKey: 10010 }),
    ];
    assert.equal(currentChapterVariantPreference(chapters[0]!, chapters), "primary");
    assert.equal(currentChapterVariantPreference(chapters[1]!, chapters), "revised");
  });
});

describe("viewer routing", () => {
  it("opens manga, document, and video viewers from persisted format plus file path", () => {
    assert.equal(
      detectViewerKindFromItem(
        chapter({ filePath: "/lib/One Piece/Chapter 001.pdf" }),
        series({ contentType: "manga", sourceFormat: "pdf" }),
      ),
      "manga",
    );
    assert.equal(
      detectViewerKindFromItem(
        chapter({ filePath: "/lib/Notes/manual.pdf" }),
        series({ contentType: "document", sourceFormat: "pdf" }),
      ),
      "manga",
    );
    assert.equal(
      detectViewerKindFromItem(
        chapter({ filePath: "/lib/Show/Episode 01.mp4" }),
        series({ contentType: "video", sourceFormat: "mp4" }),
      ),
      "video",
    );
  });

  it("routes a PDF by file extension even if series metadata still says video", () => {
    assert.equal(
      detectViewerKindFromItem(
        chapter({ filePath: "/lib/Movies/Chapter 001.pdf" }),
        series({ contentType: "video", sourceFormat: "mp4", folderPath: "/lib/Movies", title: "Movies" }),
      ),
      "manga",
    );
  });
});
