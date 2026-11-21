// Putting a dictated result into a note at the cursor.
import { describe, expect, it } from "vitest";
import { completeDraft, draftWithGaps } from "../../test/fakeBridge";
import { dictatedText, insertAtSelection, tidyDictation } from "./insertText";
import { editedText, formatDuration, relativeDate, searchKey } from "./labels";

describe("insertAtSelection", () => {
  it("adds the spaces a person would type between words", () => {
    expect(insertAtSelection("Bonjour monde", 7, 7, "cher")).toEqual({ text: "Bonjour cher monde", caret: 12 });
    expect(insertAtSelection("Bonjour ", 8, 8, "à tous.")).toEqual({ text: "Bonjour à tous.", caret: 15 });
    expect(insertAtSelection("Merci", 5, 5, "beaucoup")).toEqual({ text: "Merci beaucoup", caret: 14 });
  });

  it("adds no space at the start of a line or before punctuation", () => {
    expect(insertAtSelection("", 0, 0, "  Première ligne  ").text).toBe("Première ligne");
    expect(insertAtSelection("Titre\n", 6, 6, "Suite").text).toBe("Titre\nSuite");
    expect(insertAtSelection("Vous venez ?", 10, 10, "demain").text).toBe("Vous venez demain ?");
    expect(insertAtSelection("Fin.", 3, 3, "al").text).toBe("Fin al.");
  });

  it("replaces the selection", () => {
    expect(insertAtSelection("Rendez-vous jeudi à 14 h", 12, 17, "vendredi")).toEqual({
      text: "Rendez-vous vendredi à 14 h",
      caret: 20,
    });
  });

  it("keeps a list on its own lines and turns Rambler's bullets into « • »", () => {
    const raw = "Voici la liste :\n\n*   pain\n*   lait   \n\n\n*   citrons";
    expect(tidyDictation(raw)).toBe("Voici la liste :\n\n• pain\n• lait\n\n• citrons");
    // A result that opens with line breaks after a sentence keeps them.
    expect(insertAtSelection("Courses :", 9, 9, "\n\n*   pain\n*   lait").text).toBe("Courses :\n\n• pain\n• lait");
    // At the start of a line, the leading breaks go.
    expect(insertAtSelection("", 0, 0, "\n\n*   pain").text).toBe("• pain");
  });

  it("changes nothing for an empty result", () => {
    expect(insertAtSelection("Texte", 2, 2, "  \n ")).toEqual({ text: "Texte", caret: 2 });
  });
});

describe("dictatedText", () => {
  it("uses the full text of a complete result", () => {
    expect(dictatedText(completeDraft("Bonjour à tous."))).toEqual({ text: "Bonjour à tous.", gaps: false });
  });

  it("marks the passages an incomplete result lost", () => {
    const { text, gaps } = dictatedText(draftWithGaps());
    expect(gaps).toBe(true);
    expect(text).toBe(
      "Je ne pourrai pas venir jeudi, [Passage non transcrit] mais vendredi me convient. vers dix heures [Passage non transcrit]",
    );
  });
});

describe("Carnet labels", () => {
  const now = new Date(2026, 10, 21, 15, 0).getTime();

  it("says when a note changed, in French", () => {
    expect(relativeDate(now - 20_000, now)).toBe("À l’instant");
    expect(relativeDate(now - 5 * 60_000, now)).toBe("Il y a 5 min");
    expect(relativeDate(now - 3 * 3_600_000, now)).toBe("Il y a 3 h");
    expect(relativeDate(new Date(2026, 10, 20, 9, 5).getTime(), now)).toBe("Hier, 09:05");
    expect(relativeDate(new Date(2026, 10, 17, 9, 5).getTime(), now)).toBe("Mardi");
    expect(relativeDate(new Date(2026, 9, 2, 9, 5).getTime(), now)).toBe("2 oct.");
    expect(relativeDate(new Date(2025, 9, 2, 9, 5).getTime(), now)).toBe("2 oct. 2025");
    expect(editedText(new Date(2026, 10, 20, 9, 5).getTime(), now)).toBe("Modifiée hier à 09:05");
  });

  it("formats durations and search keys", () => {
    expect(formatDuration(42_000)).toBe("42 s");
    expect(formatDuration(7 * 60_000 + 42_000)).toBe("7 min 42 s");
    expect(formatDuration(65 * 60_000)).toBe("1 h 05 min");
    expect(searchKey("Idées ÉTÉ")).toBe("idees ete");
  });
});
