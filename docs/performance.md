# Mesures de performance et choix retenus

Mesures initiales du 2026-11-12. Il ne s'agit ni d'un classement de langages ni
d'une garantie de latence du service. Chaque réponse comptée réussie a un texte
exploitable, une fin S3, un gRPC OK et une entrée audio complète.

## Conditions et horloges

Windows11, Python3.14.5, grpcio1.85.1, 12 processeurs logiques ; détails et limites
[dans l'environnement](evidence/environment.json). Internet ordinaire, charge
machine et réseau non contrôlés, aucune instrumentation du serveur. Horloge
monotone côté client. Le corpus est la même voix synthétique Zira, phrase anglaise
de6,658s (WAV), conteneur Ogg/Opus de6,665s, entrée compressée14 136 octets dans le
premier banc. Ce n'est pas un corpus de voix naturelles.

Les premiers rapports proviennent d'un arbre de travail expérimental avant le
commit de consolidation : ils ne portent pas rétroactivement un hash de code
exact. Cette limite est conservée, pas dissimulée. Les outils publics ajoutent
maintenant provenance du code, versions et configuration aux nouveaux rapports.
Les payloads fonctionnels ont été vérifiés indépendamment ; leurs hashes et trois
captures originales inspectées sont versionnés.

Mesures distinctes :

- `first_any_response` peut être un heartbeat, pas du texte utilisable.
- `first_text` est le premier candidat textuel ; une erreur terminale ultérieure
  invaliderait la réussite de la session.
- Fin S3 et EOF gRPC sont séparés. Le résultat final ne se déduit pas d'un partiel.
- `last_audio_yield` / épuisement de source sont des timestamps **producteur**,
  pas des preuves de dernière écriture réseau terminée ni d'acquittement serveur.
- Les octets comptés sont des payloads protobuf ; pas le trafic TLS/HTTP2 exact.
- Sans instrumentation distante, temps réseau et calcul serveur restent mêlés.

## 1. Fichier préenregistré, envoi accéléré

[21 sessions originales](evidence/performance-baseline.json) : un warmup exclu des
agrégats, puis quatre groupes de5 essais, ordre alterné, concurrence1. Tous ont
réussi ; SHA-256 de l'audio consommé identique, texte et intention identiques.

| Canal API | Bloc Ogg | Premier texte médian | EOF médian | Payload requête médian | Messages |
|---|---:|---:|---:|---:|---:|
| Neuf | 224o | 3114ms | 3117ms | 15 317o | 67 |
| Neuf | 8192o | 3153ms | 3155ms | 14 327o | 5 |
| Réutilisé | 224o | 3171ms | 3174ms | 15 317o | 67 |
| Réutilisé | 8192o | 3117ms | 3120ms | 14 327o | 5 |

**Gain démontré :** 62 messages en moins et990 octets protobuf économisés,
soit6,46% des payloads requête sur cet audio. Le surcoût protobuf hors audio passe
de1181 à191 octets, environ84% de moins. Le cadrage gRPC minimal de5octets/message
économiserait310octets supplémentaires, mais HTTP2/TLS n'a pas été capturé.

**Gain non démontré :** accélération de la latence finale. Les écarts changent de
sens selon le groupe ; les distributions se recouvrent. Le p95 nearest-rank à n=5
est simplement le maximum du groupe, pas une estimation fiable de longue traîne.

**Neuf ne signifie pas nouvelle connexion physique.** Le canal réutilisé reste
ouvert ; gRPC C-core peut partager ses sous-canaux avec les canaux API neufs.
L'ouverture initiale du warmup vaut environ51ms, puis les ouvertures API médianes
environ3ms. Ces3ms ne sont donc pas un handshake TLS froid. Processus froid,
DNS froid et serveur froid ne sont pas comparés ici.

CPU du processus de mesure : médianes15,625–31,25ms par groupe, en marches visibles
de15,625ms sur Windows. Cela inclut lecture/hash et capture du banc, ainsi que le
client natif. Aucun gain CPU comparatif n'est établi avec cette granularité.
High-water RSS du processus environ47,3→49,0MiB sur21 sessions : mesure globale,
pas pic par session ni preuve d'absence de fuite en endurance longue.

**Choix :** exposer le réglage batch8192 pour les fichiers disponibles. Ne jamais
attendre artificiellement8192octets d'une source en direct : cela ajouterait une
latence d'accumulation. Réutiliser le moteur/canal sans partager l'état des streams.

## 2. Source progressive et tamponnement Ogg

[Six sessions originales](evidence/realtime-paging.json), trois par configuration,
ordre alterné. Même WAV, encodeur FFmpeg/libopus24kbps, mono16k, trames20ms,
`-re`, `-flush_packets 1`. Seule durée de page comparée :1000ms contre20ms. Un canal
ouvert, aucune relance. Toutes les sessions ont réussi avec le **même texte**.

| Réglage page Ogg | Premier événement de parole médian | Texte après épuisement source, médiane | Fin S3 après épuisement source, médiane | Ogg produit |
|---|---:|---:|---:|---:|
| 1000ms | 1663ms | 935ms | 937ms | 14 136o |
| 20ms | 216ms | 325ms | 325ms | 22 965o |

**Gain observé :** environ610ms, soit65% de moins pour le premier texte après
épuisement de la source. La fin S3 évolue de façon cohérente. **Fidélité :** les
six flux réenregistrés donnent exactement le même SHA-256 du PCM redécodé ; aucune
modification audio attribuable au changement de pages.

**Contrepartie :** environ62% d'octets Ogg en plus à cause des pages courtes.
Ce n'est pas une optimisation de bande passante. Le gain vient du tamponnement de
l'entrée, pas d'un changement du modèle distant ou d'une accélération démontrée
de son calcul. Aucun texte exploitable avant la fin n'a été observé dans ce petit
corpus, même si les marqueurs de parole arrivent plus tôt.

**Limite de simulation :** la source FFmpeg `-re` s'épuise vers6,15s pour6,66s
d'audio encodé ; ce n'est pas un microphone ni une simulation parfaite du temps
d'arrivée de chaque échantillon. Les deltas après épuisement sont comparables dans
l'expérience ; ne pas les appeler automatiquement « après la fin acoustique ».
Ils incluent réseau, tamponnement restant et service. Le CPU de l'encodeur externe
n'est pas le CPU du seul moteur ; aucune conclusion CPU n'est tirée ici.

**Choix :** profil natif optionnel PCM→Ogg/libopus avec pages20ms, mémoire bornée
et flush final explicite. La preuve FFmpeg ne valide pas cette implémentation
native à elle seule : elle doit avoir ses propres tests et sessions réelles.

## 3. Validation native et démarrage

[Une CLI PCM réelle](evidence/native-pcm-cli.json) a fonctionné avec PyAV/libopus
sans processus FFmpeg : texte attendu, VoiceEditingResult2, fin S3 et gRPC OK,
code de sortie0. Dans ce premier processus neuf, total observé5,725s, dont environ
3,198s dans le stream moteur à l'arrivée du texte. L'écart comprend démarrage de
Python, imports, préparation CLI et connexion ; aucun découpage n'en isole un
responsable. Ce sample unique n'est pas une médiane de démarrage froid.

Trois imports PyAV ultérieurs dans des processus neufs, **caches OS non purgés**,
ont pris115,169 /97,754 /99,964ms, hors réseau et hors mesure de connexion. Ces
mesures ne justifient donc pas d'attribuer les2,5s d'écart du premier essai au seul
import PyAV. L'encodeur reste facultatif/lazy ; un processus long-vivant peut le
préparer avant de commencer la capture. Ne pas confondre ce préchauffage avec une
réduction du temps de calcul du service.

## Reproduire

Préparer un WAV/Opus synthétique avec `tools/make_fixture.ps1` ou un corpus autorisé,
puis configurer les trois variables de metadata applicatives sans les journaliser.

```sh
python tools/benchmark_engine.py --live --audio artifacts/synthetic-v1/plain-en.ogg --language en-US --samples 5 --chunk-sizes 224,8192 --output local/batch-run.json
python tools/benchmark_realtime.py --help
```

Les outils refusent d'écraser une observation existante. Les fichiers bruts et les
audios restent locaux. `--include-results` est réservé aux textes contrôlés que
l'on souhaite publier. Sans ce flag, les résultats du banc sont hachés. Les tests
locaux sont indépendants du service ; les tests FFmpeg optionnels ne prouvent que
le producteur de test.

## Ce qui reste à mesurer

Démarrage de processus réellement froid, microphone/PCM réel, longues sessions,
réseaux dégradés, ressources par session isolée, trafic réseau exact et qualité sur
voix naturelles. Aucun langage concurrent n'a été benchmarké ; on ne prétend pas
que Python soit le plus rapide possible. Les mesures montrent à ce stade que le
levier important identifié est le pipeline d'entrée ; une réécriture du transport
n'aurait pas, à elle seule, démontré le gain de finalisation observé.
