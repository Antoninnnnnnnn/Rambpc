// Shared pieces of the Activité charts: the card (plate), the hover/focus
// tooltip and the table view every chart has as its text alternative.

import {
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
  type ReactNode,
} from "react";

interface ActivityCardProps {
  title: string;
  /** Short context on the right of the title (period, total). */
  meta?: ReactNode;
  children: ReactNode;
  className?: string;
  testId?: string;
}

/** A titled plate: <section> labelled by its h2. */
export function ActivityCard({ title, meta, children, className, testId }: ActivityCardProps) {
  const headingId = useId();
  return (
    <section
      className={className ? `card activity-card ${className}` : "card activity-card"}
      aria-labelledby={headingId}
      data-testid={testId}
    >
      <div className="activity-card__head">
        <h2 id={headingId} className="card__title activity-card__title">
          {title}
        </h2>
        {meta ? <p className="activity-card__meta">{meta}</p> : null}
      </div>
      {children}
    </section>
  );
}

interface Tip {
  value: string;
  label: string;
  x: number;
  y: number;
}

/**
 * One tooltip per chart, for marks carrying `data-tip-value` and
 * `data-tip-label`. It only repeats what the table view says (aria-hidden),
 * shows on hover and on keyboard focus, and reads text with textContent.
 * A hit target larger than its mark (a bar's column) sets
 * `data-tip-anchor="first"`: the tooltip then sits above its first child.
 */
export function useChartTip() {
  const hostRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLSpanElement>(null);
  const [tip, setTip] = useState<Tip | null>(null);

  const show = useCallback((target: EventTarget | null) => {
    const host = hostRef.current;
    const mark = target instanceof Element ? target.closest<HTMLElement>("[data-tip-value]") : null;
    if (!host || !mark || !host.contains(mark)) {
      setTip(null);
      return;
    }
    const anchor = (mark.dataset.tipAnchor === "first" && mark.firstElementChild) || mark;
    const box = host.getBoundingClientRect();
    const rect = anchor.getBoundingClientRect();
    setTip({
      value: mark.dataset.tipValue ?? "",
      label: mark.dataset.tipLabel ?? "",
      x: rect.left + rect.width / 2 - box.left,
      y: rect.top - box.top,
    });
  }, []);

  // Keep the tooltip inside the chart's width once its own width is known.
  useLayoutEffect(() => {
    const element = tipRef.current;
    const host = hostRef.current;
    if (!tip || !element || !host) return;
    const half = element.offsetWidth / 2;
    const width = host.clientWidth;
    const x = width > 2 * half ? Math.min(Math.max(tip.x, half), width - half) : width / 2;
    element.style.left = `${x}px`;
  }, [tip]);

  const hide = useCallback(() => setTip(null), []);

  const handlers = {
    onPointerOver: (event: PointerEvent) => show(event.target),
    onPointerLeave: hide,
    onFocus: (event: FocusEvent) => show(event.target),
    onBlur: hide,
  };

  const tooltip = tip ? (
    <span ref={tipRef} className="chart-tip" aria-hidden="true" style={{ left: `${tip.x}px`, top: `${tip.y}px` }}>
      <span className="chart-tip__value">{tip.value}</span>
      {tip.label ? <span className="chart-tip__label">{tip.label}</span> : null}
    </span>
  ) : null;

  return { hostRef, handlers, tooltip };
}

export interface TableCell {
  /** Visible text. */
  text: string;
  /** Read instead of `text` by screen readers (abbreviations, « — »). */
  spoken?: string;
  numeric?: boolean;
  strong?: boolean;
}

interface DataTableProps {
  /** Disclosure label. */
  summary: string;
  caption: string;
  columns: TableCell[];
  /** The first cell of each row is its header. */
  rows: { key: string; cells: TableCell[] }[];
  testId?: string;
}

function CellText({ cell }: { cell: TableCell }) {
  if (!cell.spoken) return <>{cell.text}</>;
  return (
    <>
      <span aria-hidden="true">{cell.text}</span>
      <span className="visually-hidden">{cell.spoken}</span>
    </>
  );
}

const cellClass = (cell: TableCell) =>
  [cell.numeric ? "data-table__num" : "", cell.strong ? "data-table__strong" : ""].filter(Boolean).join(" ") || undefined;

/** The table view of a chart, behind a native disclosure. */
export function DataTable({ summary, caption, columns, rows, testId }: DataTableProps) {
  return (
    <details className="data-table" data-testid={testId}>
      <summary className="data-table__summary">{summary}</summary>
      <div className="data-table__scroll">
        <table>
          <caption className="visually-hidden">{caption}</caption>
          <thead>
            <tr>
              {columns.map((column, index) => (
                <th key={index} scope="col" className={cellClass(column)}>
                  <CellText cell={column} />
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.key}>
                {row.cells.map((cell, index) =>
                  index === 0 ? (
                    <th key={index} scope="row" className={cellClass(cell)}>
                      <CellText cell={cell} />
                    </th>
                  ) : (
                    <td key={index} className={cellClass(cell)}>
                      <CellText cell={cell} />
                    </td>
                  ),
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  );
}
