# Rambpc — moteur Rambler natif

Moteur qui parle **directement au service Rambler/Jetson** (gRPC), reconstruit à
partir de PixelBoard 1.3.4. Aucun Android, téléphone, émulateur ni API Gemini
publique n'est requis à l'exécution. Pas d'interface graphique, pas de microphone,
pas d'insertion dans un éditeur : ces éléments appartiennent à l'application.

- **Cœur Rust** (crates `rambler-*`, CLI `rambler`) : chemin à utiliser pour une
  application. SDK et CLI fichier.
- **Python 0.3** : référence de conformité, pas un runtime caché du binaire.

## En bref

| Sujet | État | Détails |
|---|---|---|
| Cœur Rust, SDK et CLI fichier | Opérationnel. Tests hors ligne passés sous Windows, CI par lots sur Linux et macOS selon les lots | [ROADMAP](docs/ROADMAP.md), [rust.md](docs/rust.md) |
| Service réel | Réponses observées sur de petits corpus synthétiques et naturels (12 au 18 novembre 2026) | [Observations](#ce-qui-a-été-observé-sur-le-service-réel) |
| Édition | Préparation d'un remplacement avec contrôle de contexte, validation avant application | [Guide SDK](docs/research/sdk-integration-guide.md) |
| Parité PixelBoard | **Non complète** ; les modes avancés restent à qualifier | [Matrice de parité](docs/research/rust-feature-parity.md) |
| Vitesse | Gains locaux mesurés seulement ; aucun gain WAN revendiqué | [Mesures](docs/performance.md) |

## Démarrage rapide (Rust)

Prérequis : Rust/Cargo 1.88 (validé), compilateur natif de la plateforme (MSVC sur
Windows, compilateur C pour `ring` et libopus), CMake pour la feature audio.
Détails et toolchain : [construire et vérifier](docs/rust.md#construire-et-vérifier-hors-service-réel).

```sh
# Tests hors ligne : sans microphone ni service réel
cargo test --workspace --all-features --locked

# Binaire CLI, avec entrée WAV/PCM optionnelle
cargo build --release --locked -p rambler-cli --features audio
```

Le binaire est `target/release/rambler.exe` sur Windows et `target/release/rambler`
sur Linux/macOS. Pour appeler le service, fournir les métadonnées décrites dans
[Accès au service](#accès-au-service) (la CLI ne lit pas `.env` seule) :

```sh
# Windows / Git Bash ; retirer .exe sur Linux/macOS
set -a; source .env; set +a
target/release/rambler.exe dictate input.ogg --language fr-FR
```

`dictate` attend un flux Ogg/Opus continu, mono, source 16 kHz. Pour le préparer
avec FFmpeg (outil hors du moteur) :

```sh
ffmpeg -i input.wav -ac 1 -ar 16000 -c:a libopus -b:a 24k -application voip input.ogg
```

## Intégrer le SDK dans une application

Dépendances par chemin, depuis un checkout de ce dépôt (ajuster les chemins) :

```toml
[dependencies]
rambler-engine = { path = "../Rambpc/crates/rambler-engine" }
rambler-transport = { path = "../Rambpc/crates/rambler-transport" }
rambler-audio = { path = "../Rambpc/crates/rambler-audio" } # omettre pour de l'Ogg déjà encodé
bytes = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Parcours minimal :

1. Créer un `Engine` (`Engine::connect` ou `Engine::new` pour un transport partagé),
   puis fournir la langue et les credentials par session.
2. Fournir l'audio ordonné : `OggAudio::open` pour un fichier Ogg/Opus, ou un
   `AudioSource` ; le PCM passe par `rambler-audio` (voir [Entrées audio](#entrées-audio)).
3. Collecter avec `collect_dictation` (ou sa variante avec observateur) et lire le
   résultat **seulement après `Ok`**.
4. Pour une édition : `replacement_proposal` sur le résultat, approbation explicite
   de l'utilisateur, puis `validate_current` dans la même transaction d'éditeur.

Un exemple compilé est dans [`crates/rambler-engine/src/lib.rs`](crates/rambler-engine/src/lib.rs).
Le [guide d'intégration SDK](docs/research/sdk-integration-guide.md) détaille chaque étape.

Règles à respecter :

- `Ok` signifie que la session est terminée, pas qu'un texte est définitif à afficher.
- Aucun audio n'est rejoué automatiquement après une erreur ; ne jamais convertir un
  échec en `finish()`.
- Les aperçus ne servent ni à une édition, ni à un envoi, ni à un effacement.
- Ne journaliser ni audio, ni texte, ni contexte, ni réponses brutes, ni métadonnées.
- Un résultat d'édition peut contenir l'historique complet : ne pas l'ajouter au texte existant.
- Fermer l'engine uniquement à l'arrêt de l'application (`Engine::close()`), jamais pour
  écarter un aperçu.

## Entrées audio

| Entrée | Chemin | Remarques |
|---|---|---|
| Ogg/Opus continu, mono, 16 kHz | `dictate` ; `OggAudio` | Chemin validé principal. Lecteur sans codec natif : pages validées au fil de la lecture, sans cadence temps réel. |
| WAV PCM16LE mono 16 kHz | `dictate-wav` (feature `audio`) | Encodage Opus 24 kbit/s en pages de 20 ms, flush final. `--realtime` simule une arrivée progressive depuis un fichier, pas un microphone. |
| PCM 48 kHz mono PCM16LE | `pcm_channel` → `Pcm48kTo16k` → `Pcm16Opus`, puis `collect_dictation` (SDK) | Optionnel, sans changer le chemin 16 kHz. Voir [le guide, section 2](docs/research/sdk-integration-guide.md) et [BUILDING](crates/rambler-audio/BUILDING.md). |

Ne pas passer du PCM arbitraire étiqueté Opus, ni concaténer des clips encodés
séparément : l'entrée doit être **un seul flux Ogg/Opus continu**.

## Résultats et édition

`--result-policy` choisit la projection du résultat final :

| Valeur | Effet |
|---|---|
| `strict` (défaut) | Projection SDK conservatrice ; forme JSON inchangée. |
| `asr-only` | Texte de reconnaissance uniquement. |
| `s3-client-compatible` | Accepte aussi la réécriture d'une fenêtre capturée (intention 3, champ 7). |

Les choix explicites ajoutent `provenance` à la sortie (`hypothesis_text`,
`voice_edit_text` ou `voice_edit_context`).

```sh
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ten.'
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ' --selected ten --after '.' --cursor-aware
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --selected 'Send me the report now.' --cursor-aware --result-policy s3-client-compatible
```

Pour un remplacement ponctuel, `derive_scalar_splice` dérive depuis la proposition
stricte non modifiée une plage UTF-16 locale et son texte, après revalidation de la
fenêtre et de la révision. Détails et limites (scalaires, pas graphèmes) :
[guide, section 5](docs/research/sdk-integration-guide.md#opt-in-to-a-single-local-text-splice).
Les réécritures sont décrites dans [raffinement](docs/refinement.md) et dans le
[guide, section sur les réécritures](docs/research/sdk-integration-guide.md#opt-in-to-selection-rewrites).

## Accès au service

Le service attend trois métadonnées applicatives, fournies par l'environnement :
`RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` et `RAMBLER_ANDROID_CERT`.
**Aucune valeur n'est embarquée dans ce dépôt.** Les noms « Android » désignent les
headers du protocole observé, pas une dépendance d'exécution.

Pour Git Bash, remplir un fichier `.env` (ignoré par git, modèle `.env.example`),
puis charger ses variables avant la commande (voir l'exemple ci-dessus). Les tests
réels ont utilisé les métadonnées statiques du client analysé, sans compte Google,
OAuth ni Zwieback ; aucun renouvellement de ces valeurs n'a été observé. Leur
provisionnement reste local, et leur révocation ne peut pas être résolue par un
retry. Voir [authentification et limites](docs/research/authentication.md).

## Ce qui a été observé sur le service réel

Petits corpus synthétiques et naturels ; ces résultats ne mesurent pas la qualité générale.

| Date | Essai | Résultat |
|---|---|---|
| 12 nov. | Dictée anglaise | Texte ponctué, extension `VoiceEditingResult`, fin S3 et gRPC OK |
| 12 nov. | « …at ten. No, make that eleven. » | `The meeting is tomorrow at 11.` |
| 12 nov. | Contexte `The meeting is tomorrow at ten.` + « Change ten to eleven. » | `The meeting is tomorrow at eleven.` |
| 12 nov. | Sélection `ten`, contexte différé avant/sélection/après | Sélection remplacée par `eleven`, retour structuré en trois parties |
| 12 nov. | Dictée française | `Bonjour. Ceci est un essai de dictée intelligente. La réunion aura lieu demain à 11h.` |
| 13 nov. | Réécriture « Send me the report now. » | « Could you please send me the report at your earliest convenience? » |
| 13 nov. | Dix extraits naturels (7 EN, 3 FR), 72,825 s au total | Écarts WER normalisés de 3/109 mots (EN) et 3/79 (FR), dont formatage et un temps verbal |
| 18 nov. | Dictée de deux phrases et « Change blue to green » (voix synthétique) | Exacts : `The kite is blue. Marker stays.` puis `The kite is green. Marker stays.` |
| 18 nov. | Une phrase dans 13 langues (DE, ES, IT, PT, RU, AR, HI, TA, JA, KO, ZH, NL, PL) | [Toutes correctes](docs/evidence/rust-native-multilingual-real-20261118.json) ; hindi et tamoul renvoyés en alphabet latin |
| 18 nov. | Sélection « Send me the report now. » + « Make this more polite. » | Réécriture polie (intention 3, champ 7) acceptée avec `--result-policy s3-client-compatible` ; PixelBoard 1.3.5 ne l'a pas produite sur le téléphone de test (Xiaomi, pas un Pixel) |

Les flux ont consommé l'intégralité des octets audio avec vérification SHA-256. Preuves
expurgées : [docs/evidence/](docs/evidence/), notamment
[premier appel Rust](docs/evidence/rust-first-slice-public.json),
[PCM](docs/evidence/rust-pcm-functional-public.json),
[voix naturelles](docs/evidence/rust-native-natural-public.json) et
[comparaison PixelBoard](docs/evidence/pixelboard-vs-native-real-20261118.json).
La composition (intention 4) n'a pas été observée dans les essais ciblés ; ce n'est pas une
preuve d'impossibilité. Voir [l'analyse](docs/research/composition.md) et le
[protocole reconstruit](docs/research/protocol.md).

## Limites

- Pas de microphone, d'interface, d'insertion système ni de portage Tauri dans ce lot :
  voir le [contrat de portage](docs/architecture/portage-rust.md) et la
  [recommandation desktop](docs/architecture/application-desktop-multiplateforme.md).
- Accès réel au service depuis Linux non revendiqué comme testé.
- Une endurance conserve un échec intermittent ; sa cause n'est pas établie (voir
  [l'état détaillé](docs/STATUS.md) et le [diagnostic](docs/evidence/rust-incomplete-diagnostic-followup.json)).
- Aucune parité complète revendiquée ; les capacités du schéma ne sont pas des
  capacités validées en production. Le service et ses formats internes peuvent évoluer.
- Les optimisations sont mesurées localement (voir [mesures](docs/performance.md)).
- Métadonnées applicatives : provisionnement et révocation hors du moteur.

## Référence Python 0.3 : installer et tester

Python **3.11 ou plus récent**. Exemple Windows / Git Bash (Linux/macOS : remplacer
`.venv/Scripts/python` par `.venv/bin/python`) :

```sh
python -m venv .venv
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[test]'
.venv/Scripts/python -m pytest -q
.venv/Scripts/python -m rambler.cli --help
```

La suite locale utilise des serveurs gRPC loopback et ne contacte pas Google. La CI
offline/loopback et la construction passent sur Windows et Linux, Python 3.11/3.14.
Pour construire une roue : `python -m pip wheel --no-deps . -w dist`.

L'extra audio (Python **>=3.12**) ajoute PyAV/libopus embarqué :

```sh
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[audio,test]'
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR --realtime
```

La CLI Python émet du NDJSON (Unicode échappé sans perte, indépendant de la page de
codes Windows) et sépare résultats, erreurs et métriques. Les résultats d'édition peuvent
être du texte d'historique complet : ne pas les ajouter au texte existant.
L'import du codec a un coût de démarrage ; préparer le processus avant la capture si ce
délai est critique.

Intégration Python minimale :

```python
from contextlib import aclosing
from rambler.engine import RamblerEngine
from rambler.credentials import EnvironmentCredentials
from rambler.protocol import ProtocolConfig, EditingContext

async def ogg_chunks(path):
    with open(path, 'rb') as audio:
        while chunk := audio.read(8192):
            yield chunk

async def dictate(path):
    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        async with aclosing(engine.stream(
            ogg_chunks(path),
            config=ProtocolConfig(language='fr-FR'),
            context=EditingContext(),  # envoyé après l'audio, avant la fin
            timeout=30,
        )) as events:
            async for event in events:
                if event.response is not None:
                    for hypothesis in event.response.hypotheses:
                        # Préserver aussi voice_edit : intention et texte/contexte édité.
                        print(hypothesis)
```

Un producteur audio doit coopérer avec l'annulation asyncio ; utiliser `aclosing` ou
consommer jusqu'à la fin, car `break` seul ne ferme pas un générateur asynchrone.
`MetadataProvider` est injectable et rappelé à chaque session. Pour un PCM asynchrone,
passer `pcm16_to_ogg(source_pcm)` de `rambler.encoding` à `engine.stream`. Le contexte
peut être un callback asynchrone (`context_timeout=2.0`) ; une erreur de contexte n'est
jamais silencieusement remplacée par une requête sans contexte. Les biais, préférences
et langues secondaires sont dans `ProtocolConfig`. Pour le résultat final, la fonction
`collect_dictation` et les contrôles de `replacement_proposal` sont décrits dans
[raffinement](docs/refinement.md).

## Documentation

- État et journal : [STATUS](docs/STATUS.md) · priorités : [ROADMAP](docs/ROADMAP.md) · décisions : [DECISIONS](docs/DECISIONS.md)
- Rust : [moteur et CLI](docs/rust.md) · [guide d'intégration SDK](docs/research/sdk-integration-guide.md) · [parité](docs/research/rust-feature-parity.md)
- Protocole et accès : [protocole](docs/research/protocol.md) · [authentification](docs/research/authentication.md) · [raffinement et résultat](docs/refinement.md)
- Preuves et mesures : [preuves réelles](docs/evidence/) · [performance](docs/performance.md) · [plan de validation](docs/validation-plan.md)
- [Contribuer](CONTRIBUTING.md)

Ne pas committer de clés, tokens, audio personnel, captures brutes, APK ou sources
décompilées.
