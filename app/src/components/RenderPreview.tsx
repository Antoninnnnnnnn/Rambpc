// Effect of the style settings on a sample sentence, without opening the
// microphone (`preview_render`). Hidden while pending or if the preview fails.

import { useEffect, useState } from "react";
import { useBridge } from "../ipc/bridge";
import type { Settings } from "../ipc/contract";

/** Raw recognizer-like samples owned by the UI (not dictated text). */
export const PREVIEW_SAMPLES = {
  french: "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient",
  english: "i will not be able to come on thursday at 2 pm does tuesday work for you",
} as const;

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

export function RenderPreview({ settings, sample }: { settings: Settings; sample: string }) {
  const rendered = usePreviewRender(sample, settings);
  const lang = settings.language === "english" ? "en" : "fr";
  if (rendered === null) return null;

  return (
    <dl className="preview" data-testid="render-preview">
      <div className="preview__side">
        <dt className="preview__label">Reconnu</dt>
        <dd lang={lang} className="preview__text preview__text--raw">
          {sample}
        </dd>
      </div>
      <div className="preview__side preview__side--result">
        <dt className="preview__label">Inséré</dt>
        <dd lang={lang} className="preview__text">
          {rendered}
        </dd>
      </div>
    </dl>
  );
}
