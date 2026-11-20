import { useId, type ReactNode } from "react";
import type { Tone } from "../labels/fr";
import { ToneIcon } from "./Icon";
import "./InfoBar.css";

interface Props {
  tone: Tone;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  headingLevel?: 2 | 3 | "none";
  testId?: string;
}

/** Windows-style InfoBar: tone icon (distinct shapes), title, body, actions. */
export function InfoBar({ tone, title, children, actions, headingLevel = 2, testId }: Props) {
  const titleId = useId();
  const Title = headingLevel === "none" ? "p" : headingLevel === 2 ? "h2" : "h3";
  return (
    <section
      className={`infobar infobar--${tone}`}
      aria-labelledby={titleId}
      data-tone={tone}
      data-testid={testId}
    >
      <ToneIcon tone={tone} className="infobar__icon" />
      <div className="infobar__content">
        <Title id={titleId} className="infobar__title">
          {title}
        </Title>
        {children ? <div className="infobar__body">{children}</div> : null}
        {actions ? <div className="infobar__actions button-row">{actions}</div> : null}
      </div>
    </section>
  );
}
