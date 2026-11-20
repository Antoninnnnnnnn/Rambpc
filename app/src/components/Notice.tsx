import { useId, type ReactNode } from "react";
import type { Tone } from "../labels/fr";
import { ToneIcon } from "./Icon";
import "./Notice.css";

interface Props {
  tone: Tone;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  headingLevel?: 2 | 3 | "none";
  testId?: string;
  /** Lighter variant (no tinted background), for routine results. */
  quiet?: boolean;
}

/** A tone icon (distinct shapes), a title, an optional body and actions. */
export function Notice({ tone, title, children, actions, headingLevel = 2, testId, quiet = false }: Props) {
  const titleId = useId();
  const Title = headingLevel === "none" ? "p" : headingLevel === 2 ? "h2" : "h3";
  return (
    <section
      className={quiet ? `notice notice--${tone} notice--quiet` : `notice notice--${tone}`}
      aria-labelledby={titleId}
      data-tone={tone}
      data-testid={testId}
    >
      <span className="notice__icon">
        <ToneIcon tone={tone} />
      </span>
      <div className="notice__content">
        <Title id={titleId} className="notice__title">
          {title}
        </Title>
        {children ? <div className="notice__body">{children}</div> : null}
        {actions ? <div className="notice__actions btn-row">{actions}</div> : null}
      </div>
    </section>
  );
}
