// Dictated text of one result, as plain React text (never HTML), with every
// gap marked in place: « [passage manquant] », « [à revoir] »…

import type { DraftPart, DraftView } from "../ipc/contract";
import { GAP_HELP, GAP_LABELS, type GapKind } from "../labels/recents";

export function DraftParts({ parts }: { parts: DraftPart[] }) {
  return (
    <>
      {parts.map((part, index) => {
        const separator = index > 0 ? " " : "";
        if (part.status === "admitted") {
          return (
            <span key={index}>
              {separator}
              {part.text ?? ""}
            </span>
          );
        }
        return (
          <span key={index}>
            {separator}
            <span className={`gap gap--${part.status}`} data-gap={part.status}>
              <span className="visually-hidden">[</span>
              {GAP_LABELS[part.status]}
              <span className="visually-hidden">]</span>
            </span>
            {part.status === "needs-review" && part.text ? (
              <>
                {" "}
                <mark className="review-text">{part.text}</mark>
              </>
            ) : null}
          </span>
        );
      })}
    </>
  );
}

/** The full text when the take is complete, else its parts with the gaps marked. */
export function DraftText({ view }: { view: DraftView }) {
  return view.fullText !== null ? <>{view.fullText}</> : <DraftParts parts={view.parts} />;
}

export function gapKinds(parts: DraftPart[]): GapKind[] {
  const kinds = new Set<GapKind>();
  for (const part of parts) if (part.status !== "admitted") kinds.add(part.status);
  return [...kinds];
}

export function GapLegend({ kinds, idPrefix }: { kinds: GapKind[]; idPrefix: string }) {
  if (kinds.length === 0) return null;
  return (
    <ul className="gap-legend" aria-label="Signification des repères">
      {kinds.map((kind) => (
        <li key={kind} id={`${idPrefix}-gap-${kind}`} data-testid={`gap-help-${kind}`}>
          <span className={`gap gap--${kind}`}>{GAP_LABELS[kind]}</span>&nbsp;: {GAP_HELP[kind]}
        </li>
      ))}
    </ul>
  );
}
