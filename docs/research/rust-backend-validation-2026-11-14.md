# Validation du backend Rust — jalon du 14 novembre 2026

Ce document décrit un jalon, pas un backend déclaré parfait ou terminé. Les travaux de consolidation et de validation continuent ; les conteneurs vérifiés et le validateur Ogg sont maintenant intégrés. Les preuves ci-dessous ont chacune leur périmètre ; elles ne constituent pas une garantie générale d'absence de bugs.

## Résultats et présentation

- Dans `RecognitionResponse`, les champs protobuf 3 et 4 sont respectivement `language` et `spoken_language`, et non `result_id` et `alternative_text`. C'est une rupture d'API source Rust 0.1, sans changement des numéros wire ni de l'oracle Python figé. Voir [l'analyse des champs](rust-response-language.md).
- Le rendu explicite propose notamment `AsrOnly` et `S3ClientCompatible` ; ce dernier est un choix volontaire de compatibilité textuelle, pas une sélection de candidat Android ni une preuve de comportement serveur. Il accepte les intentions 2/3 et les membres VoiceEdit 3/7 ; sinon il se replie sur l'ASR. La provenance, l'intention et la raison éventuelle du repli sont exposées. Les contrôles de sécurité et d'unicité restent appliqués ; un edit accepté vide ne devient pas du texte ASR.
- En mode compatible, une proposition exige intention2/champ3 ou7, ou intention3/champ7 seulement. Intention3/champ3 reste rendable mais ne produit pas de proposition ; le texte seul ne prouve pas la portée du remplacement. La proposition concerne la fenêtre de contexte capturée. Aucune politique de rendu n'autorise des effets sur l'éditeur ou le système : les commandes restent sans effet automatique.
- La collecte de segments conserve les réponses comportant un conteneur3/5/7, quel que soit le state, et les marqueurs state1, dans l'ordre avec ordinal client. Les partial4 seuls restent des previews non retenus. Aucun assemblage global ni déduplication ; un candidat édité peut déjà inclure l'historique. Le state 1 seul ne prouve pas la réussite de la session.
- Les parties d'un résultat partiel sont présentées dans leur réponse, jamais concaténées entre réponses. La règle observée teste `score < 0.9` : NaN et +inf peuvent donc entrer dans le préfixe stable, -inf déclenche la partie tentative ; les scores non finis sont signalés, sans normalisation. Une confiance absente vaut 0 ; un texte absent est ignoré, alors qu'un texte vide présent participe au verrouillage de la partie tentative.

## Performance et couverture locale

Le [profil offline](rust-backend-profiling.md) compare des octets identiques et établit, pour la construction de requêtes audio mesurée sur Windows, le passage de 8 allocations et 3 réallocations à une allocation sans réallocation. C'est un gain CPU local mesuré, pas une preuve de gain WAN ou de latence de dictée distante ; les distributions du pipeline local se recouvrent et ne permettent pas de retenir un gain global robuste.

Sur l'arbre consolidé, le workspace passe **262 tests** (un oracle ignoré par défaut) sur Windows stable1.99.1 et1.88.0 et sur Linux1.88. L'exemple `session_reuse` passe4 tests, la CLI feature-off27 ; Clippy tous targets/features et le formatage passent sur Windows stable. Les builds release Windows avec/sans audio passent, dont le build normal sans audio avec un `CMAKE` délibérément inexistant. L'oracle audio indépendant passe séparément57 conteneurs ; la référence Python/outils passe656 tests en13,82s, sans modification de sa politique figée. La matrice CI couvre1.88/stable sur Windows/Linux/macOS et l'exemple explicite, mais n'a pas été exécutée à distance.

Le snapshot Linux **R5** contient les conteneurs, le validateur Ogg et les correctifs de conservation d'erreurs moteur. Il passe262 tests workspace +4 exemple +27 feature-off et le build release audio, sur WSL2 Ubuntu24.04, Rust1.88, GCC13.3 et CMake3.31.10. Ce sont des exécutables Linux ELF natifs, pas un cross-check Windows ni Android. Le premier snapshot avait expiré sur8Mio en5s ; `TCP_NODELAY` dans la fixture TCP personnalisée l'a corrigé sans augmenter le délai. Un ancien test supprimait abusivement une erreur driver et a été corrigé pour le contrat conservateur. Ces échecs intermédiaires restent distincts du succès R5. macOS n'a pas été exécuté.

## Sessions réelles et limites d'interprétation

La [preuve publique](../evidence/rust-native-session-reuse-public.json) décrit trois sessions contrôlées sur Windows, dans un même `Engine`, avec un seul appel explicite de connexion, en environ 8,3 s au total, sans retry. Les trois PCM ont été consommés jusqu'à EOF, fermés et vérifiés par hash ; les cas incluent une édition avec emoji au rendu exact et du français. Le nombre physique de connexions TCP distantes n'a pas été observé indépendamment. Les marqueurs et le hash exact démontrent les assertions déclarées, pas une qualité générale de reconnaissance.

Le [premier lot](../evidence/rust-reuse-initial-marker-failure.json), arrêté après une session, reste conservé comme échec ou résultat inconclusif : le marqueur attendu différait entre `eleven` et `11`. Il ne valide pas la réutilisation. Le lot ultérieur a déclaré cette alternative numérique avant son exécution ; il ne requalifie pas rétroactivement le premier.

### Voix naturelles sur le code consolidé

[Trois clips déjà licenciés CC BY4.0](../evidence/rust-native-natural-public.json),
deux anglais et un français (18,885s), passent dans un même moteur natif, un appel
explicite de connexion, zéro retry, stderr vide. Les PCM réellement consommés
correspondent aux hashes de provenance. Les marqueurs étaient déclarés avant
exécution ; les hashes de texte correspondent aussi aux sorties Python historiques,
comparaison rétrospective distincte des critères de passage. Source/lock/binaire
sont hashés. Le lanceur Python ne fait pas partie du runtime ; PATH enfant système.
Ce petit échantillon de livres lus ne prouve ni WER général, ni endurance, ni gain WAN.

## Ogg et conservation des erreurs

Le source fichier CLI valide progressivement le profil Ogg/Opus mono16k/v1/mapping0,
une page par chunk, sans décoder l'audio ni modifier les octets. Il contrôle CRC,
continuité/série/séquence, en-têtes, granules et EOS puis EOF physique ; paquets/Tags
limités à64Kio. Un EOS vide valide (RFC3533) a été vérifié aussi avec PyAV, et le
début d'OpusTags est exigé deuxième page (RFC7845). Le payload codec et la qualité
ne sont pas certifiés ; des paddings légaux au-delà des limites sont hors profil.

Deux pertes d'erreurs ont été reproduites avant correction : `Engine::close`
écrasait parfois une erreur driver déjà acquittée par une erreur session, et une
erreur audio primaire pouvait masquer un cleanup secondaire distinct. Le primaire
reste inchangé, le secondaire demeure disponible via fermeture explicite, même
après observation/reaping ; aucun acquittement groupé implicite. Les fermetures
annulées/concurrentes, deadline, GOAWAY réel loopback et stress de publication sont
couverts par13 nouveaux tests. Déduplication audio uniquement par identité de
l'objet original, jamais par texte/code redigé. Aucun changement d'API publique.

## Transport et limites restantes

Le [contrat de cycle de vie](rust-connection-lifecycle.md) ne prévoit ni reconnexion, ni retry, ni replay automatiques. Une erreur indépendante du driver peut être rapportée après une erreur gRPC de session, même si elles partagent éventuellement une cause. C'est un repli conservateur : l'API publique h2 n'expose pas d'identité permettant de corréler sûrement ces erreurs ; aucune déduplication exacte n'est garantie. Les scénarios testés ne prouvent pas l'absence de défauts de concurrence.

Le profil d'authentification testé fonctionne sans compte/OAuth/Zwieback, avec métadonnées applicatives injectables par session. Le provisionnement automatique, la récupération après révocation et la durabilité future de l'accès ne sont pas démontrés. Aucun comportement COMPOSE non établi n'est supposé. Les mesures et tests ne garantissent ni endurance WAN, ni comportement derrière tous les proxies, ni compatibilité macOS non exécutée.
