import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Settings } from "../ipc/contract";
import { createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

async function openStyle(settings: Partial<Settings> = {}) {
  const bridge = createFakeBridge({ settings });
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Style" }));
  await screen.findByRole("heading", { level: 1, name: "Style" });
  return { bridge, ...view };
}

const lastSaved = (bridge: ReturnType<typeof createFakeBridge>) => {
  const saved = bridge.invoke.mock.calls.filter(([command]) => command === "save_settings");
  return (saved[saved.length - 1]?.[1] as { settings: Settings } | undefined)?.settings;
};

describe("Style", () => {
  it("chooses Naturel or Fidèle, each described", async () => {
    const { bridge, user } = await openStyle();
    const natural = screen.getByRole("radio", { name: "Naturel" });
    const faithful = screen.getByRole("radio", { name: "Fidèle" });
    expect(natural).toBeChecked();
    expect(faithful).toHaveAccessibleDescription("La transcription de Rambler telle quelle, ponctuation comprise.");
    await user.click(faithful);
    await waitFor(() => expect(lastSaved(bridge)).toMatchObject({ profile: "faithful" }));
    // Fidèle ignores the three light rules.
    expect(screen.getByRole("switch", { name: "Ponctuation légère" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Espaces typographiques" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Majuscule de phrase" })).toBeDisabled();
    expect(screen.getByText("Ces règles sont ignorées en Fidèle.")).toBeInTheDocument();
  });

  it("switches each light rule", async () => {
    const { bridge, user } = await openStyle();
    await user.click(screen.getByRole("switch", { name: "Espaces typographiques" }));
    await waitFor(() =>
      expect(lastSaved(bridge)?.text).toEqual({ lightPunctuation: true, typographicSpaces: false, sentenceCase: true }),
    );
  });

  it("previews the sample sentence live with preview_render", async () => {
    const bridge = createFakeBridge();
    bridge.handlers.preview_render = ({ sample, settings }: { sample: string; settings: Settings }) =>
      settings.profile === "faithful" ? sample : `${sample.charAt(0).toUpperCase()}${sample.slice(1)}.`;
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Style" }));
    const preview = await screen.findByTestId("render-preview");
    expect(preview).toHaveTextContent(/Reconnu\s*je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient/);
    expect(preview).toHaveTextContent(/Inséré\s*Je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient\./);
    expect(bridge.invoke).toHaveBeenCalledWith("preview_render", {
      sample: "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient",
      settings: expect.objectContaining({ profile: "natural" }),
    });

    const sample = screen.getByRole("textbox", { name: "Phrase telle que Rambler l’écrirait" });
    await user.clear(sample);
    await user.type(sample, "bonjour à tous");
    await waitFor(() => expect(screen.getByTestId("render-preview")).toHaveTextContent(/Inséré\s*Bonjour à tous\./));
    await user.click(screen.getByRole("button", { name: "Revenir à la phrase d’exemple" }));
    expect(sample).toHaveValue("je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient");
  });

  it("hides the preview when it fails", async () => {
    const bridge = createFakeBridge();
    bridge.handlers.preview_render = () => {
      throw { code: "too-long", message: "Phrase d'exemple trop longue." };
    };
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Style" }));
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("preview_render", expect.anything()));
    expect(screen.queryByTestId("render-preview")).not.toBeInTheDocument();
  });

  it("follows the dictation language for the sample", async () => {
    await openStyle({ language: "english" });
    expect(screen.getByRole("textbox", { name: "Phrase telle que Rambler l’écrirait" })).toHaveValue(
      "i will not be able to come on thursday at 2 pm does tuesday work for you",
    );
  });

  it("has no detectable accessibility violations", async () => {
    await openStyle();
    await screen.findByTestId("render-preview");
    expect(await axeViolations()).toEqual([]);
  });
});
