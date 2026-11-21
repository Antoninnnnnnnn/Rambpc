import { describe, expect, it } from "vitest";
import type { TakeOutcome, UiState } from "../ipc/contract";
import { uiState } from "../test/fakeBridge";
import {
  clock,
  expire,
  FLASH_MS,
  flashFor,
  HIDDEN,
  nextView,
  outcomeMeaning,
  type OutcomeMeaning,
  type OverlayView,
} from "./model";

const idle = (patch: Partial<UiState> = {}) => uiState(patch);
const listening = (take: number, listeningMs = 0, patch: Partial<UiState> = {}) =>
  uiState({ phase: { kind: "listening" }, take, listeningMs, destination: "field", ...patch });

describe("overlay state mapping", () => {
  it("shows nothing for the first, idle state, even with an old outcome", () => {
    expect(nextView(HIDDEN, null, idle({ lastOutcome: { kind: "inserted" } }), 0)).toEqual(HIDDEN);
  });

  it("listens from the first event of a take, timing from listeningMs", () => {
    const view = nextView(HIDDEN, idle(), listening(4, 1500), 10_000);
    expect(view).toEqual({ kind: "listening", take: 4, startedAt: 8_500, toSambler: false });
    // Later events of the same take keep the start time.
    expect(nextView(view, listening(4, 1500), listening(4, 9000), 20_000)).toEqual(view);
    // A new take restarts it.
    expect(nextView(view, listening(4), listening(5, 0, { destination: "sambler" }), 30_000)).toEqual({
      kind: "listening",
      take: 5,
      startedAt: 30_000,
      toSambler: true,
    });
  });

  it("maps preparing and finalizing", () => {
    expect(nextView(HIDDEN, idle(), uiState({ phase: { kind: "preparing" }, take: 1 }), 0)).toEqual({ kind: "preparing" });
    expect(nextView(HIDDEN, listening(1), uiState({ phase: { kind: "finalizing" }, take: 1 }), 0)).toEqual({
      kind: "finalizing",
    });
  });

  it("flashes the outcome when a take ends, then hides after 2.5 s", () => {
    const finalizing = uiState({ phase: { kind: "finalizing" }, take: 1 });
    const view = nextView({ kind: "finalizing" }, finalizing, idle({ lastOutcome: { kind: "inserted" } }), 1000);
    expect(view).toEqual({
      kind: "outcome",
      flash: { tone: "success", icon: "check", label: "Inséré" },
      until: 1000 + FLASH_MS,
    });
    expect(expire(view, 1000 + FLASH_MS - 1)).toBe(view);
    expect(expire(view, 1000 + FLASH_MS)).toEqual(HIDDEN);
  });

  it("flashes the same outcome twice in a row (two takes)", () => {
    const outcome: TakeOutcome = { kind: "inserted" };
    let view: OverlayView = HIDDEN;
    view = nextView(view, idle({ lastOutcome: outcome }), listening(2), 0);
    view = nextView(view, listening(2), idle({ lastOutcome: outcome }), 100);
    expect(view.kind).toBe("outcome");
  });

  it("flashes a refusal that never started a take, but not unrelated changes", () => {
    const before = idle();
    const refused = idle({ lastOutcome: { kind: "refused", reason: { kind: "protected-field" } } });
    const view = nextView(HIDDEN, before, refused, 0);
    expect(view).toMatchObject({ kind: "outcome", flash: { label: "Champ protégé", icon: "lock" } });
    // An availability change while idle keeps what is shown.
    const unavailable = { ...refused, availability: { kind: "unavailable" as const, reason: "network" as const } };
    expect(nextView(view, refused, unavailable, 100)).toBe(view);
    expect(nextView(HIDDEN, refused, unavailable, 100)).toEqual(HIDDEN);
  });

  it.each<[string, TakeOutcome, string, string]>([
    ["inserted", { kind: "inserted" }, "Inséré", "success"],
    ["inserted-partially", { kind: "inserted-partially" }, "Vérifiez le champ", "warning"],
    ["kept / uncertain target", { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } }, "Texte dans Sambler", "info"],
    ["kept / destination", { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } }, "Texte dans Sambler", "info"],
    ["no-text", { kind: "no-text" }, "Aucun texte", "neutral"],
    ["cancelled", { kind: "cancelled" }, "Annulé", "neutral"],
    ["interrupted, recovered", { kind: "interrupted", reason: { kind: "network" }, recovered: true }, "Interrompue, texte gardé", "warning"],
    ["interrupted, lost", { kind: "interrupted", reason: { kind: "timeout" }, recovered: false }, "Dictée interrompue", "error"],
    ["interrupted, duration", { kind: "interrupted", reason: { kind: "duration-limit" }, recovered: true }, "Durée maximale atteinte", "info"],
    ["interrupted, mic denied", { kind: "interrupted", reason: { kind: "microphone", error: { kind: "access-denied" } }, recovered: false }, "Micro bloqué par Windows", "error"],
    ["unavailable", { kind: "refused", reason: { kind: "unavailable", reason: "network" } }, "Indisponible", "error"],
    ["needs consent", { kind: "refused", reason: { kind: "needs-consent" } }, "Dictée non activée", "warning"],
    ["needs access", { kind: "refused", reason: { kind: "needs-access" } }, "Accès à Rambler requis", "warning"],
    ["protected", { kind: "refused", reason: { kind: "protected-field" } }, "Champ protégé", "warning"],
    ["uncertain (legacy)", { kind: "refused", reason: { kind: "uncertain-target", reason: "elevated" } }, "Champ non pris en charge", "warning"],
    ["mic not found", { kind: "refused", reason: { kind: "microphone", error: { kind: "not-found" } } }, "Micro introuvable", "error"],
    ["no mic", { kind: "refused", reason: { kind: "microphone", error: { kind: "no-input" } } }, "Aucun micro", "error"],
    ["mic timeout", { kind: "refused", reason: { kind: "microphone", error: { kind: "open-timeout" } } }, "Micro indisponible", "error"],
  ])("labels %s in a few words", (_name, outcome, label, tone) => {
    const flash = flashFor(outcome);
    expect(flash.label).toBe(label);
    expect(flash.tone).toBe(tone);
    // Fits the pill of the 360 px window.
    expect(flash.label.length).toBeLessThanOrEqual(26);
  });

  it.each<[string, TakeOutcome, OutcomeMeaning]>([
    ["inserted", { kind: "inserted" }, "inserted"],
    ["inserted-partially", { kind: "inserted-partially" }, "kept"],
    ["kept", { kind: "kept-in-sambler", reason: { kind: "incomplete" } }, "kept"],
    ["no-text", { kind: "no-text" }, "neutral"],
    ["cancelled", { kind: "cancelled" }, "neutral"],
    ["interrupted, recovered", { kind: "interrupted", reason: { kind: "network" }, recovered: true }, "kept"],
    ["interrupted, lost", { kind: "interrupted", reason: { kind: "timeout" }, recovered: false }, "error"],
    ["duration, recovered", { kind: "interrupted", reason: { kind: "duration-limit" }, recovered: true }, "kept"],
    ["duration, lost", { kind: "interrupted", reason: { kind: "duration-limit" }, recovered: false }, "error"],
    ["mic denied mid-take", { kind: "interrupted", reason: { kind: "microphone", error: { kind: "access-denied" } }, recovered: true }, "error"],
    ["needs consent", { kind: "refused", reason: { kind: "needs-consent" } }, "refused"],
    ["protected", { kind: "refused", reason: { kind: "protected-field" } }, "refused"],
    ["pending draft (legacy)", { kind: "refused", reason: { kind: "pending-draft" } }, "refused"],
    ["unavailable", { kind: "refused", reason: { kind: "unavailable", reason: "network" } }, "error"],
    ["no mic", { kind: "refused", reason: { kind: "microphone", error: { kind: "no-input" } } }, "error"],
  ])("tells what %s means for the text", (_name, outcome, meaning) => {
    expect(outcomeMeaning(outcome)).toBe(meaning);
  });

  it("formats the clock", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(7_400)).toBe("0:07");
    expect(clock(271_000)).toBe("4:31");
  });
});
