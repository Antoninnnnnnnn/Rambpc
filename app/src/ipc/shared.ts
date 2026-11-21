// One native subscription per event, shared by every component that follows
// it. The voice level (`sambler://level`, 20–30 Hz while listening) feeds the
// voice bars and the theme's backdrop at once: a single IPC listener fans the
// level out to all of them instead of one listener each.

import type { Bridge, Unlisten } from "./bridge";
import type { EventMap, EventName } from "./contract";

type Handler = (payload: never) => void;

interface Channel {
  handlers: Set<Handler>;
  /** The pending or active native listener; null while nobody follows. */
  stop: Promise<Unlisten | null> | null;
}

const channels = new WeakMap<Bridge, Map<EventName, Channel>>();

/** Follow `event` through the shared listener. Returns the unsubscribe function. */
export function listenShared<E extends EventName>(
  bridge: Bridge,
  event: E,
  handler: (payload: EventMap[E]) => void,
): () => void {
  let byEvent = channels.get(bridge);
  if (!byEvent) {
    byEvent = new Map();
    channels.set(bridge, byEvent);
  }
  let channel = byEvent.get(event);
  if (!channel) {
    channel = { handlers: new Set(), stop: null };
    byEvent.set(event, channel);
  }
  const current = channel;
  current.handlers.add(handler as Handler);
  if (!current.stop) {
    current.stop = bridge
      .listen(event, (payload) => {
        for (const each of [...current.handlers]) (each as (value: EventMap[E]) => void)(payload);
      })
      .catch(() => null);
  }

  let done = false;
  return () => {
    if (done) return;
    done = true;
    current.handlers.delete(handler as Handler);
    if (current.handlers.size === 0 && current.stop) {
      const pending = current.stop;
      current.stop = null;
      void pending.then((stop) => stop?.());
    }
  };
}
