import { Search } from "lucide-react";
import { Chip } from "./Chip";
import { Input } from "./Input";
import { Select } from "./Select";
import { cn } from "../lib/utils";

export function SearchBar({
  value,
  onChange,
  placeholder = "Search your library…",
  autoFocus,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  return (
    <div className="relative">
      <Search size={18} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
      <Input
        type="search"
        aria-label="Search library"
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className={cn(
          "h-11 rounded-xl py-2.5 pl-10 pr-4 text-base md:text-sm",
        )}
      />
    </div>
  );
}

export function SortFilter({ sort, onSortChange }: { sort: string; onSortChange: (sort: string) => void }) {
  return (
    <Select
      aria-label="Sort library"
      value={sort}
      onChange={(e) => onSortChange(e.target.value)}
      className="h-11 rounded-xl"
    >
      <option value="title">Title</option>
      <option value="last_read">Last Read</option>
      <option value="added">Recently Added</option>
      <option value="progress">Progress</option>
    </Select>
  );
}

export function FilterChips({
  contentType,
  onContentType,
}: {
  contentType: "all" | "manga" | "document" | "video";
  onContentType: (v: "all" | "manga" | "document" | "video") => void;
}) {
  const chip = (active: boolean, label: string, onClick: () => void) => (
    <Chip onClick={onClick} active={active}>
      {label}
    </Chip>
  );
  return (
    <div className="no-scrollbar flex gap-2 overflow-x-auto">
      {chip(contentType === "all", "All media", () => onContentType("all"))}
      {chip(contentType === "manga", "Manga", () => onContentType("manga"))}
      {chip(contentType === "video", "Video", () => onContentType("video"))}
      {chip(contentType === "document", "Documents", () => onContentType("document"))}
    </div>
  );
}
