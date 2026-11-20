import type { ReactNode } from "react";

/** Page title (focus target after navigation) and its lead paragraph. */
export function ScreenHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <header className="screen__header">
      <h1 tabIndex={-1} data-screen-title>
        {title}
      </h1>
      {children ? <p className="screen__lead">{children}</p> : null}
    </header>
  );
}
