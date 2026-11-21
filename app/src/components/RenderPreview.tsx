// Effect of the style settings on a sample sentence, without opening the
// microphone (`preview_render`). Hidden while pending or if the preview fails.

import { useEffect, useState } from "react";
import { useBridge } from "../ipc/bridge";
import type { Language, Settings } from "../ipc/contract";
import { textLang } from "../labels/history";

const FRENCH_SAMPLE = "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient";

/**
 * Raw recognizer-like samples owned by the UI (not dictated text). In automatic
 * mode the French sample gets French typography, as a French take would.
 */
export const PREVIEW_SAMPLES: Record<Language, string> = {
  auto: FRENCH_SAMPLE,
  french: FRENCH_SAMPLE,
  english: "i will not be able to come on thursday at 2 pm does tuesday work for you",
};

/** `preview_render` for `sample`, debounced; null while pending or on failure. */
export function usePreviewRender(sample: string, settings: Settings): string | null {
  const bridge = useBridge();
  const [rendered, setRendered] = useState<string | null>(null);
  const key = JSON.stringify([sample, settings.language, settings.profile, settings.text]);

  useEffect(() => {
    let cancelled = false;
    setRendered(null);
    if (!sample.trim()) return;
    const timer = window.setTimeout(() => {
      bridge
        .invoke("preview_render", { sample, settings })
        .then((result) => {
          if (!cancelled) setRendered(result);
        })
        .catch(() => {
          if (!cancelled) setRendered(null);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // `key` captures the sample and every setting that affects rendering.
  }, [bridge, key]);

  return rendered;
}

interface Props {
  settings: Settings;
  sample: string;
  /** Labels of the two sides; « Reconnu » and « Inséré » by default. */
  labels?: { raw: string; result: string };
  /** Only the rendered side (the raw sentence is shown elsewhere). */
  compact?: boolean;
  /** A small title above the preview, hidden with it. */
  caption?: string;
  testId?: string;
}

export function RenderPreview({ settings, sample, labels, compact = false, caption, testId = "render-preview" }: Props) {
  const rendered = usePreviewRender(sample, settings);
  const lang = textLang(settings.language);
  if (rendered === null) return null;

  const preview = (
    <dl className={compact ? "preview preview--compact" : "preview"} data-testid={testId}>
      {compact ? null : (
        <div className="preview__side">
          <dt className="preview__label">{labels?.raw ?? "Reconnu"}</dt>
          <dd lang={lang} className="preview__text preview__text--raw">
            {sample}
          </dd>
        </div>
      )}
      <div className="preview__side preview__side--result">
        <dt className="preview__label">{labels?.result ?? "Inséré"}</dt>
        <dd lang={lang} className="preview__text">
          {rendered}
        </dd>
      </div>
    </dl>
  );
  if (!caption) return preview;
  return (
    <div className="preview-block">
      <p className="preview-block__caption">{caption}</p>
      {preview}
    </div>
  );
}
