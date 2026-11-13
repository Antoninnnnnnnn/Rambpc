# Rambpc — moteur Rambler sans Android

Moteur du **véritable service Rambler/Jetson**, reconstruit à partir de PixelBoard
1.3.4. Portage Rust en cours ; Python 0.3 reste la référence de conformité.
Connexion directe gRPC, sans Android, téléphone, émulateur ni API Gemini publique
à l'exécution. Pas d'interface graphique.

## Fonctionnement réellement observé

Le 12 novembre 2026, le client natif a obtenu des réponses de
`beyond-a2a-recognizer` avec le routage
`gboard_gemini_v3_streaming_voice_edit_mul` :

| Essai synthétique | Résultat du service |
|---|---|
| Dictée anglaise | Texte ponctué, extension `VoiceEditingResult`, fin S3 et gRPC OK |
| « …at ten. No, make that eleven. » | `The meeting is tomorrow at 11.` |
| Contexte `The meeting is tomorrow at ten.` + « Change ten to eleven. » | `The meeting is tomorrow at eleven.` |
| Sélection `ten`, contexte différé avant/sélection/après | Sélection remplacée par `eleven`, retour structuré en trois parties |
| Dictée française | `Bonjour. Ceci est un essai de dictée intelligente. La réunion aura lieu demain à 11h.` |

Le raffinement du **13 novembre 2026** ajoute une réécriture réelle (« Send me the
report now. » → « Could you please send me the report at your earliest convenience? »),
une sélection avec emoji, une dictée de **40,56 s** en accéléré puis à débit simulé,
et dix nouveaux extraits naturels (7 anglais, 3 français), en plus du premier clip
anglais de 3,505 s. Ces dix extraits totalisent 72,825 s ; leurs écarts WER normalisés
sont de 3/109 mots en anglais et 3/79 en français, dont des variations de formatage
et une différence de temps verbal par rapport aux références du jeu de données.
La composition n'a pas été observée dans les essais ciblés du profil S3 actuel ;
ce n'est pas une preuve d'impossibilité générale. Voir [l'analyse](docs/research/composition.md).

Les flux ont consommé l'intégralité des octets audio avec vérification SHA-256.
Les [observations expurgées](docs/evidence/) et la [reconstruction du protocole](docs/research/protocol.md)
précisent les preuves et leurs limites. Ces petits corpus n'établissent pas la qualité
générale sur voix naturelles, toutes les intentions, ni la disponibilité future
du service interne.

## Cœur Rust

Le workspace contient le codec, le transport, le moteur et une CLI fichier
Ogg/Opus. Voir **[construire, utiliser et intégrer le moteur Rust](docs/rust.md)**
pour les commandes Cargo, les garanties de fermeture et le périmètre validé.
Python 0.3 reste la **référence de conformité**, pas un runtime caché du binaire.
Les **trois premiers appels Rust réels** ont réussi sur Windows : anglais,
français et édition sur contexte, avec un PATH système sans Python/FFmpeg.
[Preuves et limites](docs/evidence/rust-first-slice-public.json) ; 121 tests Rust
et 643 tests de référence Python passent. Ce lot prend de l'Ogg/Opus déjà encodé,
pas encore l'entrée PCM native. Aucun gain de latence global n'est revendiqué.

Les preuves historiques ci-dessus concernent Python tant qu'une preuve Rust
n'est pas explicitement identifiée. Tauri, microphone et intégrations système
restent hors de ce lot. Voir le [contrat de portage](docs/architecture/portage-rust.md)
et la [recommandation desktop](docs/architecture/application-desktop-multiplateforme.md).

## Référence Python : installer et tester

Python **3.11 ou plus récent**. Exemple Windows / Git Bash :

```sh
python -m venv .venv
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[test]'
.venv/Scripts/python -m pytest -q
.venv/Scripts/python -m rambler.cli --help
```

Linux/macOS : remplacer `.venv/Scripts/python` par `.venv/bin/python`.
La suite locale utilise des serveurs gRPC loopback et ne contacte pas Google.
Le service réel a été exercé sur Windows. La CI offline/loopback et la construction
passent sur Windows et Linux, Python3.11/3.14 ; l'accès réel au service depuis Linux
n'est pas revendiqué comme testé. Pour construire une roue :
`python -m pip wheel --no-deps . -w dist`.

## Configurer l'accès et dicter

Le service observé attend trois métadonnées applicatives :
`RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` et `RAMBLER_ANDROID_CERT`.
Les fournir dans l'environnement avec des valeurs dont l'utilisation est autorisée.
**Aucune valeur n'est embarquée dans ce dépôt.** Les noms « Android » désignent
les headers du protocole observé, pas une dépendance d'exécution Android.

Les tests réels ont utilisé les métadonnées statiques du client analysé, sans
compte Google, OAuth ou Zwieback. Aucun renouvellement de session de ces valeurs
statiques n'a été observé. Leur provisionnement reste une configuration locale ;
leur révocation/restriction future ne peut pas être résolue par un retry.
Voir [authentification et limites](docs/research/authentication.md).

Pour une configuration locale Git Bash, remplir un fichier `.env` (ignoré par git,
modèle vide `.env.example`), puis charger ses variables avant les commandes :

```sh
set -a
source .env
set +a
```

Le moteur n'a pas besoin de l'APK ni des outils d'analyse à l'exécution.

Le chemin validé prend un **flux Ogg/Opus continu, mono, source 16 kHz**. Il ne
prend pas du PCM arbitraire étiqueté Opus. Exemple de préparation explicite :

```sh
ffmpeg -i input.wav -ac 1 -ar 16000 -c:a libopus -b:a 24k -application voip input.ogg
.venv/Scripts/python -m rambler.cli dictate input.ogg --language fr-FR
# Édition d'un texte existant
.venv/Scripts/python -m rambler.cli dictate instruction.ogg --language en-US --before 'The meeting is tomorrow at ten.'
```

FFmpeg sert ici uniquement à préparer l'entrée ; le cœur du moteur ne l'exécute
pas et n'en dépend pas. Sur Windows, `tools/make_fixture.ps1` génère un corpus
synthétique reproductible avec SAPI et FFmpeg, sans microphone.

### Entrée PCM native, sans exécutable externe

Python **>=3.12** et l'extra audio ajoutent PyAV/libopus embarqué :

```sh
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[audio,test]'
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR
# Simulation de l'arrivée progressive depuis un fichier, pas acquisition microphone
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR --realtime
```

Entrée WAVE PCM16LE mono16k, encodage en flux Opus24kbps / trames et pages20ms,
flush final sans perdre les derniers échantillons. Ce chemin a lui aussi reçu une
vraie réponse Rambler via la CLI. L'encodeur optionnel est chargé à la demande ;
une entrée déjà Ogg/Opus n'importe pas PyAV. L'import/initialisation du codec a un
coût de démarrage : préparer le processus avant la capture si ce délai est critique.

La CLI émet du NDJSON (Unicode échappé sans perte, indépendant de la page de codes
Windows) et sépare résultats, erreurs et métriques. Les résultats
d'édition peuvent être du **texte d'historique complet** : ne pas les ajouter
aveuglément au texte existant.

## API d'intégration

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

Réutiliser le même moteur ouvert pour plusieurs sessions. Les streams gardent
leur état propre ; aucun audio n'est rejoué automatiquement après une erreur.
Un producteur audio doit coopérer avec l'annulation asyncio. Utiliser `aclosing`
ou consommer jusqu'à la fin ; `break` seul ne ferme pas déterministement un
générateur asynchrone Python. `MetadataProvider` est injectable et rappelé à chaque
session pour permettre une gestion de credentials propre à l'intégrateur.

Pour une source PCM asynchrone, passer `pcm16_to_ogg(source_pcm)` depuis
`rambler.encoding` à `engine.stream`. Pour obtenir le contexte actuel au moment
de l'arrêt audio, `context` accepte aussi un callback **asynchrone** retournant
`EditingContext` ; il est appelé après l'épuisement audio, avec délai borné
(`context_timeout=2.0`, toujours limité par le délai global). Une erreur de contexte
ne devient pas silencieusement une requête sans contexte.

Les erreurs `IncompleteSessionError` et `S3SessionError` sont exportées ; une
exception gRPC peut survenir après des réponses déjà reçues. N'appliquer un résultat
comme définitif qu'après consommation réussie du flux complet.

**Arrêt normal :** terminer la source audio puis continuer à lire ; le moteur
flushe l'encodeur, envoie le contexte et le marqueur final, puis half-close.
**Annulation :** annuler la tâche consommatrice, fermer explicitement le stream
ou appeler `engine.close()` ; cela interrompt la session, ce n'est pas une
finalisation de dictée. Le test réel de reprise après annulation est dans les preuves.

### Biais, préférences et langues secondaires

`ProtocolConfig` accepte les tuples optionnels `bias_terms`, `preferences` et
`secondary_languages` ; ils sont envoyés à l'initialisation, tandis que le contexte
d'édition reste différé après l'audio. Leurs limites et la différence entre encodage
SDK et effet serveur sont précisées dans [raffinement](docs/refinement.md). Les essais
bornés n'établissent ni gain général, ni composition vocale intention 4.

### Résultat final et proposition d'édition

Pour une intégration qui n'a pas besoin de traiter chaque événement, l'API publique
`collect_dictation` consomme le flux jusqu'à une fin S3 et gRPC réussie. Elle conserve
les mises à jour finales séparément avec leurs réponses brutes et expose les métriques
EOF. Les erreurs tardives sont propagées ; les limites `max_final_updates` et
`max_result_bytes` bornent la conservation.

```python
from rambler import (AmbiguousResultError, EditingContext, ProtocolConfig,
                     collect_dictation)

async def collect(engine, ogg_chunks):
    result = await collect_dictation(
        engine, ogg_chunks, config=ProtocolConfig(language='fr-FR'),
        context=EditingContext(before='Texte existant.'),
    )
    try:
        print(result.text)  # seulement si le résultat est unique et interprétable
    except AmbiguousResultError:
        print('Inspecter result.final_updates sans choisir arbitrairement.')
    return result
```

`result.text` rend les champs vérifiés d'édition historique (intention 2) et le
texte réécrit du champ 3 pour l'intention 3 ; il ne choisit pas arbitrairement parmi
plusieurs résultats. `replacement_proposal` reste limité à l'intention 2 : rendre
une réécriture ne prouve pas sa portée de remplacement dans un éditeur.

`replacement_proposal` peut décrire le remplacement d'une fenêtre de contexte pour
le cas d'édition historique vérifié, avec contrôle de contexte et révision avant
application. Aucun texte n'est appliqué automatiquement : un résultat édité peut
inclure l'historique complet et ne doit pas lui être ajouté. Pour les détails, les
exemples exécutables, les unités UTF-16 et les limites de validation, voir
[API de résultat et raffinement](docs/refinement.md).

## Documentation et contribution

- [État, capacités et limites](docs/STATUS.md)
- [Décisions](docs/DECISIONS.md) · [protocole](docs/research/protocol.md) · [authentification](docs/research/authentication.md)
- [Preuves réelles](docs/evidence/) · [mesures et optimisations](docs/performance.md) · [plan de validation](docs/validation-plan.md)
- [Contribuer](CONTRIBUTING.md)

Ne pas committer de clés, tokens, audio personnel, captures brutes, APK ou sources
décompilées. Le service et les formats internes peuvent évoluer ; une capacité
présente dans le schéma n'est pas une capacité validée en production.
