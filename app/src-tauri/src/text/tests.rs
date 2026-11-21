use super::*;

const NNBSP: &str = "\u{202f}";
const NBSP: &str = "\u{a0}";

fn natural(language: Language) -> TextRules {
    TextRules {
        language,
        ..TextRules::default()
    }
}

fn options(light_punctuation: bool, typographic_spaces: bool, sentence_case: bool) -> TextOptions {
    TextOptions {
        light_punctuation,
        typographic_spaces,
        sentence_case,
        voice_commands: false,
    }
}

fn entry(heard: &str, written: &str) -> LexiconEntry {
    LexiconEntry {
        id: heard.into(),
        heard: heard.into(),
        written: written.into(),
        enabled: true,
        apply_in_faithful: false,
    }
}

fn expression(trigger: &str, text: &str) -> Expression {
    Expression {
        id: trigger.into(),
        trigger: trigger.into(),
        text: text.into(),
        enabled: true,
    }
}

#[test]
fn defaults_and_rule_snapshot_match_settings() {
    assert_eq!(
        TextRules::from_settings(&Settings::default()),
        TextRules::default()
    );
    let mut settings = Settings {
        language: Language::English,
        profile: RenderProfile::Faithful,
        text: options(false, true, false),
        ..Settings::default()
    };
    let rules = TextRules::from_settings(&settings);
    settings.text.typographic_spaces = false;
    assert_eq!(rules.language, Language::English);
    assert_eq!(rules.profile, RenderProfile::Faithful);
    assert_eq!(rules.options, options(false, true, false));
    assert!(rules.lexicon.is_empty() && rules.expressions.is_empty());
    assert!(!settings.text.typographic_spaces);
}

#[test]
fn faithful_preserves_admitted_bytes_with_all_switches_on() {
    let rules = TextRules {
        profile: RenderProfile::Faithful,
        expressions: vec![expression("signature", "Replacement")],
        lexicon: vec![entry("mardi", "jeudi")],
        ..TextRules::default()
    };
    let sample = "\t  signature ! mardi,,  mardi  \r\n\n3. 14 👩🏽‍💻  ";
    assert_eq!(render(sample, &rules), sample);
    assert_eq!(render("", &rules), "");
}

#[test]
fn faithful_lexicon_has_a_separate_opt_in() {
    let mut yes = entry("projet Mélusine", "pRoJet\nMÉLUSINE !");
    yes.apply_in_faithful = true;
    let mut disabled = entry("oui", "non");
    disabled.enabled = false;
    disabled.apply_in_faithful = true;
    let rules = TextRules {
        profile: RenderProfile::Faithful,
        lexicon: vec![yes, disabled, entry("bonjour", "au revoir")],
        ..TextRules::default()
    };
    assert_eq!(
        render("bonjour  PROJET MÉLUSINE ! oui", &rules),
        "bonjour  pRoJet\nMÉLUSINE ! ! oui"
    );
}

#[test]
fn french_typography_only() {
    let rules = TextRules {
        options: options(false, true, false),
        ..TextRules::default()
    };
    assert_eq!(
        render("salut  ; oui  : « vite  ! vraiment ? » , alors .", &rules),
        format!("salut{NNBSP}; oui{NBSP}: «{NNBSP}vite{NNBSP}! vraiment{NNBSP}?{NNBSP}», alors.")
    );
    assert_eq!(
        render("oui :non,,d'accord", &rules),
        format!("oui{NBSP}:non,,d'accord")
    );
}

#[test]
fn english_typography_only() {
    let rules = TextRules {
        options: options(false, true, false),
        ..natural(Language::English)
    };
    assert_eq!(
        render("yes  ,  now . good ; here : really ! what ?", &rules),
        "yes, now. good; here: really! what?"
    );
    assert_eq!(render("yes ,now", &rules), "yes,now");
}

#[test]
fn light_punctuation_only_repairs_clear_prose_artifacts() {
    let rules = TextRules {
        options: options(true, false, false),
        ..TextRules::default()
    };
    assert_eq!(
        render(
            "bonjour,,monsieur . encore!vraiment?oui;merci:demain",
            &rules
        ),
        "bonjour, monsieur. encore! vraiment? oui; merci: demain"
    );
    assert_eq!(
        render("oui  ; non?? sérieux... parfait?!", &rules),
        "oui  ; non?? sérieux... parfait?!"
    );
}

#[test]
fn sentence_case_only_preserves_the_existing_case_and_spacing() {
    let rules = TextRules {
        options: options(false, false, true),
        ..TextRules::default()
    };
    assert_eq!(
        render("  «été  . oui!ça?déjà. iPhone et SNCF», encore", &rules),
        "  «Été  . Oui!Ça?Déjà. iPhone et SNCF», encore"
    );
    assert_eq!(render("e\u{301}té", &rules), "E\u{301}té");
}

#[test]
fn all_prose_options_off() {
    let rules = TextRules {
        options: options(false, false, false),
        ..TextRules::default()
    };
    let sample = "l'été  ! oui,,merci .\n\nà 9 h 05";
    assert_eq!(render(sample, &rules), sample);
}

#[test]
fn french_documentation_examples_preserve_meaning() {
    for (input, expected) in [
        ("rendez-vous jeudi à 14 h", "Rendez-vous jeudi à 14 h"),
        (
            "je ne peux pas mardi, mais jeudi oui",
            "Je ne peux pas mardi, mais jeudi oui",
        ),
        (
            "je peux mardi, pardon jeudi à 14 h",
            "Je peux mardi, pardon jeudi à 14 h",
        ),
        ("l'été à 9 h 05", "L'été à 9 h 05"),
        ("l’été à 9 h 05", "L’été à 9 h 05"),
        (
            "c'est très très important pour Mme Dupont",
            "C'est très très important pour Mme Dupont",
        ),
    ] {
        assert_eq!(render(input, &TextRules::default()), expected);
    }
}

#[test]
fn english_samples_keep_negation_repetition_and_no_added_period() {
    let rules = natural(Language::English);
    assert_eq!(
        render("i don't want Tuesday, but Thursday yes", &rules),
        "I don't want Tuesday, but Thursday yes"
    );
    assert_eq!(
        render("it was very very quiet", &rules),
        "It was very very quiet"
    );
    assert_eq!(
        render(
            "There's one and there's another: the Dudley and the Flint.",
            &rules
        ),
        "There's one and there's another: the Dudley and the Flint."
    );
}

#[test]
fn dates_times_and_amounts_survive_even_if_spacing_is_unusual() {
    let rules = TextRules::default();
    for sample in [
        "3,14",
        "3.14",
        "14 h",
        "14:30",
        "1  000",
        "1\u{202f}000",
        "1\u{a0}000",
        "12,50  €",
        "€  1  000,50",
        "-12,50 €",
        "05/11/2026",
        "2026-11-20",
        "9  h  05",
        "14 : 30",
        "20  %",
        "3,  14",
        "3,,14",
        "1.2.3",
    ] {
        let input = format!("total : {sample} !");
        let expected = format!("Total{NBSP}: {sample}{NNBSP}!");
        assert_eq!(render(&input, &rules), expected, "{sample}");
    }
    for sample in ["$  1,005.50", "3 p.m.", "3:45 pm", "12:00 AM", "USD  15.00"] {
        let input = format!("at {sample} today");
        let expected = format!("At {sample} today");
        assert_eq!(
            render(&input, &natural(Language::English)),
            expected,
            "{sample}"
        );
    }
}

#[test]
fn ambiguous_numeric_seams_from_the_sdk_are_not_reinterpreted() {
    let joined = rambler_audio::join_segment_texts(["3.", "14"]);
    assert_eq!(joined, "3. 14");
    for language in [Language::French, Language::English] {
        let rules = natural(language);
        assert_eq!(render(&joined, &rules), joined);
        assert_eq!(
            render("c'est 3. 14 en tout. merci", &rules),
            "C'est 3. 14 en tout. Merci"
        );
        assert_eq!(
            render("3... 14 ou 3 . . . 14", &rules),
            "3... 14 ou 3 . . . 14"
        );
    }
}

#[test]
fn sentence_case_respects_abbreviations_ellipses_and_technical_case() {
    let rules = natural(Language::English);
    assert_eq!(
        render(
            "dr. watson met J. smith in the U.S. capital. then wrote it down",
            &rules
        ),
        "Dr. watson met J. smith in the U.S. capital. Then wrote it down"
    );
    assert_eq!(
        render("wait... please wait… yes. now!go", &rules),
        "Wait... please wait… yes. Now! Go"
    );
    assert_eq!(
        render("iPhone is ready. iOS works. déjà prêt", &rules),
        "iPhone is ready. iOS works. Déjà prêt"
    );
}

#[test]
fn technical_spans_are_opaque_in_both_languages() {
    let samples = [
        "https://example.test/a?x=1,2&y=oui!",
        "http://example.test/abc#one:two",
        "www.example.test",
        "example.test:8080?key=value",
        "localhost:3000",
        "mailto:name+label@example.fr",
        "name+label@example.fr",
        "main.rs",
        "m.rs",
        "myFileName.txt",
        "2.schema.json",
        ".gitignore",
        "src/main.rs",
        r#""c:\Program  Files\main.rs""#,
        "C:\\x",
        "c:\\users\\foo.txt",
        "C:\\",
        "\\\\server\\share\\main.rs",
        "./main.rs",
        "../deps/main.rs",
        "/usr/bin/env",
        "foo::bar",
        "--dry-run",
        "snake_case",
        "foo_bar()",
        "foo++",
        "x==y",
        "a+b",
        "`if  no? yes,,x`",
        "```\nfoo  ? bar,,baz\n```",
        "👩🏽‍💻",
        "🇫🇷",
        "1️⃣",
        "!\u{fe0f}",
    ];
    for language in [Language::French, Language::English] {
        let rules = natural(language);
        for sample in samples {
            assert_eq!(render(sample, &rules), sample, "{sample}");
        }
    }
    assert_eq!(
        render("voir src/main.rs , merci !", &TextRules::default()),
        format!("Voir src/main.rs, merci{NNBSP}!")
    );
    assert_eq!(
        render("👩🏽‍💻 bonjour?oui", &TextRules::default()),
        format!("👩🏽‍💻 Bonjour{NNBSP}? Oui")
    );
}

#[test]
fn blank_lines_and_indent_are_preserved() {
    let rules = TextRules::default();
    assert_eq!(
        render("bonjour.\r\n\r\n  merci\rà bientôt\n", &rules),
        "Bonjour.\n\n  Merci\nà bientôt\n"
    );
    assert_eq!(render("\n\n", &rules), "\n\n");
}

#[test]
fn lexicon_uses_longest_match_after_the_join() {
    let rules = TextRules {
        lexicon: vec![
            entry("projet", "plan"),
            entry("projet mélusine", "projet Mélusine"),
        ],
        ..TextRules::default()
    };
    let sample = rambler_audio::join_segment_texts(["je lance le projet", "MÉLUSINE et un projet"]);
    assert_eq!(
        render(&sample, &rules),
        "Je lance le projet Mélusine et un plan"
    );
    assert_eq!(render("projet\u{a0}   mélusine", &rules), "projet Mélusine");
}

#[test]
fn lexicon_is_caseless_accent_sensitive_and_respects_word_boundaries() {
    let rules = TextRules {
        options: options(false, false, false),
        lexicon: vec![
            entry("été", "SUMMER"),
            entry("note", "MEMO"),
            entry("straße", "STREET"),
            entry("Σ", "sigma"),
        ],
        ..TextRules::default()
    };
    assert_eq!(
        render(
            "été ÉTÉ e\u{301}te\u{301} ete étéiste superété l’été notebook note noté STRASSE ς",
            &rules
        ),
        "SUMMER SUMMER SUMMER ete étéiste superété l’été notebook MEMO noté STREET sigma"
    );
}

#[test]
fn lexicon_equates_apostrophe_styles_without_matching_a_word_fragment() {
    let rules = TextRules {
        options: options(false, false, false),
        lexicon: vec![entry("l'été", "la saison"), entry("john’s", "his")],
        ..TextRules::default()
    };
    assert_eq!(
        render("L’ÉTÉ l'été lʼété l'ete l'étéiste John's Johnson's", &rules),
        "la saison la saison la saison l'ete l'étéiste his Johnson's"
    );
}

#[test]
fn lexicon_keeps_punctuation_significant() {
    let rules = TextRules {
        lexicon: vec![entry("hello, world", "one")],
        ..natural(Language::English)
    };
    assert_eq!(
        render("hello world; hello, world", &rules),
        "Hello world; one"
    );
}

#[test]
fn each_lexicon_occurrence_is_replaced_once_and_written_is_literal() {
    let mut disabled = entry("d", "never");
    disabled.enabled = false;
    let rules = TextRules {
        lexicon: vec![
            entry("a", "b"),
            entry("b", "c"),
            entry("x", "lower  !  Hi,,"),
            disabled,
        ],
        ..TextRules::default()
    };
    assert_eq!(render("a b a x d", &rules), "b c b lower  !  Hi,, d");
    assert_eq!(render("x", &rules), "lower  !  Hi,,");
}

#[test]
fn standalone_expression_expands_to_the_literal_multiline_signature() {
    let rules = TextRules {
        expressions: vec![expression(
            "insère ma formule courte",
            "bien à vous !\r\nÉlodie  MARTIN\n\nQA,,",
        )],
        lexicon: vec![entry("Élodie", "changed")],
        ..TextRules::default()
    };
    assert_eq!(
        render("« INSÈRE, MA formule-courte ! »", &rules),
        "bien à vous !\nÉlodie  MARTIN\n\nQA,,"
    );
    assert_eq!(
        render("insere ma formule courte", &rules),
        "Insere ma formule courte"
    );
}

#[test]
fn expression_matches_inside_a_sentence_and_keeps_surrounding_punctuation() {
    let rules = TextRules {
        expressions: vec![expression("add greeting", "hello  there\nfrom Jo !")],
        ..natural(Language::English)
    };
    assert_eq!(
        render("note: add—greeting. now go!", &rules),
        "Note: hello  there\nfrom Jo !. Now go!"
    );
    assert_eq!(render("preadd greetinging", &rules), "Preadd greetinging");
}

#[test]
fn expression_uses_word_boundaries_and_apostrophe_variants() {
    let rules = TextRules {
        expressions: vec![expression("l’été revient", "OK")],
        options: options(false, false, false),
        ..TextRules::default()
    };
    assert_eq!(render("l'ÉTÉ… revient", &rules), "OK");
    assert_eq!(render("l’ete revient", &rules), "l’ete revient");
    assert_eq!(render("l’été revenait", &rules), "l’été revenait");
    assert_eq!(render("l’été ☀️ revient", &rules), "l’été ☀️ revient");
}

#[test]
fn expressions_win_over_lexicon_and_neither_stage_rescans_payloads() {
    let mut off = expression("off", "should not render");
    off.enabled = false;
    let rules = TextRules {
        expressions: vec![
            expression("short", "loser"),
            expression("short signature", "second"),
            expression("second", "third"),
            off,
        ],
        lexicon: vec![
            entry("short signature", "lexicon loss"),
            entry("second", "casing loss"),
            entry("alpha", "short signature"),
        ],
        ..natural(Language::English)
    };
    assert_eq!(
        render("short signature, second, alpha, off", &rules),
        "second, third, short signature, off"
    );
}

#[test]
fn expressions_and_lexicon_never_consume_explicit_line_breaks() {
    let rules = TextRules {
        expressions: vec![expression("add signature", "Jo\nQA")],
        lexicon: vec![entry("projet Mélusine", "match")],
        ..TextRules::default()
    };
    assert_eq!(
        render("add\nsignature\nprojet\nMélusine", &rules),
        "Add\nsignature\nprojet\nMélusine"
    );
    assert_eq!(render("\nadd signature\n", &rules), "\nJo\nQA\n");
}

#[test]
fn validation_reports_duplicates_cross_kind_collisions_and_containment() {
    use RuleConflictReason as Reason;
    let lexicon = [
        entry("projet MÉLUSINE", "a"),
        entry("Projet  me\u{301}lusine", "b"),
        entry("formule-courte", "c"),
    ];
    let expressions = [
        expression("formule courte", "a"),
        expression("FORMULE, COURTE !", "b"),
        expression("insère formule courte maintenant", "c"),
        expression("contreformule courte", "d"),
    ];
    let conflicts = validate_rules(&lexicon, &expressions);
    let pairs = |reason| conflicts.iter().filter(|c| c.reason == reason).count();
    assert_eq!(pairs(Reason::DuplicateHeard), 1);
    assert_eq!(pairs(Reason::DuplicateTrigger), 1);
    assert_eq!(pairs(Reason::TriggerShadowsHeard), 2);
    assert_eq!(pairs(Reason::TriggerOverlap), 2);
    assert!(conflicts.iter().any(|c| {
        c.reason == Reason::TriggerOverlap
            && c.rule.id == "formule courte"
            && c.related
                .as_ref()
                .is_some_and(|r| r.id == "insère formule courte maintenant")
    }));
}

#[test]
fn validation_reports_empty_unmatchable_and_multiline_fields_for_enabled_rules() {
    use RuleConflictReason as Reason;
    let entries = [entry("  ", " \t"), entry("…", "x"), entry("a\nb", "y")];
    let mut disabled = expression("", "");
    disabled.enabled = false;
    let expressions = [expression("«?!»", ""), disabled];
    let conflicts = validate_rules(&entries, &expressions);
    assert_eq!(conflicts.len(), 6);
    for reason in [
        Reason::EmptyMatch,
        Reason::EmptyReplacement,
        Reason::NoMatchableWords,
        Reason::MultilineMatch,
    ] {
        assert!(
            conflicts
                .iter()
                .any(|c| c.reason == reason && c.related.is_none())
        );
    }
    assert!(conflicts.iter().all(|c| !c.rule.id.is_empty()));
    let rules = TextRules {
        lexicon: entries.to_vec(),
        expressions: expressions.to_vec(),
        ..TextRules::default()
    };
    assert_eq!(render("a\nb …", &rules), "A\nb …");
}

#[test]
fn first_configured_rule_wins_ties_even_without_validation() {
    let rules = TextRules {
        lexicon: vec![entry("yes", "one"), entry("YES", "two")],
        expressions: vec![expression("hello", "first"), expression("HELLO!", "second")],
        ..TextRules::default()
    };
    assert_eq!(render("HELLO YES", &rules), "first one");
}

#[test]
fn preview_and_render_agree() {
    let rules = TextRules {
        expressions: vec![expression("signature", "Jo\nQA")],
        ..TextRules::default()
    };
    assert_eq!(
        preview("bonjour ! signature", &rules),
        render("bonjour ! signature", &rules)
    );
}

#[test]
fn spoken_breaks_use_the_joined_take_and_sentence_case() {
    let rules = TextRules::default();
    let sample = rambler_audio::join_segment_texts([
        "bonjour. À LA",
        "LIGNE, merci, nouveau paragraphe. à bientôt, nouvelle ligne, fin",
    ]);
    assert_eq!(render(&sample, &rules), "Bonjour\nMerci\n\nÀ bientôt\nFin");
    assert_eq!(preview(&sample, &rules), render(&sample, &rules));
    assert_eq!(render("Nouvelle ligne.", &rules), "\n");
    assert_eq!(render("à la ligne nouveau paragraphe", &rules), "\n\n\n");
    assert_eq!(
        render("bonjour à la ligne, voici", &rules),
        "Bonjour\nVoici"
    );
    assert_eq!(
        render("bonjour, à\tla ligne, voici", &rules),
        "Bonjour\nVoici"
    );
    assert_eq!(
        render("oui ! À la ligne, c'est fini.", &rules),
        format!("Oui{NNBSP}!\nC'est fini.")
    );
}

#[test]
fn spoken_quotes_and_parentheses_follow_french_typography() {
    let rules = TextRules::default();
    let sample = "elle dit, ouvrez les GUILLEMETS, oui, fermez les guillemets, puis ouvrez la parenthèse, bas, fermez la parenthèse.";
    assert_eq!(
        render(sample, &rules),
        format!("Elle dit «{NNBSP}oui{NNBSP}» puis (bas)")
    );
    let mut raw = rules;
    raw.options = options(false, false, false);
    raw.options.voice_commands = true;
    assert_eq!(render(sample, &raw), "elle dit «oui» puis (bas)");
    assert_eq!(render("oui, à la ligne, non", &raw), "oui\nnon");
    assert_eq!(
        render(
            "ouvrez la parenthèse ouvrez la parenthèse oui fermez la parenthèse fermez la parenthèse",
            &raw
        ),
        "((oui))"
    );
}

#[test]
fn spoken_layout_needs_natural_enabled_and_french() {
    let input = "Bonjour, à la ligne, merci.";
    let mut rules = TextRules::default();
    rules.options.voice_commands = false;
    assert_eq!(render(input, &rules), input);
    rules.options.voice_commands = true;
    rules.profile = RenderProfile::Faithful;
    assert_eq!(render(input, &rules), input);
    rules.profile = RenderProfile::Natural;
    rules.language = Language::English;
    assert_eq!(render(input, &rules), input);
}

#[test]
fn spoken_layout_preserves_literal_and_ambiguous_content() {
    let mut rules = TextRules {
        options: TextOptions {
            voice_commands: true,
            ..options(false, false, false)
        },
        ..TextRules::default()
    };
    for input in [
        "il est allé à la ligne 3 du tableau",
        "il est allé, à la ligne 3 du tableau",
        "le texte continue à la ligne.",
        "ce nouveau paragraphe est court",
        "j'ai créé une nouvelle ligne ce matin",
        "à la ligne 7 du tableau",
        "n'ouvrez les guillemets que si nécessaire",
        "préouvrez les guillemets",
        "ouvrez les guillemetsanglais",
        "à la ligne_suffixe",
        "`ouvrez les guillemets`",
        "```\nÀ la ligne, voici\n```",
        r#""C:\\mon dossier\\ouvrez la parenthèse""#,
        "https://example.test/ouvrez les guillemets",
        "example.test/fermez la parenthèse",
        "https://example.test/?x=à la ligne, merci",
        "voir Mme Dupont et M. à la ligne suivante",
    ] {
        assert_eq!(render(input, &rules), input, "{input}");
    }
    rules.expressions = vec![
        expression("insère ma formule", "à la ligne, ouvrez les guillemets"),
        expression("nouvelle ligne", "expression prioritaire"),
    ];
    rules.lexicon = vec![
        entry("allé", "parti"),
        entry("citation", "ouvrez la parenthèse"),
    ];
    assert_eq!(
        render("insère ma formule, nouvelle ligne, citation", &rules),
        "à la ligne, ouvrez les guillemets, expression prioritaire, ouvrez la parenthèse"
    );
    assert_eq!(
        render("Il est allé à la ligne suivante", &rules),
        "Il est parti à la ligne suivante"
    );
}

#[test]
fn voice_command_help_matches_the_renderer() {
    let rules = TextRules::default();
    let entries = voice_commands();
    assert_eq!(entries.len(), 7);
    for entry in entries {
        let expected = entry.result.replace('↵', "\n");
        assert_eq!(render(&entry.spoken, &rules), expected, "{}", entry.spoken);
        assert!(!entry.description.is_empty());
    }
}

#[test]
fn clean_prose_is_idempotent_with_all_switch_combinations() {
    for language in [Language::French, Language::English] {
        for mask in 0..8 {
            let rules = TextRules {
                options: options(mask & 1 != 0, mask & 2 != 0, mask & 4 != 0),
                ..natural(language)
            };
            for input in [
                "l’été à 9 h 05. merci !",
                "salut  ;«déjà?» oui,,encore ?!",
                "lisez src/main.rs et écrivez à qa@example.org !",
                "hello. wait... yes . go!now",
                "after 3. 14 no, , yes. Café 🇫🇷",
                "42  € ?! c'est ça\n\nUne autre ligne.",
            ] {
                let clean = render(input, &rules);
                assert_eq!(
                    render(&clean, &rules),
                    clean,
                    "starting from {input:?}, {language:?}, mask {mask}"
                );
            }
        }
    }
}

#[test]
fn french_guillemets_replace_straight_quotes_from_rambler() {
    let rules = crate::text::TextRules::from_settings(&crate::settings::Settings::default());
    let real = "Bonjour Claire,\n\nMerci pour ton retour. \"Parfait\".";
    assert_eq!(
        crate::text::render(real, &rules),
        "Bonjour Claire,\n\nMerci pour ton retour. «\u{202f}Parfait\u{202f}»."
    );
    assert_eq!(
        crate::text::render("Bonjour Claire, merci pour ton retour.\n\n\"Parfait\"", &rules),
        "Bonjour Claire, merci pour ton retour.\n\n«\u{202f}Parfait\u{202f}»"
    );
    // Odd count, URL and code stay as received.
    assert_eq!(crate::text::render("Un \" seul", &rules), "Un \" seul");
    assert!(crate::text::render("Ouvre \"https://exemple.fr\"", &rules).contains("\"https://exemple.fr\""));
    assert!(crate::text::render("Mets name=\"valeur\"", &rules).contains("name=\"valeur\""));
}

#[test]
fn french_guillemets_respect_language_profile_and_app_overrides() {
    let base = crate::text::TextRules::from_settings(&crate::settings::Settings::default());
    let quoted = "He said \"fine\".";
    let mut english = base.clone();
    english.language = crate::model::Language::English;
    assert_eq!(crate::text::render(quoted, &english), quoted);
    let mut faithful = base.clone();
    faithful.profile = crate::model::RenderProfile::Faithful;
    assert_eq!(crate::text::render("Il a dit \"oui\".", &faithful), "Il a dit \"oui\".");
    // A per-app style turning typographic spaces off (e.g. VS Code) keeps quotes.
    let mut no_typography = base.clone();
    no_typography.options.typographic_spaces = false;
    assert!(crate::text::render("Il a dit \"oui\".", &no_typography).contains("\"oui\""));
}
