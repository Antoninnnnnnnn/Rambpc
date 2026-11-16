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
ces preuves fonctionnelles. Les hints sont désormais exposés comme indiqué ci-dessous ;
leur encodage ne garantit pas leur effet serveur. Restent notamment une évaluation
représentative des voix naturelles, l'endurance plus longue, les réseaux réellement
dégradés, le démarrage à froid et la portée des autres intentions. Les observations
supplémentaires ci-dessous ne remplacent pas les rapports historiques.

## Poursuite : corpus naturel, composition et endurance

### Dix nouveaux extraits naturels et trois essais progressifs

Les [entrées et licences](evidence/natural-expansion-inputs.json) couvrent **7 extraits
anglais LibriSpeech et 3 français MLS**, chacun avec un identifiant de locuteur et
de chapitre distinct dans sa langue. Les 72,825 s de lecture sont normalisées en
PCM16 mono 16 kHz sans découpage. Les références sont celles des jeux de données,
pas des transcriptions indépendamment corrigées.

Les [dix sessions accélérées](evidence/natural-expansion-public.json) terminent avec
source complète, fin S3/gRPC et rendu unique. Le [résumé qualité](evidence/natural-expansion-quality.json)
compte 3/109 mots en anglais et 3/79 en français selon le WER ponctuation/casse/blancs
normalisés, **sans** équivalence entre nombres et mots ni entre mots composés.
`cannon balls`/`cannonballs`, `narwhale`/`narwhal` et `cinq`/`5` contribuent donc au
score ; `eut achevé`/`achevait` est une véritable différence de temps par rapport à
la référence, sans que l'audio ait été réannoté indépendamment. Ne pas présenter
ces ratios comme une précision générale, ni toute différence comme une perte audio.

[Trois sessions à débit simulé](evidence/natural-expansion-realtime-public.json),
une anglaise et deux françaises, ont reçu les mêmes textes que leurs versions
accélérées avec le même PCM source consommé. Ce sont encore de courts extraits
lus, pas du bruit, du dialogue spontané ou un microphone réel.

### Composition : recherche et observations, pas simple fonctionnalité oubliée

L'[analyse dédiée](research/composition.md) distingue l'enum/mock `COMPOSE`, le
câblage vide des alternatives MUSE, le chemin Android Lite et le service S3 natif.
Les cinq essais sans invocation restent de la dictée pour les demandes de création,
avec réécriture positive pour le témoin connu. Les essais d'invocation ultérieurs
reconnaissent phonétiquement « Hey Gboard » mais le conservent littéralement avec et
sans biais ; « Okay » complet avec biais reste aussi littéral. Ils ne valident pas
COMPOSE 4. Aucune impossibilité universelle ni activation par flags n'est déduite.

### Hints au démarrage : SDK 0.3

`ProtocolConfig` accepte des **tuples** optionnels `bias_terms`, `preferences` et
`secondary_languages`, vides par défaut. Ils sont sérialisés dans l'initialisation :
`InputBoxInfo` extension 1203, champs 3 et 5 répétés, puis `UserInfo` extension 293000,
champ 4 répété pour les langues. Le contexte d'édition reste une requête distincte
**après l'audio**, avant la fin. Sans options, les goldens restent identiques.
Exemple :

```python
from rambler import ProtocolConfig
config = ProtocolConfig(language='en-US', bias_terms=('Zunarel', 'Valmera'),
                        preferences=('Use British spelling.',),
                        secondary_languages=('fr-FR',))
```

Le SDK impose au plus 1 024 chaînes par catégorie biais/préférences, un budget
UTF-8 total de 1 MiB pour ces deux catégories, et au plus 16 langues secondaires
avec un budget total de 1 024 octets UTF-8 ; les tags secondaires sont non vides.
Les autres limites de message restent applicables. Ce sont des bornes SDK, pas des
quotas ni des limites serveur démontrées. Les contacts et consignes peuvent être
sensibles : ne les envoyer qu'avec autorisation.

L'[expérience bornée 10/10](evidence/input-hints-public.json) conserve hash du
manifeste et résumés : avec biais, `Zunarell` devient `Zunarel`, mais `and` devient
une virgule ; un seul cas ne démontre pas de gain global. La préférence d'orthographe
britannique n'a pas d'effet démontré ; les deux configurations FR/EN donnent le même
texte mixte correct, sans bénéfice isolé des langues secondaires. L'acceptation
observée de ces requêtes ne démontre pas une prise en compte universelle des hints.

Le placement initial est appuyé par `ihv` lignes 153–211, `knv-simple` lignes
259–309 et `zxh` lignes 243–255 dans les sorties statiques identifiées par
[protocol.md](research/protocol.md). Le contexte tardif reste indépendant. Ne pas
activer de flags, quotas ou chemins d'authentification par supposition.

### Endurance progressive : premier palier réel

Le [palier 118,682 s](evidence/endurance-120-public.json) a consommé ses **3 797 836
octets PCM**, reçu une finale unique et conservé «Amber Orchard», «Cobalt Lantern»
et «Violet Harbor» dans cette finale, sans concaténation ni déduplication.
Collecte/pipeline : 119,880 s, CPU processus 5,891 s ; ce CPU inclut encodage,
hashing, instrumentation et moteur, pas le seul service distant.

RSS courant échantillonné : 58 511 360 octets au début, 60 723 200 à la fin,
maximum échantillonné 60 907 520. L'écart maximal entre échantillons est 1,015 s,
sous le plafond de couverture de 3 s. Ce n'est ni le pic exact, ni le high-water
historique, ni une preuve d'absence de fuite. Le banc distingue succès transport,
intégrité source, couverture mémoire et divergences sémantiques.

`tools/validate_endurance.py` limite sa campagne à trois sessions séquentielles,
300 s audio et 360 s de timeout par session ; **ce sont des garde-fous du banc,
pas des limites serveur ou du SDK établies**. Le palier 300 exige un rapport
120 complet et compatible, pas un simple booléen `healthy`. Le rapport local
n'est toutefois pas une attestation infalsifiable. Une lacune de sampling rend le
profil inconclusif sans transformer un transport réussi en échec.

Le [palier 300](evidence/endurance-300-public.json) comporte trois sessions saines
séquentielles de **288,3515625 s** chacune, même canal et cadence simulée 20 ms ;
chaque source PCM de **9 227 250 octets** est intégrale, les marqueurs sont présents
et les ensembles finaux ordonnés identiques. Les trois durées pipeline sont environ
291,238 / 290,908 / 290,747 s, avec CPU processus 17,094 / 14,469 / 14,672 s.
Les maximums RSS échantillonnés sont 60 796 928 / 61 120 512 / 61 661 184 octets ;
ce ne sont ni des pics exacts ni une preuve d'absence de fuite. Ces mesures utilisent
la base SDK 0.2 **avant intégration des hints**, avec hashes de modules dans les
rapports ; elles ne mesurent pas le nouveau code 0.3.

Les rapports structurés du banc ne contiennent pas d'octets bruts, valeurs inconnues
ou identifiants de service. `--include-text` exige toujours une revue d'un corpus
contrôlé ; une dictée personnelle n'est pas automatiquement publiable. Les captures
privées de continuation restent dans `artifacts/continuation-private/`, hors git.

Reproduction facultative avec les sources autorisées déjà préparées, variables d'accès
environnementales autorisées et le répertoire `local/` existant ; cela contacte le
service réel et consomme du temps audio, sans relance automatique :

```sh
.venv/Scripts/python -I tools/validate_corpus.py --live --manifest tools/fixtures/input-hints-validation.json --output artifacts/my-hints/raw.json --timeout 90 --include-results
.venv/Scripts/python -I tools/validate_endurance.py --live --manifest tools/fixtures/endurance-validation.json --id endurance-two-minute-en --stage 120 --timeout 180 --output local/endurance-120.json
.venv/Scripts/python -I tools/validate_endurance.py --live --manifest tools/fixtures/endurance-validation.json --id endurance-five-minute-en --stage 300 --previous-report local/endurance-120.json --sessions 3 --timeout 360 --output local/endurance-300.json
```

Les fichiers de sortie doivent être nouveaux ; la préparation des audios est locale
et doit respecter les manifestes. Les rapports `validate_corpus --include-results`
contiennent des données brutes et exigent une projection expurgée et une revue avant
publication ; ne pas les confondre avec les rapports structurés d'endurance.
