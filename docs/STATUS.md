# État du moteur — 2026-11-12

## Résultat actuel

**Des réponses du véritable chemin Rambler ont été obtenues par un client natif
Windows sans Android.** Ce n'est plus seulement une piste statique ou une connexion
gRPC. Aucun téléphone, compte Google, OAuth ou Zwieback utilisé pour ces essais.

Chemin exercé : `speechs3proto2-pa.googleapis.com` / `S3Bidi`, service
`beyond-a2a-recognizer`, routage `gboard_gemini_v3_streaming_voice_edit_mul`,
Ogg/Opus16k mono, contexte tardif, fin S3 puis statut gRPC OK.

## Capacités et niveau de validation

| Capacité | Preuve actuelle |
|---|---|
| Dictée anglaise et française | Service réel, corpus synthétique contrôlé |
| Correction orale « ten… no, eleven » | Service réel, texte corrigé |
| Édition de texte existant par instruction | Service réel, contexte avant curseur et historique édité renvoyé |
| Intentions et résultats d'édition structurés | `VoiceEditingResult` réel, intention2 ; octets décodés indépendamment |
| Contexte en trois parties / sélection | Schéma et tests locaux ; pas encore exercé avec le service |
| Sessions successives / canal réutilisé | 21/21 sessions du banc accéléré réussies ; audio et texte identiques |
| Alimentation progressive à débit simulé | 6/6 sessions réussies ; pages20ms et1000ms, PCM décodé identique |
| Annulation, erreurs tardives, deadlines, isolation concurrente | Tests gRPC loopback via transport et moteur ; tests réels supplémentaires à effectuer |
| Protobuf inconnu / nouveaux champs | Préservés, dont un champ5 observé dans le service mais absent du schéma optimisé de l'APK |
| Entrée PCM16 avec encodeur natif optionnel | Validée au service réel via CLI : PyAV/libopus, aucun exécutable FFmpeg |

**354 tests locaux passent** en mode asyncio debug, avertissements traités comme
erreurs ; roue Python construite. La CI multi-plateforme reste à observer après push.

Le premier probe complet, trois cas fonctionnels, 21 sessions accélérées, six
sessions progressives et une CLI PCM représentent **32 sessions réussies** à ce
point du journal.
Ne pas extrapoler ce corpus à toutes les intentions, langues, qualités de voix ou
conditions réseau. Les réponses de champ `transcript` peuvent déjà être corrigées
ou contenir l'historique complet : ce n'est pas une garantie de verbatim audio.

## Preuves et performance

- [Observations et conditions](evidence/README.md)
- [Premier appel complet](evidence/reconstructed-default-session.json)
- [Correction, contexte, français](evidence/functional-cases.json)
- [Banc accéléré](evidence/performance-baseline.json)
- [Tamponnement temps simulé](evidence/realtime-paging.json)
- [Trois payloads réels publics inspectés](../tests/fixtures/live_synthetic_responses.json)

Les réponses binaires originales ont été redécodées indépendamment de notre
codec Python ; les fixtures publiques ne contiennent que textes synthétiques
contrôlés et champs numériques inspectés. Le reste des captures reste local,
hors git. Pas de metadata applicative dans les observations publiques.

Constats préliminaires : batch8192 réduit les messages67→5 et les payloads protobuf
d'environ6,5%, mais pas de gain de latence finale net dans le petit banc. Pages
Ogg20ms au lieu de1000ms : délai médian premier texte après épuisement de source
935→325ms, avec PCM décodé identique et davantage d'octets Ogg. Ces durées sont
observées côté client, pas du temps de calcul serveur ; l'expérience utilise une
source FFmpeg `-re`, pas un microphone réel.

## Dépendances et limites

- Python>=3.11, grpcio ; le codec d'entrée natif optionnel nécessite Python>=3.12
  et PyAV. Le flux déjà encodé n'en dépend pas.
- Trois métadonnées applicatives doivent être configurées avec une utilisation
  autorisée ; valeurs jamais embarquées dans le dépôt. Pas de provisionnement
  automatique de credentials. Aucun jeton de session temporaire copié à la main
  n'a été nécessaire pendant ces essais.
- Les metadata statiques n'ont pas de refresh observé. Une révocation ou une
  restriction future doit être traitée explicitement, pas contournée ou retryée.
- Zwieback : branche conditionnée par Jetson et `jetson_send_zb`, défautfalse.
  La première décompilation donnait une condition supplémentaire erronée sur
  l'application séparée ; le bytecode a permis de la corriger. Voir la note auth.
- Service interne non contractuel : schéma/routage/disponibilité peuvent changer.
- Pas de PCM brut étiqueté Opus, pas d'API Gemini alternative, pas d'application
  graphique ni d'intégration microphone/environnement dans ce périmètre.
- Nettoyage asyncio suppose des producteurs coopératifs. Les résultats partiels
  ne valent pas une fin de session. Erreurs S3 et statut final gRPC sont séparés.
- Voix naturelles, endurance longue, transport réellement perdu et renouvellement
  de credentials après révocation restent non validés.

## Historique utile

Dépôt initialement vide ; premier lot documentation/wire poussé dans
`4745be3`, branche `feat/rambler-engine`. Analyse statique APK PixelBoard1.3.4
(SHA dans notes recherche), vérification bytecode ciblée, puis preuves serveur
réelles. Le Xiaomi proposé n'a pas été utilisé. La documentation technique et les
mesures restent versionnées avec le code pour permettre les contributions.
