// OPTIONAL CAPABILITY: shows the effect of the rendering settings on a fixed
// sample sentence, without opening the microphone. Uses the proposed native
// command `preview_render` (see OptionalCommandMap in ipc/contract.ts) and
// renders nothing when the native side does not provide it.

import { useEffect, useState } from "react";
import { useBridge } from "../ipc/bridge";
import type { Settings } from "../ipc/contract";

/** Raw recognizer-like samples owned by the UI (not dictated text). */
export const PREVIEW_SAMPLES = {
  french: "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient",
  english: "i will not be able to come on thursday at 2 pm does tuesday work for you",
} as const;

export function RenderPreview({ settings }: { settings: Settings }) {
  const bridge = useBridge();
  const [rendered, setRendered] = useState<string | null>(null);
  const [supported, setSupported] = useState(true);
  const sample = PREVIEW_SAMPLES[settings.language];
  const key = JSON.stringify([settings.language, settings.profile, settings.text]);

  useEffect(() => {
    if (!supported) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      bridge
        .invokeOptional("preview_render", { sample, settings })
        .then((result) => {
          if (cancelled) return;
          if (result === undefined) setSupported(false);
          else setRendered(result);
        })
        .catch(() => {
          if (!cancelled) setRendered(null);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // `key` captures every setting that affects rendering.
  }, [bridge, key, supported]);

  if (!supported || rendered === null) return null;

  return (
    <div className="render-preview" data-testid="render-preview">
      <p className="render-preview__title">Exemple, sans ouvrir le micro</p>
      <dl className="render-preview__pairs">
        <div>
          <dt>Reconnu</dt>
          <dd lang={settings.language === "english" ? "en" : "fr"}>{sample}</dd>
        </div>
        <div>
          <dt>Inséré</dt>
          <dd lang={settings.language === "english" ? "en" : "fr"}>{rendered}</dd>
        </div>
      </dl>
    </div>
  );
}
