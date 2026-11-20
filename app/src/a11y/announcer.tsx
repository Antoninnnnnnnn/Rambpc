// One polite live region for the whole window. Only `Announcement` values
// reach it: fixed French copy built from text-free state, never dictated text.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

declare const announcementBrand: unique symbol;

/** French status text that is safe to speak: never contains dictated words. */
export type Announcement = string & { readonly [announcementBrand]: "announcement" };

/** Wrap fixed copy (labels/fr.ts) as an announcement. Never pass draft text. */
export function announcement(text: string): Announcement {
  return text as Announcement;
}

type Announce = (message: Announcement) => void;

const AnnounceContext = createContext<Announce>(() => {});

export function useAnnounce(): Announce {
  return useContext(AnnounceContext);
}

export function AnnouncerProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState("");
  const timer = useRef<number | undefined>(undefined);

  const announce = useCallback<Announce>((next) => {
    // Clear first so that the same message spoken twice is announced twice.
    setMessage("");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setMessage(next), 60);
  }, []);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <AnnounceContext.Provider value={announce}>
      {children}
      <div className="visually-hidden" role="status" aria-live="polite" aria-atomic="true" data-testid="live-region">
        {message}
      </div>
    </AnnounceContext.Provider>
  );
}
