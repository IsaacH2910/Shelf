export type ReaderShortcut =
  | { type: "back" }
  | { type: "fullscreen" }
  | { type: "overlay" }
  | { type: "variant"; direction: "forward" | "backward" }
  | { type: "scroll"; direction: "up" | "down" | "home" | "end" };

const TEXT_INPUT_TYPES = new Set([
  "text",
  "search",
  "password",
  "email",
  "number",
  "url",
  "tel",
  "date",
  "datetime-local",
  "month",
  "time",
  "week",
]);

/** True when the event originated in a field that should keep its own keys. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const field = target.closest("input, textarea, select");
  if (!field) return false;
  if (field instanceof HTMLTextAreaElement || field instanceof HTMLSelectElement) return true;
  if (field instanceof HTMLInputElement) {
    return TEXT_INPUT_TYPES.has((field.type || "text").toLowerCase());
  }
  return false;
}

export function resolveReaderShortcut(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "metaKey" | "ctrlKey" | "altKey">,
): ReaderShortcut | null {
  if (event.metaKey || event.ctrlKey || event.altKey) return null;

  switch (event.key) {
    case "Escape":
      return { type: "back" };
    case "f":
    case "F":
      return { type: "fullscreen" };
    case "t":
    case "T":
      return { type: "overlay" };
    case "v":
    case "V":
      return { type: "variant", direction: event.shiftKey ? "backward" : "forward" };
    case "ArrowDown":
    case "PageDown":
    case "ArrowRight":
    case " ":
      return { type: "scroll", direction: "down" };
    case "ArrowUp":
    case "PageUp":
    case "ArrowLeft":
      return { type: "scroll", direction: "up" };
    case "Home":
      return { type: "scroll", direction: "home" };
    case "End":
      return { type: "scroll", direction: "end" };
    default:
      return null;
  }
}
