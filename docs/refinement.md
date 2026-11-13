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
  ou concatène les trois chaînes du champ 7. Les autres intentions et le champ 2 à
  sémantique inconnue ne sont pas interprétés. L'appelant peut inspecter les réponses
  complètes si une politique différente est nécessaire.
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

## Capacités et validation à venir

Les intentions 0 à 6 sont décodables et couvertes par des tests construits ; **seule
l'intention 2 est observée** dans les preuves fonctionnelles réelles actuelles. La
présence des champs de biais/contacts `InputBoxInfo.field3`, des préférences `field5`
ou de langues secondaires `UserInfo.field4` dans le schéma ne prouve pas leur effet
serveur ; ils ne sont pas exposés dans l'API courante. Le moment approprié pour ces
paramètres, notamment avant ou après l'audio, reste à vérifier. Ne pas activer des
flags ou expérimenter sur les quotas par supposition. Voir la [reconstruction du
protocole](research/protocol.md).

Un échantillon anglais naturel LibriSpeech de 3,505 s est disponible avec provenance,
hash et attribution **CC BY 4.0** dans [natural-corpus-inputs.json](evidence/natural-corpus-inputs.json).
Le corpus français essayé reste indisponible : l'un des accès par serveur de données
n'est pas accessible sans authentification ; l'autre dépasse la limite de scan. Aucun
compte, nouveau credential ou téléphone n'est nécessaire au profil actuel pour autant.
Les scénarios longue dictée, reformulation, composition et sélection emoji sont
préparés dans [refinement-validation.json](../tools/fixtures/refinement-validation.json),
mais **aucun résultat serveur de ces scénarios n'est encore revendiqué**.

Les bancs de pages 20/40/60/100 ms et la signature de tous les résultats `state=1`
sont préparés ; la grille n'a pas encore été lancée. Il n'y a donc **aucune nouvelle
mesure de latence** à rapporter ici. Les anciennes mesures restent distinctes dans
[performance.md](performance.md).
