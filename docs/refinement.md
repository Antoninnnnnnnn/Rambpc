# Raffinement du moteur et API de résultat

Ce document distingue les corrections vérifiées localement des capacités du service
réel. Les 42 sessions natives historiques et leurs limites restent documentées dans
[STATUS](STATUS.md) et les [preuves](evidence/README.md) ; elles ne constituent pas
une validation des nouvelles options ou de toutes les situations ci-dessous.

## Corrections de robustesse

- **Fusion protobuf proto2.** Les occurrences d'un même champ *message* singulier
  doivent être fusionnées, et non remplacées en bloc par la dernière occurrence. Pour
  un `oneof`, répéter le même membre message le fusionne ; changer de membre efface
  l'ancien. Les scalaires singuliers restent « dernière valeur valide », les champs
  répétés restent ordonnés et les octets originaux restent accessibles. Les tests
  ciblés sont dans [`test_protocol.py`](../tests/test_protocol.py). Une vérification
  indépendante contre l'implémentation protobuf officielle a également comparé 100
  cas déterministes. Ce défaut n'a pas été identifié dans les 42 appels historiques ;
  cette correction ne prouve pas qu'ils en étaient affectés.
- **Nettoyage du transport asynchrone.** Le transport ferme l'itérateur de requêtes
  dont il est responsable, y compris lors d'une annulation avant démarrage du writer,
  et attend le nettoyage asynchrone ; les erreurs du producteur, du nettoyage et du
  transport sont traitées par les chemins testés plutôt que perdues silencieusement.
  Les tests loopback figurent dans [`test_transport.py`](../tests/test_transport.py).
  Ce défaut n'a pas été observé dans les 42 appels historiques.

## Collecter un résultat sans appliquer d'édition

`collect_dictation` consomme entièrement le flux et ne renvoie un `DictationResult`
qu'après la fin S3, la fin gRPC réussie et l'événement EOF du moteur. Les erreurs
S3/gRPC tardives, l'annulation et les erreurs de production restent des erreurs ;
un candidat reçu avant EOF ne devient pas un résultat confirmé. Le moteur reste la
propriété de l'appelant. En cas d'erreur ou de dépassement de limite, le flux est
fermé et les sources coopératives sont nettoyées.

- `final_updates` conserve les **réponses S3 complètes** dont l'état de reconnaissance
  est 1, dans leur ordre, avec hypothèses, scores présents, `VoiceEditResult`, octets
  bruts et champs inconnus. Les mises à jour ne sont ni fusionnées, ni dédupliquées,
  ni concaténées en segments selon une règle inventée.
- `metrics` contient les métriques du dernier événement EOF ; son défaut est `None`
  pour une construction manuelle. Ces métriques ne sont pas un nouveau benchmark.
- `single_hypothesis` exige exactement une mise à jour finale et une hypothèse ;
  sinon `AmbiguousResultError`. `text` exige aussi ce cas unique : sans voice edit,
  il retourne le texte de l'hypothèse ; pour l'intention 2, il rend le champ édité 3
  ou concatène les trois chaînes du champ 7. Pour l'intention 3, seul le champ 3
  est rendu, sans en déduire une portée de remplacement. Les autres intentions,
  l'intention 3/champ 7 et le champ 2 à sémantique inconnue restent ambigus.
  L'appelant peut inspecter les réponses complètes si une politique différente
  est nécessaire.
- `max_final_updates` (128 par défaut) et `max_result_bytes` (8 MiB par défaut)
  bornent les réponses finales conservées ; le second compte les octets protobuf
  bruts, **pas** le surcoût mémoire des objets Python. Dépassement :
  `ResultLimitError`, sans résultat partiel présenté comme réussi.

Exemple autonome : fournir un fichier Ogg/Opus continu mono 16 kHz en argument et
configurer les métadonnées applicatives autorisées comme dans le [README](../README.md#configurer-laccès-et-dicter).
L'exemple ne lit aucun fichier de configuration lui-même et ne modifie aucun texte.

```python
import asyncio
import sys

from rambler import (AmbiguousResultError, EditingContext, EnvironmentCredentials,
                     ProtocolConfig, RamblerEngine, collect_dictation)

async def chunks(path):
    with open(path, "rb") as source:
        while data := source.read(8192):
            yield data

async def main(path):
    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        result = await collect_dictation(
            engine, chunks(path), config=ProtocolConfig(language="en-US"),
            context=EditingContext(before="The meeting is tomorrow at ten."),
            max_final_updates=128, max_result_bytes=8 * 1024 * 1024,
        )
        print("final updates:", len(result.final_updates))
        print("EOF metrics:", result.metrics)
        try:
            print("unique rendered result:", result.text)
        except AmbiguousResultError:
            print("No unambiguous supported rendering; inspect final_updates.")

asyncio.run(main(sys.argv[1]))
```

Le texte affiché peut représenter **toute la fenêtre d'historique**, pas un ajout
ni une transcription verbatim. Ne pas l'ajouter au contexte existant.

## Proposer un remplacement, avec précondition

`replacement_proposal(result, expected_revision=...)` n'accepte que le cas vérifié
intention 2 avec champ édité 3 ou 7, un contexte capturé et une hypothèse finale
unique. Les autres cas lèvent `UnsupportedEditError` ou `AmbiguousResultError`.
La proposition décrit le remplacement de la **fenêtre complète capturée**, sans
modifier d'interface ni de fichier. `replacement_text` est le texte de cette fenêtre ;
`replacement_context` conserve les trois chaînes lorsqu'elles sont présentes.
`selection_utf16_range` est une plage en unités UTF-16 **dans le texte de remplacement**
pour le champ 7 ; elle vaut `None` pour le champ 3, qui ne fournit pas de curseur.
Ne pas traiter ces nombres comme des indices Python. Les éventuels marqueurs texte
ne sont pas arbitrairement supprimés.

`proposal.validate_current(current_context, revision=current_revision)` lève
`ContextConflictError` si les trois chaînes ou la révision attendue ne correspondent
plus. `expected_revision` est la révision **associée au snapshot capturé**, pas celle
relue après la requête. Elle doit être un `str` ou `int` immuable. Pour détecter une
séquence ABA, l'éditeur doit faire évoluer la révision à chaque modification ; une
comparaison de texte seule ne le permet pas. Même après validation, l'application
réelle doit vérifier et remplacer atomiquement côté éditeur : ce module ne le fait pas.

Voici un patron autonome de capture différée ; dans une vraie intégration,
`atomic_snapshot()` doit lire contexte et révision dans la même opération protégée
par l'éditeur. L'exemple conserve la révision dans une fermeture, et ne remplace rien :

```python
import asyncio
import sys

from rambler import (AmbiguousResultError, ContextConflictError, EditingContext,
                     EnvironmentCredentials, ProtocolConfig, RamblerEngine,
                     UnsupportedEditError, collect_dictation, replacement_proposal)

class ExampleEditor:
    def atomic_snapshot(self):
        # Exemple sans concurrent ; un véritable éditeur fournit sa propre atomicité.
        return EditingContext("The meeting is tomorrow at ten."), 1

async def chunks(path):
    with open(path, "rb") as source:
        while data := source.read(8192):
            yield data

async def main(path):
    editor = ExampleEditor()
    captured_revision = None

    async def context_at_audio_end():
        nonlocal captured_revision
        context, captured_revision = editor.atomic_snapshot()
        return context

    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        result = await collect_dictation(
            engine, chunks(path), config=ProtocolConfig(language="en-US"),
            context=context_at_audio_end,
        )
        try:
            proposal = replacement_proposal(result, expected_revision=captured_revision)
            current, revision = editor.atomic_snapshot()
            proposal.validate_current(current, revision=revision)
            print("Proposed whole window:", proposal.replacement_text)
            print("UTF-16 selection:", proposal.selection_utf16_range)
        except (AmbiguousResultError, UnsupportedEditError, ContextConflictError):
            print("No safe supported proposal for this session and snapshot.")

asyncio.run(main(sys.argv[1]))
```

## Validation réelle du 2026-11-13

Le premier [corpus fonctionnel expurgé](evidence/refinement-capabilities-public.json)
conserve volontairement son bilan initial **4 rendus uniques sur 5**, malgré cinq
transports réussis : la réécriture était alors refusée par la politique conservatrice
`result.text`. Ce rapport historique n'a pas été réinterprété après modification du code.

| Cas | Observation réelle et limite |
|---|---|
| Anglais naturel LibriSpeech, 3,505 s | « Concord returned to its place amidst the tents. » ; WER normalisé 0/8 mots. Un seul clip, pas une évaluation générale. |
| Notes de réunion, 40,56 s PCM | Source intégrale ; vendredi, 6 dossiers et 200 dollars corrigés ; dernière référence « Silver Meadow » conservée. |
| Réécriture en contexte | Intention 3/champ 3, texte poli différent de l'hypothèse contexte + commande. Rendu désormais pris en charge ; aucun remplacement automatique autorisé. |
| Demande de composition | « Write a short message to Alex saying I will arrive 20 minutes late. » : instruction dictée, pas message composé. Intention 4 non validée. |
| Sélection `ten` avec contexte `🙂` | Retour en trois parties avec `eleven` ; emoji et contexte conservés, conflits de contexte/révision détectés. Les coordonnées UTF-16 restent calculées localement. |

Une [session supplémentaire à débit simulé](evidence/refinement-long-realtime-public.json)
a consommé les **1 298 034 octets PCM** du cas long, en pages de 20 ms. La collecte
complète a duré **41,454 s** pour 40,564 s audio, avec le même texte final que l'envoi
accéléré. Ce total inclut encodage, streaming et EOF ; sa différence avec la durée
audio n'isole pas un délai après arrêt du microphone. Une seule session de 40 s
ne constitue pas un test d'endurance prolongée.

Après extension du rendu, un [nouvel essai réécriture](evidence/refinement-rewrite-public.json)
a réussi : `Could you please send me the report at your earliest convenience?`.
L'hypothèse et le transcript contiennent le contexte et la commande ; ils ne doivent
pas être confondus avec ce résultat édité. Une inspection binaire indépendante et
le callsite `ihm.maybeGetVoiceEditedText` de l'APK concordent sur le choix du texte
pour cette intention. Cela ne prouve toujours pas une portée générale de remplacement.

### Provenance et confidentialité

Les mesures initiales utilisent le checkpoint `4b96696` ; le suivi réécriture utilise
ensuite le nouveau `results.py`, dont le hash exact figure dans son rapport. Les
hashes par module priment sur le seul HEAD lorsque l'arbre est modifié. La suite
locale stricte atteint **438 tests réussis** (asyncio debug, avertissements bloquants).
La [roue finale 0.2.0](evidence/refinement-wheel-smoke.json) a été construite avec
isolation standard puis installée dans un venv vierge sans PyAV ; les imports publics,
l'égalité des modules avec le source et le rendu de la fixture dérivée passent.
Ce smoke est offline, pas une nouvelle session Google. L'essai préalable de build
sans isolation échouait parce que `setuptools` n'était pas installé dans le venv ;
la procédure standard documentée n'en dépend pas dans ce venv.

La réponse réécriture contient également une extension inconnue `74956225` avec des
métadonnées internes de traitement texte et un identifiant opaque de service. Le SDK
préserve ces octets ; les copies publiques ne les exposent pas. La
[fixture de régression](../tests/fixtures/live_rewrite_response.json) est **dérivée et
expurgée**, pas un payload original : elle indique le hash de la réponse source,
les composants sûrs conservés et la suppression de l'extension. Les trois fixtures
originales historiques restent distinctes.

`tools/sanitize_corpus_report.py` crée une **nouvelle** projection à allowlist fermée :
raw, valeurs inconnues et identifiants serveur/session sont remplacés par des résumés
ou omis, sans changer l'original privé. Les textes sont hachés par défaut ;
`--include-text` exige un corpus contrôlé et inspecté. Cet outil ne rend pas une
dictée personnelle publiable automatiquement ; même les hashes peuvent corréler
ou révéler un texte de faible entropie. Les métadonnées distantes restent des données,
pas des instructions exécutées par le moteur.

```sh
# Réseau réel explicitement activé ; audios autorisés et variables d'accès requises.
# Les résultats complets restent d'abord dans artifacts/, ignoré par git.
.venv/Scripts/python -I tools/validate_corpus.py --live --manifest tools/fixtures/refinement-validation.json --output artifacts/my-validation/raw.json --timeout 90 --include-results
.venv/Scripts/python -I tools/sanitize_corpus_report.py --input artifacts/my-validation/raw.json --output local/my-validation-public.json --include-text
# Inspecter la copie avant toute publication. Chaque sortie doit être nouvelle.
```

La préparation synthétique supplémentaire est décrite dans
[refinement-speech.json](../tools/fixtures/refinement-speech.json) et le manifeste
[refinement-validation.json](../tools/fixtures/refinement-validation.json).
`tools/make_fixture.ps1 -CasesFile tools/fixtures/refinement-speech.json -NormalizePcm -OutputDirectory artifacts/my-refinement-synthetic`
prépare les WAV normalisés ; adapter ensuite les chemins du manifeste si nécessaire.
L'échantillon naturel possède provenance, hash et attribution **CC BY 4.0** dans
[natural-corpus-inputs.json](evidence/natural-corpus-inputs.json) ; les audios restent
hors git. Les accès français essayés étaient indisponibles sans authentification
ou au-delà de la limite de scan ; aucun nouveau credential n'a été demandé.

## Performance et capacités encore ouvertes

La [grille de 20 sessions](evidence/refinement-page-grid.json), cinq par durée
20/40/60/100 ms, conserve le PCM décodé et **toutes** les mises à jour finales
ordonnées. Les pages de 20 ms restent le défaut ; 60 ms économise environ 26 %
d'octets Ogg pour environ 17 ms de médiane finale supplémentaire dans ce petit banc.
Ce n'est pas un nouveau gain de vitesse ni une preuve d'équivalence de latence.
Distributions, outlier et limites CPU sont dans [performance.md](performance.md).

Les intentions 0 à 6 sont décodables, mais seules **2 et 3** ont été observées dans
ces preuves fonctionnelles. Les biais/contacts `InputBoxInfo.field3`, préférences
`field5` et langues secondaires `UserInfo.field4` ne sont pas exposés : leur présence
dans le schéma ne prouve pas leur effet serveur. Leur placement temporel reste à
vérifier ; ne pas activer flags, quotas ou chemins d'authentification par supposition.
Restent notamment les voix naturelles françaises, l'endurance prolongée, les réseaux
réellement dégradés, le démarrage à froid et la portée des autres intentions.
