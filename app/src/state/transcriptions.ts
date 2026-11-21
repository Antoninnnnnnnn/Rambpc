// Audio file transcriptions of this session (milestone 5): `list_transcriptions`
// once, then `sambler://transcription` events. Text-free: a finished
// transcription is a Carnet note, read only by the Carnet (`get_note`).
//
// One store per bridge, shared by the Carnet and the window-wide drop offer,
// so that a transcription started from either is followed by both.

import { useMemo, useSyncExternalStore } from "react";
import { toCommandError, useBridge, type Bridge } from "../ipc/bridge";
import { EVENT_TRANSCRIPTION, type FileTranscription } from "../ipc/contract";
import { listenShared } from "../ipc/shared";

export interface TranscriptionsSnapshot {
  /** This session's transcriptions, newest first. */
  jobs: FileTranscription[];
  /** `list_transcriptions` answered (or failed). */
  loaded: boolean;
  /** The native side predates file transcription. */
  unsupported: boolean;
  /** Finished transcriptions the person closed (their card stays hidden). */
  dismissed: ReadonlySet<string>;
}

function newest(a: FileTranscription, b: FileTranscription): number {
  return b.startedAt - a.startedAt;
}

class TranscriptionStore {
  private snapshot: TranscriptionsSnapshot = { jobs: [], loaded: false, unsupported: false, dismissed: new Set() };
  private listeners = new Set<() => void>();
  private stopEvents: (() => void) | null = null;
  /** Jobs updated by an event since the last `list_transcriptions` call. */
  private fresh = new Set<string>();

  constructor(private readonly bridge: Bridge) {}

  getSnapshot = () => this.snapshot;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    if (!this.stopEvents) this.follow();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0 && this.stopEvents) {
        this.stopEvents();
        this.stopEvents = null;
      }
    };
  };

  private set(patch: Partial<TranscriptionsSnapshot>) {
    this.snapshot = { ...this.snapshot, ...patch };
    this.listeners.forEach((listener) => listener());
  }

  /** Listen first, then ask: an event that overtakes the answer wins. */
  private follow() {
    this.fresh = new Set();
    this.stopEvents = listenShared(this.bridge, EVENT_TRANSCRIPTION, (job) => {
      this.fresh.add(job.id);
      this.upsert(job);
    });
    this.bridge
      .invoke("list_transcriptions")
      .then((listed) => {
        const byId = new Map(this.snapshot.jobs.map((job) => [job.id, job]));
        for (const job of listed) if (!this.fresh.has(job.id)) byId.set(job.id, job);
        this.set({ jobs: [...byId.values()].sort(newest), loaded: true, unsupported: false });
      })
      .catch((raw) => {
        this.set({ loaded: true, unsupported: toCommandError(raw).code === "unknown" });
      });
  }

  upsert(job: FileTranscription) {
    const others = this.snapshot.jobs.filter((each) => each.id !== job.id);
    this.set({ jobs: [job, ...others].sort(newest) });
  }

  async start(path: string): Promise<FileTranscription> {
    const job = await this.bridge.invoke("transcribe_file", { path });
    // An event may already have brought this job, in a later state.
    if (!this.snapshot.jobs.some((each) => each.id === job.id)) this.upsert(job);
    return job;
  }

  /** The native file dialog, then the transcription; null when the person cancels the dialog. */
  async pickAndStart(): Promise<FileTranscription | null> {
    const path = await this.bridge.invoke("pick_audio_file");
    if (path === null) return null;
    return this.start(path);
  }

  async cancel(id: string): Promise<FileTranscription> {
    const job = await this.bridge.invoke("cancel_transcription", { id });
    this.upsert(job);
    return job;
  }

  dismiss(id: string) {
    const dismissed = new Set(this.snapshot.dismissed);
    dismissed.add(id);
    this.set({ dismissed });
  }
}

const stores = new WeakMap<Bridge, TranscriptionStore>();

function storeFor(bridge: Bridge): TranscriptionStore {
  let store = stores.get(bridge);
  if (!store) {
    store = new TranscriptionStore(bridge);
    stores.set(bridge, store);
  }
  return store;
}

export interface Transcriptions extends TranscriptionsSnapshot {
  /** The transcription in progress, if any (one at a time). */
  running: FileTranscription | null;
  /** Rejects with a `CommandError`. */
  start: (path: string) => Promise<FileTranscription>;
  pickAndStart: () => Promise<FileTranscription | null>;
  cancel: (id: string) => Promise<FileTranscription>;
  dismiss: (id: string) => void;
}

export function useTranscriptions(): Transcriptions {
  const bridge = useBridge();
  const store = storeFor(bridge);
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return useMemo(
    () => ({
      ...snapshot,
      running: snapshot.jobs.find((job) => job.status === "running") ?? null,
      start: (path: string) => store.start(path),
      pickAndStart: () => store.pickAndStart(),
      cancel: (id: string) => store.cancel(id),
      dismiss: (id: string) => store.dismiss(id),
    }),
    [snapshot, store],
  );
}
