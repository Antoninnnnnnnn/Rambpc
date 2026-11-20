// Inline SVG icons. Decorative: the adjacent text always carries the meaning,
// and each status has its own shape so color is never the only cue.

import type { ReactNode } from "react";
import type { StatusShape, Tone } from "../labels/fr";

function Svg({ children, className, shape }: { children: ReactNode; className?: string; shape?: string }) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      viewBox="0 0 24 24"
      width="24"
      height="24"
      aria-hidden="true"
      focusable="false"
      data-shape={shape}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

const STATUS_PATHS: Record<StatusShape, ReactNode> = {
  // Ring with a check mark.
  available: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12.5l2.7 2.7L16.2 9.5" />
    </>
  ),
  // Two open arcs.
  connecting: (
    <>
      <path d="M4.5 12a7.5 7.5 0 0 1 12.8-5.3" />
      <path d="M19.5 12a7.5 7.5 0 0 1-12.8 5.3" />
      <path d="M17.6 3.8v3.1h-3.1M6.4 20.2v-3.1h3.1" />
    </>
  ),
  // Hourglass.
  preparing: (
    <>
      <path d="M7 3.5h10M7 20.5h10" />
      <path d="M8 3.5c0 4 4 5.5 4 8.5s-4 4.5-4 8.5M16 3.5c0 4-4 5.5-4 8.5s4 4.5 4 8.5" />
    </>
  ),
  // Filled microphone.
  listening: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" fill="currentColor" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5M8.5 21h7" />
    </>
  ),
  // Three dots in a rounded box.
  finalizing: (
    <>
      <rect x="3" y="7" width="18" height="10" rx="5" />
      <circle cx="8" cy="12" r="1.2" fill="currentColor" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" />
      <circle cx="16" cy="12" r="1.2" fill="currentColor" />
    </>
  ),
  // Triangle with exclamation mark.
  unavailable: (
    <>
      <path d="M12 3.5l9.5 16.5h-19z" />
      <path d="M12 10v4.5" />
      <circle cx="12" cy="17.3" r="0.6" fill="currentColor" />
    </>
  ),
  // Padlock.
  setup: (
    <>
      <rect x="5" y="10.5" width="14" height="10" rx="2" />
      <path d="M8.5 10.5V7.5a3.5 3.5 0 0 1 7 0v3" />
      <path d="M12 14.5v2.5" />
    </>
  ),
};

export function StatusIcon({ shape, className }: { shape: StatusShape; className?: string }) {
  return (
    <Svg className={className} shape={shape}>
      {STATUS_PATHS[shape]}
    </Svg>
  );
}

const TONE_PATHS: Record<Tone, ReactNode> = {
  success: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12.5l2.7 2.7L16.2 9.5" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5.5" />
      <circle cx="12" cy="7.8" r="0.6" fill="currentColor" />
    </>
  ),
  warning: STATUS_PATHS.unavailable,
  error: (
    <>
      <path d="M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3z" />
      <path d="M9.2 9.2l5.6 5.6M14.8 9.2l-5.6 5.6" />
    </>
  ),
};

export function ToneIcon({ tone, className }: { tone: Tone; className?: string }) {
  return (
    <Svg className={className} shape={tone}>
      {TONE_PATHS[tone]}
    </Svg>
  );
}

export type GlyphName = "home" | "draft" | "settings" | "guide" | "copy" | "trash" | "mic" | "external";

const GLYPHS: Record<GlyphName, ReactNode> = {
  home: <path d="M4 11l8-7 8 7v8.5a1 1 0 0 1-1 1h-4.5v-6h-5v6H5a1 1 0 0 1-1-1z" />,
  draft: (
    <>
      <path d="M6 3h8l4 4v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
      <path d="M14 3v4h4M8.5 12h7M8.5 15.5h7" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7" />
      <circle cx="12" cy="12" r="6.5" />
    </>
  ),
  guide: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M10 8.5l5.5 3.5-5.5 3.5z" />
    </>
  ),
  copy: (
    <>
      <rect x="8" y="8" width="12" height="12" rx="2" />
      <path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3" />
    </>
  ),
  trash: <path d="M4.5 6.5h15M9.5 6.5V4h5v2.5M6.5 6.5l1 13.5h9l1-13.5M10.2 10v6.5M13.8 10v6.5" />,
  mic: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5" />
    </>
  ),
  external: <path d="M14 4h6v6M20 4l-8.5 8.5M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" />,
};

export function Glyph({ name, className }: { name: GlyphName; className?: string }) {
  return <Svg className={className}>{GLYPHS[name]}</Svg>;
}

/** Sambler's mark: a microphone on an accent tile (colors set in layout.css). */
export function AppMark() {
  return (
    <svg className="app-mark" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">
      <rect className="app-mark__tile" x="1" y="1" width="22" height="22" rx="6" />
      <rect className="app-mark__glyph" x="9" y="5" width="6" height="10" rx="3" />
      <path className="app-mark__stroke" d="M6.5 11.5a5.5 5.5 0 0 0 11 0M12 17v2.5" />
    </svg>
  );
}
