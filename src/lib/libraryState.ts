export type LibraryPresence =
  | "loading"
  | "error"
  | "indexing"
  | "empty-no-folders"
  | "empty-no-files"
  | "ready";

export function resolveLibraryPresence(input: {
  loading: boolean;
  error: string | null;
  itemCount: number;
  rootCount: number | null;
  indexing: boolean;
  statusKnown?: boolean;
}): LibraryPresence {
  if (input.itemCount > 0) return "ready";
  if (input.loading) return "loading";
  if (input.error) return "error";
  if (input.statusKnown === false) return "loading";
  if (input.indexing) return "indexing";
  if (input.rootCount == null) return "loading";
  if (input.rootCount === 0) return "empty-no-folders";
  return "empty-no-files";
}
