// Inline SVG icons. Decorative: the adjacent text always carries the meaning,
// and each status has its own shape so color is never the only cue.

import { useId, type ReactNode } from "react";
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
      strokeWidth="1.7"
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
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8.4 12.3l2.5 2.5 4.8-5.2" />
    </>
  ),
  // Two open arcs chasing each other.
  connecting: (
    <>
      <path d="M4.6 12a7.4 7.4 0 0 1 12.6-5.2" />
      <path d="M19.4 12a7.4 7.4 0 0 1-12.6 5.2" />
      <path d="M17.4 3.9v3.2h-3.2M6.6 20.1v-3.2h3.2" />
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
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5" />
    </>
  ),
  // Three dots in a capsule.
  finalizing: (
    <>
      <rect x="3" y="7.5" width="18" height="9" rx="4.5" />
      <circle cx="8" cy="12" r="1.15" fill="currentColor" />
      <circle cx="12" cy="12" r="1.15" fill="currentColor" />
      <circle cx="16" cy="12" r="1.15" fill="currentColor" />
    </>
  ),
  // Triangle with exclamation mark.
  unavailable: (
    <>
      <path d="M10.3 4.4a2 2 0 0 1 3.4 0l7 12.1a2 2 0 0 1-1.7 3H5a2 2 0 0 1-1.7-3z" />
      <path d="M12 9.5v4" />
      <circle cx="12" cy="16.6" r="0.5" fill="currentColor" />
    </>
  ),
  // Padlock.
  setup: (
    <>
      <rect x="5" y="10.5" width="14" height="10" rx="2.5" />
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
  success: STATUS_PATHS.available,
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5" />
      <circle cx="12" cy="7.9" r="0.5" fill="currentColor" />
    </>
  ),
  warning: STATUS_PATHS.unavailable,
  error: (
    <>
      <path d="M8.4 3.5h7.2l4.9 4.9v7.2l-4.9 4.9H8.4l-4.9-4.9V8.4z" />
      <path d="M9.4 9.4l5.2 5.2M14.6 9.4l-5.2 5.2" />
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

export type GlyphName =
  | "home"
  | "dictionary"
  | "shortcuts"
  | "style"
  | "history"
  | "settings"
  | "guide"
  | "copy"
  | "check"
  | "trash"
  | "mic"
  | "external"
  | "add"
  | "edit"
  | "search"
  | "chevron-down"
  | "arrow-right"
  | "minimize"
  | "maximize"
  | "restore"
  | "close"
  | "stop"
  | "keyboard"
  | "clipboard"
  | "shield"
  | "power"
  | "bell"
  | "key"
  | "globe"
  | "clock"
  | "inbox"
  | "notebook"
  | "activity"
  | "file-audio"
  | "apps"
  | "return";

const GLYPHS: Record<GlyphName, ReactNode> = {
  // A notebook with a spiral edge: the Carnet.
  notebook: (
    <>
      <path d="M7 3.5h10.5A1.5 1.5 0 0 1 19 5v14a1.5 1.5 0 0 1-1.5 1.5H7z" />
      <path d="M4.5 7h3M4.5 12h3M4.5 17h3M11 8.5h5M11 12h5" />
    </>
  ),
  // Bars of different heights: activity.
  activity: (
    <>
      <path d="M5 20V13M10 20V6M15 20v-9M20 20V9" />
      <path d="M3.5 20.5h17" />
    </>
  ),
  // A page with a sound wave: an audio file.
  "file-audio": (
    <>
      <path d="M14 3.5H7A1.5 1.5 0 0 0 5.5 5v14A1.5 1.5 0 0 0 7 20.5h10a1.5 1.5 0 0 0 1.5-1.5V8z" />
      <path d="M14 3.5V8h4.5M9 15v-2M11.5 17v-6M14 15.5v-3" />
    </>
  ),
  // Four tiles: applications.
  apps: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
    </>
  ),
  // A return arrow: a spoken layout command.
  return: (
    <>
      <path d="M19.5 5.5v6a3 3 0 0 1-3 3h-11" />
      <path d="M9 10.5l-4 4 4 4" />
    </>
  ),
  home: (
    <>
      <path d="M4 10.5L12 4l8 6.5V19a1.5 1.5 0 0 1-1.5 1.5h-3.5v-5.5h-6v5.5H5.5A1.5 1.5 0 0 1 4 19z" />
    </>
  ),
  // An open book.
  dictionary: (
    <>
      <path d="M12 6.5c-1.8-1.3-4.3-2-7.5-2v13c3.2 0 5.7.7 7.5 2 1.8-1.3 4.3-2 7.5-2v-13c-3.2 0-5.7.7-7.5 2z" />
      <path d="M12 6.5v13" />
    </>
  ),
  // A speech bubble with a spark: a phrase that expands.
  shortcuts: (
    <>
      <path d="M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8.5A1.5 1.5 0 0 1 19 17h-8l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5V7A1.5 1.5 0 0 1 5 5.5z" />
      <path d="M12.6 8.2l-2.2 3.3h3.2l-2.2 3.3" />
    </>
  ),
  // "Aa": typography.
  style: (
    <>
      <path d="M3.5 18l4.5-12 4.5 12M5.2 13.6h5.6" />
      <path d="M20.5 18v-5.2a2.8 2.8 0 0 0-5.3-1.2M20.5 15.5c-.6 1.6-1.7 2.5-3.1 2.5-1.3 0-2.2-.8-2.2-1.9 0-1.4 1.4-2 5.3-2.1" />
    </>
  ),
  history: (
    <>
      <path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3" />
      <path d="M4 4v3.5h3.5M12 8v4.5l3 2" />
    </>
  ),
  // Two sliders.
  settings: (
    <>
      <path d="M4 7.5h9M17 7.5h3M4 16.5h3M11 16.5h9" />
      <circle cx="15" cy="7.5" r="2" />
      <circle cx="9" cy="16.5" r="2" />
    </>
  ),
  // A compass star.
  guide: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M14.8 9.2l-1.6 4-4 1.6 1.6-4z" />
    </>
  ),
  copy: (
    <>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2.2" />
      <path d="M15.5 8.5V6.2A1.7 1.7 0 0 0 13.8 4.5H6.2A1.7 1.7 0 0 0 4.5 6.2v7.6a1.7 1.7 0 0 0 1.7 1.7h2.3" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  trash: (
    <>
      <path d="M4.5 6.5h15M9.5 6.5V4.8a.8.8 0 0 1 .8-.8h3.4a.8.8 0 0 1 .8.8v1.7" />
      <path d="M6.5 6.5l.9 12.1a1.5 1.5 0 0 0 1.5 1.4h6.2a1.5 1.5 0 0 0 1.5-1.4l.9-12.1M10.2 10.5v5.5M13.8 10.5v5.5" />
    </>
  ),
  mic: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5" />
    </>
  ),
  external: <path d="M14 4.5h5.5V10M19.5 4.5L11 13M17.5 14v4.5a1.5 1.5 0 0 1-1.5 1.5H6a1.5 1.5 0 0 1-1.5-1.5V8.5A1.5 1.5 0 0 1 6 7h4.5" />,
  add: <path d="M12 5v14M5 12h14" />,
  edit: (
    <>
      <path d="M4.5 19.5l1-4.2L15.8 5a2 2 0 0 1 2.9 0l.3.3a2 2 0 0 1 0 2.9L8.7 18.5z" />
      <path d="M14 6.8l3.2 3.2" />
    </>
  ),
  search: (
    <>
      <circle cx="10.8" cy="10.8" r="6" />
      <path d="M15.3 15.3l4.7 4.7" />
    </>
  ),
  "chevron-down": <path d="M6.5 9.5l5.5 5.5 5.5-5.5" />,
  "arrow-right": <path d="M5 12h14M13.5 6.5L19 12l-5.5 5.5" />,
  minimize: <path d="M6.5 12h11" />,
  maximize: <rect x="6.5" y="6.5" width="11" height="11" rx="1.8" />,
  restore: (
    <>
      <rect x="5.5" y="8.5" width="10" height="10" rx="1.8" />
      <path d="M8.5 5.5h8.2a1.8 1.8 0 0 1 1.8 1.8v8.2" />
    </>
  ),
  close: <path d="M7 7l10 10M17 7L7 17" />,
  stop: <rect x="7" y="7" width="10" height="10" rx="2.2" fill="currentColor" stroke="none" />,
  keyboard: (
    <>
      <rect x="3" y="6" width="18" height="12" rx="2.5" />
      <path d="M7 10h.01M10.3 10h.01M13.7 10h.01M17 10h.01M8 14h8" />
    </>
  ),
  clipboard: (
    <>
      <rect x="5.5" y="5" width="13" height="15.5" rx="2" />
      <path d="M9 5V4.2a.7.7 0 0 1 .7-.7h4.6a.7.7 0 0 1 .7.7V5M9 11h6M9 14.5h4" />
    </>
  ),
  shield: (
    <>
      <path d="M12 3.5l7 2.8v5.2c0 4.3-2.9 7.6-7 9-4.1-1.4-7-4.7-7-9V6.3z" />
      <path d="M9 12l2.2 2.2L15.3 10" />
    </>
  ),
  power: (
    <>
      <path d="M12 3.5v8" />
      <path d="M7 6.5a7.5 7.5 0 1 0 10 0" />
    </>
  ),
  bell: (
    <>
      <path d="M6.5 16.5V11a5.5 5.5 0 0 1 11 0v5.5l1.5 2H5z" />
      <path d="M10 20.5a2.2 2.2 0 0 0 4 0" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15" r="4" />
      <path d="M10.9 12.1L19 4M15.5 7.5l2.5 2.5M17.5 5.5l2 2" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.3 2.4 3.5 5.2 3.5 8.5s-1.2 6.1-3.5 8.5c-2.3-2.4-3.5-5.2-3.5-8.5S9.7 5.9 12 3.5z" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  // A tray: text kept in Sambler.
  inbox: (
    <>
      <path d="M4 13.5l2.2-7.2A1.5 1.5 0 0 1 7.6 5.2h8.8a1.5 1.5 0 0 1 1.4 1.1l2.2 7.2V18a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 18z" />
      <path d="M4 13.5h4.5l1 2h5l1-2H20" />
    </>
  ),
};

export function Glyph({ name, className }: { name: GlyphName; className?: string }) {
  return <Svg className={className}>{GLYPHS[name]}</Svg>;
}

/**
 * Sambler's mark: a tile with a text cursor and two voice waves
 * (app-icon.svg), redrawn with a softer gradient and a light sheen. Its
 * colours come from the theme (`--mark-*`): plum and braise in Verre et
 * aurore, pêche to lilas in Doux et rond.
 */
export function AppMark({ className }: { className?: string }) {
  const id = useId().replace(/:/g, "");
  return (
    <svg
      className={className ? `app-mark ${className}` : "app-mark"}
      viewBox="0 0 1024 1024"
      width="24"
      height="24"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id={`${id}-tile`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style={{ stopColor: "var(--mark-tile-from)" }} />
          <stop offset="1" style={{ stopColor: "var(--mark-tile-to)" }} />
        </linearGradient>
        <linearGradient id={`${id}-sheen`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: "var(--mark-ink)", stopOpacity: 0.22 }} />
          <stop offset="0.55" style={{ stopColor: "var(--mark-ink)", stopOpacity: 0 }} />
        </linearGradient>
      </defs>
      <rect className="app-mark__tile" x="64" y="64" width="896" height="896" rx="216" fill={`url(#${id}-tile)`} />
      <rect x="64" y="64" width="896" height="896" rx="216" fill={`url(#${id}-sheen)`} />
      <g style={{ fill: "var(--mark-ink)" }}>
        <rect x="300" y="262" width="56" height="500" rx="28" />
        <rect x="236" y="262" width="184" height="44" rx="22" />
        <rect x="236" y="718" width="184" height="44" rx="22" />
      </g>
      <path
        d="M500 390a150 150 0 0 1 0 244"
        fill="none"
        style={{ stroke: "var(--mark-wave)" }}
        strokeWidth="56"
        strokeLinecap="round"
      />
      <path
        d="M600 310a250 250 0 0 1 0 404"
        fill="none"
        style={{ stroke: "var(--mark-ink)" }}
        strokeWidth="56"
        strokeLinecap="round"
      />
    </svg>
  );
}
