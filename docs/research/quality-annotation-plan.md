# Preparing FR/EN quality acceptance

The cases below are **written reference scenarios**, not a recorded corpus, spontaneous speech, or provider results. They serve to rehearse annotation and prepare an authorized study. Keep results from protocol fixtures, read or synthesized speech, and spontaneous voices separate.

## Dictation: reference and facts to check

In the cases below, quoted text is the invented stimulus. Annotate two references before the trial when they differ: **literal** (words actually spoken, including repetitions) and **cleaned** (intended text, if that mode is under test). Hesitations, repairs, and inserted punctuation are annotated, not inferred after seeing an output.

| ID | Language and scenario | Predeclared reference or invariant |
| --- | --- | --- |
| D01 | FR, « mardi à quinze heures… non, jeudi à quinze heures » | Literal: both days and both times. Cleaned: **jeudi** at **15 h**, without retaining the Tuesday appointment. `quinze heures` is an accepted form variant for the fact. |
| D02 | FR, « c’est très, très important » | **Both** occurrences of `très` are intentional in both modes; `important` alone is insufficient. |
| D03 | FR, « Merci d’envoyer le devis à Maëlle. » | Elision `d’envoyer` required; record `de envoyer` separately. Name `Maëlle`, document `devis`. |
| D04 | FR, « N’envoie pas le dossier avant le 9 mai à 8 h 30. » | Dictation, no action. Negation + boundary `avant`, date `9 mai`, time `08:30`. |
| D05 | FR, « Euh, on se retrouve à Varenne, je veux dire, Varennes-sur-Lac. » | Literal: hesitation and self-correction present. Cleaned: place `Varennes-sur-Lac` only. Test the same stimulus with and without contextual vocabulary (invented place name). |
| D06 | FR, « Il a dit : “On part ?” » | Attribution to `il`, preserve the question; also annotate the quotation/question boundaries, without imposing a quotation-mark style. |
| D07 | FR, « Le total est de douze euros cinquante, pas vingt euros cinquante. » | Amount `12,50 €`; `20,50 €` is explicitly excluded. Do not reduce to one amount in literal mode. |
| D08 | FR, two groups separated by a pause: « Prends les trois clés. [pause] Elles sont dans le tiroir du haut. » | Two clauses, pause without added or lost words, `trois`, `tiroir du haut`. The notation `[pause]` is not expected text. |
| D09 | EN, “Don’t ship box B until June eighteenth, at four thirty.” | Negative + `until`, identifier `B`, June 18 at 4:30; no AM/PM inferred. No host action. |
| D10 | EN, “We need ten—no, eleven—blue labels.” | Literal: both numbers and correction. Clean: `eleven` blue labels, not `ten`. |
| D11 | EN, “No, no, I really meant Lake Mira.” | Both intentional `no`s; `Lake Mira` is a fictional name. |
| D12 | FR→EN mid-sentence, « Je te retrouve after the last train près du hall B. » | Preserve both languages in the same utterance, `last` and `hall B`; do not translate the English passage. Same input with and without secondary language. |
| D13 | EN→FR mid-sentence, “For the sign, write rendez-vous à midi next to the arrow.” | Preserve the French island and `midi`; retain the meaning of `next to`, without automatic action. |

## Editing: independent scope and intent

In this notation, `⟦…⟧` is the initial selection (empty = cursor). The delimiters are **not** part of the document; `L | ` and ` | R` are literal sentinels before/after the target. For a new observation, separately freeze the complete document, exact window boundaries, initial local UTF-16 range, and expected resulting selection if defined. Do not generate the expected document from the proposal. Commands appear here as stimuli, not as indications of the raw intent a provider would return.

| ID | Initial window and instruction | Intent, acceptable text, and boundary |
| --- | --- | --- |
| E01 | `L \| La veste est ⟦bleue⟧. \| R` — « Remplace bleue par verte. » | Replace only the selected word with `verte`, preserving the sentence and both sides. Transcription of the instruction is not an edit. |
| E02 | `L \| ⟦Donne-moi le devis avant jeudi, 15 h.⟧ \| R` — « Rends ça plus poli. » | Polite tone, same document and deadline (`avant jeudi, 15 h`); for example `Pourriez-vous m’envoyer le devis avant jeudi à 15 h ?` or `Merci de me transmettre le devis avant jeudi à 15 h.` No new promise or change outside the selection. |
| E03 | `L \| ⟦It is important to note that the museum closes at 6 p.m. and does not open on Mondays.⟧ \| R` — “Make this shorter.” | Shorten the selection, preserving both closing time and Monday closure. `The museum closes at 6 p.m. and is closed Mondays.` is one acceptable variant. |
| E04 | `L \| Bravo⟦⟧ ! \| R` — « Ajoute l’emoji confettis après Bravo. » | Local insertion of `🎉` after `Bravo`, separated by a space if declared; do not duplicate the whole sentence or change the sentinels. |
| E05 | `L \| Send ⟦a draft copy of ⟧the form. \| R` — “Remove ‘a draft copy of’.” | Deletion within the selection, result `L \| Send the form. \| R`; do not send anything. |
| E06 | After a first dictation: `L \| Départ à ⟦dix heures⟧. \| R` — « Corrige : onze heures. » | History already present; produce `L \| Départ à onze heures. \| R`. If the proposal contains the entire window, do not concatenate it. |
| E07 | `⟦Brouillon⟧` (entire window by assumption), text outside the window unchanged — « Supprime la sélection. » | An empty replacement proposal can constitute an edit (rather than a `no_edit`). Without explicit host authorization, no effect. |
| E08 | `L \| ⟦é🇫🇷👩‍💻⟧ \| R` — test edit, `è🇫🇮👨‍💻` expected | Check the exact Unicode text, UTF-16 selection `[4,15]` in the returned window, and splice boundaries compatible with graphemes. Both accents are decomposed. Change revision, anchor, or document in separate trials: conflict, no change. |
| E09 | `L \| ⟦Draft⟧ \| R` — “Send.” and “Clear all.” (separate trials) | Record transcription, any classification, and any host decision separately. No test authorization to send/clear; document unchanged. |

For safety fixtures, do not annotate identical finals as a single hypothesis, or a response with unknown safety status as a qualified rejected proposal. Mark a preview as provisional and replaceable; an ambiguous final validates neither text nor a mutation. For `Strict`, `AsrOnly`, and `S3ClientCompatible`, label the rendering by its observed provenance. A literal string, even identical to a canonical instruction, is never sufficient to infer a rewrite.

## Rubric to freeze before collection

1. **Identity and pairing.** Preregister ID, language, utterance and reference version, development/holdout split, configuration (including policy, context, and vocabulary), categories and criteria below. For a configuration pair, retain the same input and change only one dimension; annotate recording differences instead of concealing them. Anonymize labels and keep authorized recordings out of the repository.
2. **Literal words.** Align literal output with the literal reference, accent-sensitive but insensitive to case and surface punctuation; set tokenization rules in advance (straight/curly apostrophes equivalent, internal apostrophe retained in the word; no merging of spelled-out and digit numbers for the primary WER). Hesitations and repetitions actually spoken remain in the reference. Publish `S`, `D`, `I`, `N`, and `(S+D+I)/N` by stratum and overall for evaluable cases; `N` = reference words. Publish the non-evaluable count separately. Do not apply this WER to a free rewrite or present it as a cleaned-content score.
3. **Cleaned text and features.** Preannotate expected sentence boundaries and marks, entities/spellings, numerical facts (value, unit, role), negations and their scope, intended repetitions (count), and repairs (retained/rejected value). A form variant can preserve a fact without passing literal WER. Record missing/added punctuation, number or negation errors, lost/duplicated repetition, repair residue, preserved language switching, and name/elision errors. A label can apply to multiple categories.
4. **Editing.** Independently score **intent understood** (action/target matching the request), **acceptable text** (facts preserved and instruction followed, accepted variants), and **scope** (window ready to substitute, sentinels and outside content preserved, correct selection/range). Label each axis `pass`, `error`, or `not assessable`. Use `not applicable` when the protocol specifies in advance that the edit will not be applied. Separately report the proposal's provenance and safety status, anchor/revision validation result, and any actually observed host effect. A good rephrasing in the wrong place is a scope failure. The [exact checker](edit-acceptance-contract.md) does not replace this annotation: it compares one expected window without judging semantic variants.
5. **Publication.** For each category, give the planned, evaluable, passed/failed, and non-evaluable counts and an authorized explanatory example, without filling in missing data. Have at least editing cases and ambiguous repairs independently annotated, blind candidate names if possible, and record disagreement resolution before aggregation. Report the breakdown by language, conditions, and mode without merging `Strict` and `S3ClientCompatible` results.

## Awaiting real references

The proposed batch of **40 spontaneous utterances** has not been collected. Proposed stratification: 28 FR, 8 EN, 4 mixed; hold out 7 FR, 2 EN, 1 mixed, with both utterances **and speakers** distinct from the development batch. Use the themes above as a guide, without turning a reading of these sentences into “spontaneous” speech. Once authorization and recruitment are in place, vary accents, rate, noise, and silences, and record conditions, consent, and retention. Actual error rates, the effect of vocabulary or secondary language, and actual behavior for unestablished intents remain to be measured.

Comparisons also **pending**: Rust and PixelBoard 1.3.5 on the Xiaomi (same recorded versions/configurations, initial text, selection, and utterance paired with documented capture differences), and official Gboard on eligible hardware. No service, microphone, or phone trial or reference result is required to reproduce offline acceptance tests.
