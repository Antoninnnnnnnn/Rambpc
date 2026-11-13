# État du moteur — 2026-11-13

## Raffinement 0.2.0 — code et nouveaux essais réels du 2026-11-13

Branche locale : `feat/engine-refinement`, construite sur `1f52c94` sans modifier
la branche publiée. **438 tests locaux passent** avec asyncio debug et avertissements
bloquants. La [roue 0.2.0 finale](evidence/refinement-wheel-smoke.json) est construite
et installée dans un venv vierge sans PyAV : imports publics, modules identiques au
source et rendu de la fixture dérivée vérifiés offline. Les mesures initiales
portent le checkpoint `4b96696` ; le suivi
réécriture enregistre aussi les hashes du nouveau code de rendu.

- Deux défauts corrigés : fusion des messages protobuf singuliers répétés (y compris
  même membre oneof) et nettoyage producteur asynchrone interrompu par la fin RPC.
  Aucun des deux n'a été observé dans les 42 essais historiques ; reproductions
  offline/loopback et vérification canonique ajoutées.
- API de résultat final bornée, ambiguïté explicite, proposition de remplacement
  sans effet de bord et contrôle snapshot/révision : [guide](refinement.md).
- **20/20 sessions de la grille** 20/40/60/100 ms réussies, PCM décodé et toutes les
  mises à jour finales ordonnées identiques. Défaut 20 ms conservé ; 60 ms économise
  environ 26 % d'Ogg avec environ 17 ms de médiane finale supplémentaire sur ce banc.
  Aucun nouvel optimum de vitesse établi ; [distributions et limites](performance.md).
- Cinq cas fonctionnels ont terminé leur transport ; quatre rendus uniques au
  premier passage. La réécriture réelle, intention 3/champ 3, a révélé une limite de
  l'API puis a réussi dans un nouvel essai après correction du rendu. Le rapport
  initial 4/5 est conservé, pas réécrit.
- Clip naturel anglais de 3,505 s : WER 0/8 mots, sans généralisation. Dictée de
  40,56 s intégrale en accéléré puis à débit simulé (collecte 41,454 s), même texte
  corrigé et référence finale conservée. Sélection avec emoji réussie. La demande
  de composition est restée une dictée de l'instruction, pas une composition validée.

Les **27 nouveaux transports complets** sont les 20 mesures, cinq cas fonctionnels,
une dictée longue progressive et le suivi réécriture ; ils ne constituent pas
27 succès de rendu du premier coup. Les rapports publics sont expurgés, les captures
avec identifiants opaques restent hors git. Aucune nouvelle CI distante ou exécution
Google/Linux n'est revendiquée pour ce raffinement local.

## Jalon réel du 2026-11-12

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
| Intentions et résultats d'édition structurés | `VoiceEditingResult` réel, intentions 2 et 3 ; cas historiques et réécriture décodés indépendamment |
| Réécriture polie en contexte | Service réel, intention 3/champ 3 rendue par le SDK ; pas de proposition de remplacement pour cette intention |
| Voix naturelle anglaise | Un clip LibriSpeech de 3,505 s, WER normalisé 0/8 ; validation très limitée |
| Dictée de 40,56 s | Envoi accéléré puis progressif, source intégrale et même texte final ; pas un test d'endurance prolongée |
| Sélection avec emoji | Service réel, contexte non-BMP conservé et sélection `ten`→`eleven` |
| Contexte en trois parties / sélection | Service réel : `ten`→`eleven`, contexte différé appelé après EOF audio, retour avant/sélection/après |
| Sessions successives / canal réutilisé |21/21 sessions du banc accéléré réussies ; audio et texte identiques |
| Alimentation progressive à débit simulé |12/12 sessions réussies : six FFmpeg, six encodeur natif ; PCM décodé identique entre pages dans chaque expérience |
| Annulation, isolation concurrente | Service réel : source fermée après annulation, puis sessions anglaise/française concurrentes sur même canal sans mélange |
| Erreurs tardives et deadlines | Tests gRPC loopback via transport et moteur ; pas d'injection d'erreur dans le vrai service |
| Protobuf inconnu / nouveaux champs | Préservés, dont un champ5 observé dans le service mais absent du schéma optimisé de l'APK |
| Entrée PCM16 avec encodeur natif optionnel | Validée au service réel via CLI : PyAV/libopus, aucun exécutable FFmpeg |

Au jalon précédent, **356 tests locaux passaient** en mode asyncio debug, avertissements traités comme
erreurs ; roue Python construite et installée dans un environnement vierge sans
PyAV. La [CI du jalon0814813](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43304009641)
a réussi sur Windows/Linux et Python3.11/3.14 (tests réseau local uniquement).

Le premier probe, trois cas fonctionnels,21 sessions accélérées,12 progressives,
une CLI PCM, trois cas sélection/concurrence et un smoke de roue installée
représentent **42 sessions complètes réussies**, plus **une annulation volontaire
réussie**. Le diagnostic anonyme initial a été refusé ; il n'est pas compté comme
une session réussie.
Ne pas extrapoler ce corpus à toutes les intentions, langues, qualités de voix ou
conditions réseau. Les réponses de champ `transcript` peuvent déjà être corrigées
ou contenir l'historique complet : ce n'est pas une garantie de verbatim audio.

## Preuves et performance

- [Observations et conditions](evidence/README.md)
- [Premier appel complet](evidence/reconstructed-default-session.json)
- [Correction, contexte, français](evidence/functional-cases.json)
- [Banc accéléré](evidence/performance-baseline.json)
- [Tamponnement source FFmpeg](evidence/realtime-paging.json)
- [Encodeur natif progressif, source et commit hashés](evidence/native-realtime.json)
- [Sélection, contexte différé, annulation et concurrence](evidence/lifecycle.json)
- [Roue installée, configuration locale, sans APK à l'exécution](evidence/installed-wheel-smoke.json)
- [Trois payloads réels publics inspectés](../tests/fixtures/live_synthetic_responses.json)
- [Grille native 20/40/60/100 ms du raffinement](evidence/refinement-page-grid.json)
- [Corpus fonctionnel initial expurgé, bilan historique 4/5](evidence/refinement-capabilities-public.json)
- [Dictée de 40,56 s progressive, copie expurgée](evidence/refinement-long-realtime-public.json)
- [Réécriture après extension du rendu, copie expurgée](evidence/refinement-rewrite-public.json)
- [Roue 0.2.0 finale, installation et smoke offline sans PyAV](evidence/refinement-wheel-smoke.json)
- [Fixture réécriture dérivée et expurgée, non originale](../tests/fixtures/live_rewrite_response.json)

Les **33 réponses des trois premiers cas fonctionnels** ont été redécodées
indépendamment de notre codec Python ; les fixtures publiques correspondantes ne
contiennent que textes synthétiques contrôlés et champs numériques inspectés.
Les preuves ultérieures (sélection, cycle de vie et banc natif) proviennent du
codec moteur et ne sont pas présentées comme une nouvelle inspection binaire
indépendante. Le reste des captures reste local, hors git. Pas de metadata
applicative dans les observations publiques.

Constats préliminaires : batch8192 réduit les messages67→5 et les payloads protobuf
d'environ6,5%, mais pas de gain de latence finale net dans le petit banc. Pages
Ogg20ms au lieu de1000ms : délai médian premier texte après épuisement de source
935→325ms, avec PCM décodé identique et davantage d'octets Ogg. Le banc natif sur
le commit0814813 reproduit le gain : **990→328ms** (fin S3 :991→330ms), trois
répétitions par réglage, aucun processus FFmpeg. Ces durées sont observées côté
client, pas du temps de calcul serveur ; les deux expériences utilisent une
source simulée, pas un microphone réel. Ne pas mélanger les horloges EOF des deux
producteurs (stdout encodé FFmpeg contre lecteur PCM natif).

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
- Au-delà de l'unique clip naturel anglais et des sessions de 40,56 s, qualité sur
  voix naturelles, endurance prolongée, transport réellement perdu et renouvellement
  de credentials après révocation restent non validés.

## Historique utile

Dépôt initialement vide ; premier lot documentation/wire poussé dans
`4745be3`, branche `feat/rambler-engine`. Analyse statique APK PixelBoard1.3.4
(SHA dans notes recherche), vérification bytecode ciblée, puis preuves serveur
réelles. Le Xiaomi proposé n'a pas été utilisé. La documentation technique et les
mesures restent versionnées avec le code pour permettre les contributions.
