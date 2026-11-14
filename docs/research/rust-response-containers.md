# Conteneurs de réponse Rust : structure sans politique de rendu

État : 2026-11-14. Schéma statique, tests offline ; **aucune nouvelle capacité du
service validée**, aucun nouvel appel réseau ou essai Android. Ce lot ne change
ni la requête, ni le wire parser, ni la sélection de texte du moteur.

## Preuve et périmètre

Les références Java suivantes concernent le build APK décrit dans
[rust-feature-parity.md](rust-feature-parity.md). La vérification des métadonnées
brutes a été confirmée par le parent du lot, après l'analyse Kimi
`a99ede6646ef4ac75` ; les positions ci-dessous sont des repères de ce build,
pas des identifiants pérennes d'API.

| Conteneur | Métadonnées vérifiées | API Rust |
|---|---|---|
|`RecognitionResult` / `aaln.java:33`|field5 : `aalp` singulier, présence bit4 (`0x10`) ; field7 : `aalp` singulier, présence bit5 (`0x20`) ; indépendants de3/4 et entre eux, **pas un oneof**|`hypothesis_result_field5`, `hypothesis_result_field7` : `Option<HypothesisResult>`|
|`HypothesisResult` / `aalp.java:29`|field3 : `aalg` répété existant ; field5 : `aalo` singulier, présence `0x10`|`hypotheses` inchangé ; `fallback_field5: Option<HypothesisFallback>`|
|`HypothesisFallback` / `aalo.java:27`|field1 : `aalg` singulier, présence bit0 ; **hypothèse complète, pas une string**|`hypothesis: Option<Hypothesis>`, `raw`, `unknown_fields`|

Les noms par numéro n'attribuent pas de rôle métier non prouvé. Le mot
`Fallback` désigne ici le wrapper vérifié, pas une consigne de sélection.
Le champ10 de `RecognitionResult` reste opaque : la connaissance d'une map
`int32 -> int64` dans `aalm.field5` ne suffit pas à nommer son rôle.

## Fusion et conservation

- Chaque message singulier fusionne récursivement toutes ses occurrences proto2.
  Une occurrence vide conserve sa présence et n'efface pas les données antérieures.
  Un wrapper field5 vide est distinct d'un wrapper avec field1 vide, lui-même
  distinct de l'absence de wrapper.
- Les hypothèses répétées field3 conservent ordre et doublons au sein de leur
  conteneur. Les trois conteneurs et le résultat partiel field4 coexistent.
- Le wrapper appelle le **même décodeur `Hypothesis`** que la liste primaire :
  texte, confiance, safety field30 fermé0/1, VoiceEdit/oneof/contexte, raw et
  inconnus ne sont pas projetés vers une simple chaîne.
- Les mauvais wire types restent inconnus et ne créent ni présence ni changement
  de valeur. Les valeurs safety inconnues ne suppriment pas un0/1 déjà reconnu.
- `raw` racine reste exact ; le `raw` d'un singulier fusionné est la concaténation
  logique des payloads originaux, pas une capture unique. Chaque occurrence est
  parsée séparément : deux fragments malformés ne peuvent pas se réparer.
- La limite racine reste **8 MiB** et le budget global **100000 champs**.
  Chaque champ interprété est débité une seule fois, y compris les valeurs safety
  et les branches ensuite effacées par le oneof de l'enveloppe. Les payloads
  inconnus ne sont pas parcourus récursivement.

Des occurrences length-delimited autrefois opaques sont désormais validées comme
messages : un contenu malformé sur ces chemins est maintenant une erreur, même
si une occurrence ultérieure aurait fourni un résultat utilisable.

## Lecture structurelle et absence de priorité métier

`hypotheses()` et `partials()` restent limités à leurs chemins primaires
existants. Aucun texte n'est concaténé, promu, sélectionné ou appliqué.

`all_hypotheses()` est disponible sur `HypothesisResult`, `RecognitionResult` et
`S3Response` pour l'inspection de tous les candidats du résultat actif. Son ordre
est **canonique par conteneur**, pas un classement ni l'ordre global du wire :

1. conteneur RecognitionResult field3 : liste répétée, puis éventuel fallback ;
2. conteneur field5 : liste répétée, puis éventuel fallback ;
3. conteneur field7 : liste répétée, puis éventuel fallback.

Les champs publics des modèles permettent de retrouver cette provenance.
L'itérateur ne ressuscite pas les résultats déjà effacés par le oneof extérieur
et n'inclut pas les hypothèses partielles, qui ont un autre schéma.
Le parent du lot adapte séparément la politique safety du moteur.

Il ne faut pas transplanter une priorité d'un autre client :

- SmartDictation (`suz.java:128–189`) utilise le primaire3, puis le fallback
  seulement si sa liste est vide ; il peut utiliser7 pour le non-final.
- Rambler (`ihm.java:142–157,224–253`) ignore5/7/fallback sur ces chemins.

Ces comportements statiques distincts ne prouvent aucune priorité serveur ni
aucune nouvelle fonction distante.

## Validation offline

`tests/response_containers.rs` apporte huit tests, dont :

- **11111 séquences exhaustives** de longueur0 à4, dix opérations indépendantes :
  liste, fallback et fallback vide dans chacun des conteneurs3/5/7, et partiels4 ;
  oracle par opérations indépendant des encodeurs/décodeurs de production,
  fusion des enveloppes et égalité des modèles/raw ;
- fusion récursive fallback → hypothèse complète → VoiceEdit → contexte,
  conservation d'un safety1 malgré valeurs inconnues/occurrences vides ;
- invariance aux mauvais wire types avec présence et inconnus exacts ;
- chaque coupure intérieure d'un champ aux nouvelles frontières singulières,
  y compris VoiceEdit/contexte et branches effacées ensuite ; contrôles positifs
  pour les coupures entre champs ;
- wire/UTF-8 invalides impossibles à masquer, field1 vérifié comme message ;
- budget exact100000, rejet100001, avec safety et branches fusionnées/effacées ;
  borne exacte8 MiB sur un nouveau chemin, rejet au-delà ; field10 toujours opaque.

Vérifiés avec `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, cible courte locale au worktree :

- nouveaux tests : **8/8** ;
- suites existantes : adversarial4, decode20, requests10, safety5, wire4,
  mutations/troncatures des fixtures1 ; doctest1 : **tous passent** ;
- `cargo clippy -p rambler-protocol --all-targets --locked -- -D warnings` : OK.

La suite intégrale initiale met en évidence une assertion historique à adapter
par le parent dans `tests/fixtures.rs:146` : elle attendait RecognitionResult
field5 dans les inconnus. Il devient logiquement structurel ; les fichiers de
fixtures, payloads et SHA-256 ne sont pas modifiés par ce lot. Le test concerné
n'est pas compté parmi les succès ci-dessus. La dernière exécution globale a
explicitement exclu cette seule assertion historique en attente d'intégration.

Ces tests bornés ne sont ni une campagne de fuzzing exhaustive ni une preuve
service. Aucun rendu nouveau n'est autorisé par ces seules structures.
