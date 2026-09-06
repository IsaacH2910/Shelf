import { useEffect, useRef, useState } from "react";
import type { OcrRegion, OverlayMode } from "../types";

interface TranslationOverlayProps {
  regions: OcrRegion[];
  mode: OverlayMode;
  onEdit?: (region: OcrRegion) => void;
}

export function TranslationOverlay({ regions, mode, onEdit }: TranslationOverlayProps) {
  if (mode === "off" || regions.length === 0) return null;

  return (
    <div className="absolute inset-0">
      {regions.filter((r) => !r.hidden).map((region) => {
        const showOriginal = mode === "both";
        const text = mode === "on" || mode === "both"
          ? region.translatedText ?? region.text
          : region.text;
        return (
          <button
            key={region.id}
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onEdit?.(region);
            }}
            className="absolute rounded border border-border bg-surface/95 px-1.5 py-0.5 text-left text-[11px] leading-snug text-text hover:ring-1 hover:ring-accent"
            style={{
              left: `${region.x * 100}%`,
              top: `${region.y * 100}%`,
              width: `${Math.max(region.width * 100, 8)}%`,
              writingMode: region.vertical ? "vertical-rl" : "horizontal-tb",
            }}
          >
            {showOriginal && region.translatedText && (
              <span className="mb-0.5 block text-[10px] text-muted line-through">
                {region.text}
              </span>
            )}
            {text}
          </button>
        );
      })}
    </div>
  );
}

interface EditDialogProps {
  region: OcrRegion;
  onSave: (region: OcrRegion) => void;
  onClose: () => void;
}

export function RegionEditDialog({ region, onSave, onClose }: EditDialogProps) {
  const originalRef = useRef<HTMLTextAreaElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const [original, setOriginal] = useState(region.text);
  const [translated, setTranslated] = useState(region.translatedText ?? "");

  useEffect(() => {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    originalRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      openerRef.current?.focus();
    };
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/75" role="dialog" aria-modal="true" aria-labelledby="translation-dialog-title" onClick={onClose}>
      <div
        className="w-96 rounded-xl border border-border bg-surface p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="translation-dialog-title" className="mb-3 text-sm font-medium text-text">Edit translation</h3>
        <label className="mb-1 block text-xs text-muted">Original</label>
        <textarea
          ref={originalRef}
          value={original}
          onChange={(e) => setOriginal(e.target.value)}
          className="mb-3 h-20 w-full rounded-lg border border-border bg-elevated p-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
        />
        <label className="mb-1 block text-xs text-muted">Traditional Chinese</label>
        <textarea
          value={translated}
          onChange={(e) => setTranslated(e.target.value)}
          className="mb-4 h-20 w-full rounded-lg border border-border bg-elevated p-2 text-sm text-text focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
        />
        <div className="flex justify-end gap-2">
          <button
            onClick={() => onSave({ ...region, hidden: true, text: original, translatedText: translated || undefined })}
            className="reader-btn mr-auto rounded-lg px-3 py-1.5 text-sm"
          >
            Hide
          </button>
          <button onClick={onClose} className="reader-btn rounded-lg px-3 py-1.5 text-sm">
            Cancel
          </button>
          <button
            onClick={() =>
              onSave({ ...region, text: original, translatedText: translated || undefined, hidden: false })
            }
            className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-bg hover:bg-accent-hover"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
