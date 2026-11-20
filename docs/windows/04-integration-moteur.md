# Brancher le microphone Windows sur le moteur Rust

**Sambler utilise un seul moteur de dictée, Rambler/Jetson de Google, via les crates Rust du dépôt.** En V1, la personne choisit le rendu **Naturel** (par défaut) ou **Fidèle** dans les réglages. Les deux utilisent le même flux de reconnaissance et le même collecteur segmenté. L’application possède le micro, le brouillon et l’autorisation d’insérer. Le SDK encode le son, conduit les échanges et interprète les réponses. L’objectif de cinq minutes par prise reste à qualifier sur Windows. [Point d’entrée des applications](../../app/README.md) · [Guide du SDK](../research/sdk-integration-guide.md)

## Ce qui fonctionne déjà

| Étape | Ce que montrent le code et les essais du dépôt |
| --- | --- |
| PCM → Opus → résultat | Le 18 novembre 2026, la CLI native a transcrit des WAV synthétiques en français et en anglais auprès du service distant. Deux longs flux uniques ont échoué vers 300 s sans texte final. Ces essais sur une machine et un réseau motivent la segmentation. La qualité au micro et la disponibilité future restent à mesurer. [Campagne réelle](../evidence/rust-native-real-campaign-20261118.json) · [Endurance et limites](../evidence/rust-native-real-reliability-20261118.json) |
| Perte, annulation et texte partiel | Le serveur local de test peut retarder ou refuser le dernier résultat. Les essais vérifient les plages reçues, la conservation des segments précédents et le retrait du texte bloqué. Le test combiné du 19 novembre couvre un WAV à 48 kHz avec une plage inutilisable. [Livraison des segments](../research/recoverable-segment-delivery.md) · [Essai Windows](../evidence/interactive-partial-wav48-windows-20261119.md) |
| Accès | Le 19 novembre, le service a accepté une connexion de test et refusé la dictée dépourvue d’authentification applicative. La disponibilité publique du produit dépend d’un accès Rambler autorisé pour Sambler. Une connexion établie ne suffit pas à annoncer que la dictée fonctionne. [Audit du 19 novembre](../evidence/rust-integration-outcomes-windows-20261119.md) · [État du moteur](../STATUS.md) |
| Bureau Windows | L’exemple `minimal_app` lit un WAV et utilise un éditeur simulé. L’ouverture du micro, les raccourcis, la cible au premier plan, l’insertion et le stockage de l’historique appartiennent à la future application. [Exemple compilé](../../crates/rambler-cli/examples/minimal_app.rs) · [Tests de l’exemple](../../crates/rambler-cli/examples/minimal_app/tests.rs) |

## La frontière avec l’application

Créer pour l’application son propre workspace Cargo dans `app/src-tauri/`, avec sa table `[workspace]` et son `Cargo.lock`. Référencer `rambler-engine`, `rambler-audio` et `rambler-transport` par `../../crates/…`. Déclarer explicitement un Tokio compatible avec celui du SDK, puis vérifier la résolution effective avec `cargo tree --manifest-path app/src-tauri/Cargo.toml -d`. Ce choix garde les dépendances de bureau Tauri dans la CI de l’application. Le guide général décrit aussi l’option d’un workspace commun. [Workspace du moteur](../../Cargo.toml) · [Guide des applications](../../app/README.md)

Garder la CLI comme outil d’acceptation. La lancer pour chaque phrase imposerait un contrat supplémentaire sur stdout, la supervision d’un processus et une nouvelle connexion. Opus est déjà intégré statiquement et ses notices accompagnent les binaires distribués. [Compilation et licences](../../crates/rambler-audio/BUILDING.md) · [Notices](../../crates/rambler-audio/THIRD_PARTY_LICENSES)

| Ce que l’application demande | Responsabilités du contrôleur et de l’adaptateur Rambler |
| --- | --- |
| Démarrer | Une seule dictée active. Affecter un identifiant, mémoriser la cible sur le poste et figer langue, profil de rendu et règles de texte pour la prise. Vérifier les choix d’envoi et d’archivage. Retirer l’autorisation d’envoi coupe immédiatement la capture et la transmission. Désactiver l’historique empêche l’écriture encore en attente. [Règles de confidentialité](06-confidentialite-et-continuite.md) |
| Suivre | Tout événement porte l’identifiant de prise. Un aperçu éventuel remplace le précédent. Un segment terminé porte un index, une plage et un résultat interprété. Le contrôleur conserve en RAM seulement le texte admis et les statuts nécessaires au brouillon. |
| Terminer | Clore proprement la capture et attendre le résultat de l’ensemble. Le contrôleur est seul à pouvoir demander une insertion, une fois, si l’intégralité du texte est admissible et si la cible est encore valide. Une seconde notification de fin reste sans effet. |
| Annuler | Invalider immédiatement l’identifiant, retirer le brouillon et l’aperçu, arrêter le producteur et abandonner la collecte. Un événement tardif de cette prise est ignoré, même après le démarrage d’une nouvelle dictée. |

L’adaptateur publie du texte et des états. Les boutons « Copier », « Insérer » et les écritures d’historique appartiennent au contrôleur Windows. Les doubles de test et le serveur en boucle locale servent uniquement à reproduire les réponses et les incidents pendant le développement.

## Le chemin audio

1. **Préparer le moteur.** Sur un runtime Tokio vivant, appeler `Engine::connect_ready(transport_config)` une fois l’accès Rambler configuré et le traitement par Google accepté. Réutiliser cet `Engine` pour les prises suivantes. Le contrôle initial établit la connexion et attend un aller-retour HTTP/2. Une dictée peut encore être refusée ensuite. Pour chaque prise, créer un `InteractivePcmProfile::default()`, renseigner `session.config.language` et confier les données d’accès autorisées à `session.credentials` via `Arc<dyn MetadataProvider>`. Elles restent du côté natif. L’identité de recherche du client dans `ProtocolConfig::default()` ne constitue pas une identité de distribution. [Cycle de connexion](../research/sdk-integration-guide.md#warm-the-connection-before-recording) · [Options de session](../../crates/rambler-engine/src/engine.rs) · [Configuration](../../crates/rambler-protocol/src/request.rs)
2. **Adapter le format à la frontière Windows.** Fournir du PCM signé 16 bits, petit-boutiste, mono à 16 ou 48 kHz. À 16 kHz, passer directement la source au collecteur. À 48 kHz, placer **un seul** `Pcm48kTo16k::new(input)` avant le collecteur pendant toute la prise. Le collecteur produit ses propres flux Ogg/Opus. Pour un micro en flottants, stéréo ou à 44,1 kHz, l’adaptateur Windows doit négocier un format pris en charge ou convertir explicitement avec un état continu. En cas d’échec, afficher le format et proposer un autre périphérique. [Surfaces audio publiques](../../crates/rambler-audio/src/lib.rs) · [Essai de continuité à 48 kHz](../../crates/rambler-cli/tests/wav.rs)
3. **Armer la file avant les callbacks.** `capture_channel(CaptureConfig { max_chunks, max_chunk_bytes })` fournit un `CaptureSender` unique et un `CaptureInput`. Le callback série appelle `offer(&pcm16le)` avec des blocs complets, non vides. La file est préallouée. À titre d’essai, 300 emplacements de 1 920 octets contiennent 6 s à 48 kHz mono si chaque bloc fait 20 ms. Les blocs courts utilisent aussi un emplacement. Un débordement est définitif pour la prise. Arrêter la production, signaler la perte, conserver les segments déjà confirmés. Mesurer la taille réelle des callbacks et l’occupation maximale lors d’une finalisation lente avant de fixer la capacité. `pcm_channel()` et son `reserve().await` conviennent au producteur WAV qui peut attendre. [Contrat de capture](../research/rust-pcm-capture.md) · [Implémentation](../../crates/rambler-audio/src/capture.rs)
4. **Collecter pendant que le micro produit.** Utiliser `stream_long_pcm_dictation`, ou sa variante `_with_observer` si l’aperçu est activé. Le profil interactif limite un segment à 120 s d’audio ou de temps écoulé. Durant les 5 dernières secondes, il cherche un bloc de faible énergie. Une coupure peut partager un mot. Chaque RPC dispose de 135 s au total et de 5 s de finalisation à partir de la fin propre de la source encodée. La capture peut continuer dans la file pendant cette finalisation. Le profil brut `SessionOptions::default()` a une échéance de 30 s et `LongDictationOptions::default()` une durée de segment de 240 s. Utiliser le profil interactif tel quel pour l’essai V1. [Profil et délais](../../crates/rambler-audio/src/long_dictation.rs) · [Méthode de mesure](../research/interactive-pcm-dictation.md)
5. **Surveiller le micro jusqu’à l’arrêt.** Entre deux segments, le SDK attend la prochaine trame PCM sans échéance de RPC. Horodater l’arrivée des trames au producteur, indépendamment du niveau sonore. Borner l’ouverture du micro puis essayer une seconde sans trame après le démarrage, à ajuster selon la cadence mesurée des pilotes. Un bloc silencieux compte comme une trame reçue. Si le délai expire, y compris au moment du relâchement, signaler une interruption, arrêter la capture via `abort()` et proposer les seuls segments admis en récupération. [Attente du collecteur](../../crates/rambler-audio/src/long_dictation.rs#L208) · [Arrêt du producteur](../../crates/rambler-audio/src/capture.rs#L222)
6. **Terminer sur la même prise.** Au relâchement normal ou à la limite annoncée de 300 s, arrêter les nouveaux callbacks, offrir la dernière tranche et appeler `CaptureSender::finish()` une seule fois. Attendre le retour du collecteur. S’il échoue pendant que le producteur attend le micro, arrêter et rejoindre le producteur. [Contrat du producteur](../research/sdk-integration-guide.md#2-feed-ordered-caller-audio) · [Arrêt et annulation](../research/recoverable-segment-delivery.md)

Exemple d’adaptation pour une source **déjà normalisée en mono PCM16LE à 48 kHz**. Le contrôleur prépare `profil` avec sa langue et son accès. Il pilote simultanément `CaptureSender` et cet appel. `livrer` s’exécute dans la tâche de collecte, en ordre, avant l’ouverture du segment suivant.

```rust
use rambler_audio::{
    CaptureInput, CompletedDictationSegment, InteractivePcmProfile,
    Pcm48kTo16k, stream_long_pcm_dictation,
};
use rambler_engine::{CollectorLimits, Engine, EngineError};

async fn dicter_48k(
    moteur: &Engine,
    pcm: CaptureInput,
    profil: InteractivePcmProfile,
    livrer: &mut (dyn FnMut(CompletedDictationSegment) + Send),
) -> Result<(), EngineError> {
    stream_long_pcm_dictation(
        moteur,
        Pcm48kTo16k::new(pcm),
        profil.session,
        CollectorLimits::default(),
        profil.segmentation,
        livrer,
    ).await
}
```

Le consommateur projette le segment, puis libère ses candidats bruts. Garder ce travail synchrone court, sans attente du disque ni de l’interface. Une limite de mémoire ou une file de résultats saturée doit marquer la prise incomplète et empêcher l’insertion d’un texte tronqué. Les limites SDK par défaut de 128 réponses finales et 8 Mio de données sérialisées s’appliquent **par RPC**. L’application borne aussi la taille totale de son brouillon. [Collecteur](../../crates/rambler-audio/src/long_dictation.rs) · [Limites de rétention](../../crates/rambler-engine/src/results.rs)

## Du résultat moteur au texte insérable

Le niveau du micro suffit comme retour immédiat en V1. Si l’aperçu textuel est retenu ensuite, `PartialTranscript::from_result` expose un préfixe et un suffixe qui peuvent changer. Remplacer l’aperçu entier à chaque réponse, le limiter à la surimpression locale et l’effacer à la fin du segment, à l’annulation ou à l’erreur. Cette projection provisoire ne bénéficie pas du contrôle appliqué au texte final. [Présentation](../../crates/rambler-engine/src/presentation.rs) · [Observation du flux](../research/sdk-integration-guide.md#3-observe-preview-and-progress-without-taking-over-the-collector)

Une réponse ressemblant à un résultat final peut encore être suivie d’une erreur. Le callback de `CompletedDictationSegment` livre une session terminée après fin de l’envoi, statut gRPC réussi, fin applicative et nettoyage. Conserver `(identifiant de prise, segment_index, sample_range, issue)`. Les indices commencent à zéro et les plages `[début, fin)` comptent les échantillons **à 16 kHz**, après conversion. La clé `(prise, index)` permet d’écarter les doublons. [Livraison garantie par le collecteur](../research/recoverable-segment-delivery.md)

### Choisir le rendu avant le premier échantillon

Le réglage appartient à Sambler. Les deux modes transmettent le même audio à Rambler sans contexte d’éditeur. Ils utilisent **une seule interprétation** par segment et préservent les contrôles du SDK sur la fin de session, l’unicité et le statut de sécurité. Le rendu Naturel exploite le texte déjà retourné et des règles de typographie déterministes sur le poste. Il n’appelle aucun modèle de langage supplémentaire. [Politiques du SDK](../../crates/rambler-engine/src/results.rs) · [Cas d’essai](../../crates/rambler-engine/tests/explicit_results.rs)

| Mode dans les réglages | Politique fixée au démarrage de la prise | Texte admissible |
| --- | --- | --- |
| **Fidèle** | `interpret(RenderPolicy::AsrOnly)` | Conserver `UsableText` de provenance `HypothesisText` tel que Rambler l’a reconnu. Garder les répétitions et la ponctuation présentes dans ce texte. La reconnaissance peut déjà normaliser une heure, ponctuer ou omettre une hésitation. « Fidèle » décrit le traitement de Sambler, sans garantie de verbatim absolu. |
| **Naturel avec nettoyage léger, par défaut dès la V1** | Choisir `AsrOnly` **avant la prise**, puis appliquer au texte admis les règles de ponctuation, d’espacement et de casse activées. | Par exemple, corriger les espaces autour des signes de ponctuation selon la langue dans la prose ordinaire. Préserver le contenu des URL, des adresses et des nombres. Nom propre, négation ou répétition volontaire restent intacts. Si une règle n’est pas applicable avec certitude, garder le passage reçu. |
| **Naturel avec nettoyage avancé, après qualification du rendu Rambler** | Si la personne active cette option pour une langue et une version qualifiées, choisir `interpret(RenderPolicy::Strict)` avant la prise. Le nettoyage léger reste le réglage initial. | `HypothesisText` reste de la reconnaissance. Admettre `VoiceEditText` uniquement pour une forme précise éprouvée comme **texte neuf** en dictée sans contexte. Étudier en premier l’intention 2 portant le texte édité de champ 3. Une sortie marquée `UsableText` et `VoiceEditText` exige aussi cette vérification de portée. |

La forme `VoiceEditContext` représente une fenêtre d’édition entière. L’insérer au curseur pourrait dupliquer ou altérer le document. La classer « À revoir » et exclure son texte de l’ajout automatique, y compris si `Strict` a renvoyé `UsableText`. Une forme de réécriture non qualifiée suit le même traitement. Une réponse bloquée, ambiguë ou sans texte ne déclenche **aucune seconde projection** et aucune retransmission. Le choix provisoire d’`AsrOnly` en mode Naturel se fait pour toute la prise avant son démarrage. [Rendu et provenance](../../crates/rambler-engine/src/results.rs) · [Fenêtre éditée et opérations d’éditeur](../research/edit-acceptance-contract.md)

### Assembler le résultat de la prise

| Issue du SDK | Traitement par Sambler |
| --- | --- |
| `UsableText` | Conserver le texte exact de chaque plage admise. Une fois la capture complète et la collecte terminée par `Ok(())`, assembler les plages avec `join_segment_texts`. Une prise dont tous les segments et leurs jonctions sont admissibles peut produire une seule insertion. |
| `NoResult` ou `NeedsReview` | En `AsrOnly`, une hypothèse ASR absente ou vide donne « Aucun texte obtenu » pour cette plage, même si Rambler a envoyé une proposition d’édition. Une ambiguïté donne une plage à revoir. Garder les autres passages admis dans l’ordre et permettre leur copie individuelle. La présence de parole demeure inconnue et toute lacune écarte l’insertion automatique. |
| `SafetyBlocked` ou `UnrecognizedSafety` | Rendre visible le statut, masquer les candidats concernés et arrêter l’insertion d’ensemble. Le contenu refusé reste également hors du brouillon et des journaux. |
| `VerifiedDeletion` ou issue future inconnue | Écarter l’effet sur le document dans les deux modes. Une demande de suppression relève d’un parcours d’édition avec cible et portée vérifiables. |
| Aucun segment | Indiquer « Aucun texte obtenu ». Ne produire aucune action dans la cible. |

`join_segment_texts` choisit l’espacement à partir des fragments. Dans le test du SDK, `3.` suivi de `14` donne `3. 14`. Si deux fragments numériques peuvent appartenir au même nombre et que la séparation reste ambiguë, conserver les fragments exacts dans le brouillon pour relecture. Sur un texte complet admis, appliquer une seule fois les expressions, le lexique et la typographie activés. Empêcher toute expansion récursive ou à travers une lacune. [Cas chiffrés du SDK](../../crates/rambler-audio/src/segment_text.rs#L346) · [Règles personnelles](03-produit-et-priorites.md)

En cas d’échec ultérieur, les plages déjà livrées restent récupérables, avec les lacunes indiquées. Une erreur de transport, d’accès ou de nettoyage de session garde sa catégorie même si une partie du texte est utilisable. Conserver au plus un brouillon en mémoire pour la durée choisie, dix minutes par défaut et au maximum. Fermer le brouillon, verrouiller Windows, quitter Sambler ou annuler explicitement l’efface. L’historique facultatif accueille uniquement les résultats complets, sept jours par défaut après activation. [Essai de résultats mixtes](../../crates/rambler-cli/tests/interactive_partial.rs) · [Jonction de texte](../../crates/rambler-audio/src/segment_text.rs)

**Comment qualifier le texte neuf en français et en anglais.** Préparer des phrases de référence, sans sélection ni texte d’éditeur transmis. Inclure un courriel avec négation, une répétition voulue, une hésitation, une heure, un nom propre et une instruction citée comme « envoie le message ». Faire relire les rendus à partir de paroles consenties. Relever pour chaque mode et langue les prises utilisables, manquantes, ambiguës, refusées et les provenances. Pour le rendu édité candidat, vérifier qu’il contient uniquement la nouvelle dictée, conserve le sens et n’ajoute aucune instruction ou fenêtre d’éditeur. Une variante qui échoue reste hors de la liste admissible. La campagne publiée montre une dictée française rendue par `VoiceEditText`. Elle ne suffit pas à qualifier une famille de réponses ni la couverture de `Hypothesis.text`. [Campagne datée](../evidence/rust-native-real-campaign-20261118.json) · [Matrice de cas](../../crates/rambler-engine/tests/explicit_results.rs) · [Essais de composition](../research/composition.md)

## Coupure, confidentialité et édition avancée

- **Incident temporaire.** Distinguer réseau indisponible, délai dépassé, limitation et accès refusé. Arrêter le micro et conserver seulement les passages déjà admis. L’audio de la prise interrompue n’est ni mis en attente ni renvoyé automatiquement. Si la connexion est morte, la fermer et créer une nouvelle connexion Rambler pour la prochaine prise explicitement déclenchée. Une phrase perdue peut être redictée quand le service répond. [Test de coupure TCP](../../crates/rambler-engine/tests/app_tcp_recovery.rs) · [Livraison des segments précédents](../research/recoverable-segment-delivery.md)
- **Perte durable de l’accès.** Afficher « Dictée indisponible », laisser le micro fermé et maintenir l’accès aux réglages, à l’historique consenti et au brouillon tant qu’il est conservé. Suspendre l’annonce de disponibilité. Une reprise demande un accès Rambler autorisé pour Sambler et une qualification du parcours réel de bout en bout. Un double de test ou un simple contrôle de connexion ne valide pas ce retour. [État de l’accès](../STATUS.md) · [Continuité et confidentialité](06-confidentialite-et-continuite.md)
- **Cycle de vie.** À l’annulation, le contrôleur sort de la portée qui possède la future de collecte et arrête le producteur. Pour une collecte lancée avec `tokio::spawn`, appeler `abort()` puis attendre le `JoinHandle`. À l’arrêt de l’application, rejoindre les producteurs, libérer les collectes, attendre `Engine::close()` et traiter les échecs de nettoyage avant d’arrêter Tokio. La fermeture du moteur est définitive. [Moteur et sessions](../../crates/rambler-engine/src/engine.rs) · [Supervision de la CLI](../../crates/rambler-cli/src/main.rs)
- **Données transmises.** En modes Naturel et Fidèle, audio encodé, langue, métadonnées du client et accès autorisé quittent le poste pour Rambler/Jetson. Pour la V1, `ContextInput::None`, `trigger_application_id = None`, `bias_terms` et `preferences` vides. Le collecteur segmenté exige ce contexte vide. Garder texte, audio, contenu de l’éditeur et identifiants d’accès hors des diagnostics. La CLI possède `--diagnostics-only` pour tester cette frontière. [Options exposées](../../crates/rambler-protocol/src/request.rs) · [Tests des diagnostics](../../crates/rambler-cli/tests/diagnostics_output.rs)
- **Réécriture ultérieure.** Une édition de sélection exige un parcours dédié à une session, consentement à la transmission, révision capturée, identité du document, ancre et sélection. Avant toute modification, revalider ces éléments dans la même transaction que l’écriture. `replacement_proposal_with_policy` et `derive_selection_splice` permettent de borner la modification en UTF-16. Lorsque l’éditeur tiers ne fournit aucune telle transaction, présenter le texte à relire et à copier. Une observation distante de réécriture sur une phrase ne qualifie pas toutes les commandes vocales. [Contrat d’édition](../research/edit-acceptance-contract.md) · [Exemple sur texte enrichi](../../crates/rambler-cli/examples/minimal_app/tests.rs)

## Vérifier le branchement sans service distant

Depuis la racine, avec les dépendances de `Cargo.lock` en cache, Rust, CMake et les outils C++ MSVC du projet. Pour un premier poste en ligne, `cargo fetch --locked` prépare le cache de dépendances. Les essais suivants utilisent des entrées synthétiques et, lorsqu’une connexion est nécessaire, un serveur local contrôlé. [Préparation Windows](../../crates/rambler-audio/BUILDING.md) · [Manifeste des tests et exemples](../../crates/rambler-cli/Cargo.toml)

```powershell
cargo test --locked --offline -p rambler-cli --features audio --example minimal_app
cargo test --locked --offline -p rambler-audio --test capture_handoff
cargo test --locked --offline -p rambler-cli --features audio --test capture_rollover_delivery --test interactive_48k_partial
cargo test --locked --offline -p rambler-engine --test app_tcp_recovery --test explicit_results
```

**Validation historique du SDK.** Le 20 novembre 2026, sur Windows x64 à la révision `3683b574c9a8`, les **28 tests** de ces cibles avaient réussi et l’extrait `dicter_48k` avait compilé avec `rustdoc`. Les essais applicatifs des deux modes sur de vrais micros et éditeurs restent à mener.

**Recette du futur adaptateur.** En boucle locale de test, simuler une réponse tardive pour « A », annuler et commencer « B ». La cible doit recevoir « B » exactement une fois. Tester séparément une coupure après le premier segment, une lacune au milieu et un `VoiceEditContext` inattendu en mode Naturel. Vérifier la récupération des passages admis et zéro insertion automatique dans ces trois cas. [Test de livraison segmentée](../../crates/rambler-cli/tests/capture_rollover_delivery.rs) · [Tests du rendu](../../crates/rambler-engine/tests/explicit_results.rs)

**Qualification terrain.** Tester les dictées courtes puis 30, 121 et 300 s sur de vrais micros. Répartir les cas entre un brouillon Outlook, un document Word, des champs Edge et Chrome et un prompt d’IA dans VS Code. Le Bloc-notes sert de témoin. Consigner l’application, le type de champ, la langue, le mode et le nombre d’échecs. Viser **au moins 90 dictées utilisables sur 100**. Pour les phrases de 3 à 10 s à chaud, viser un **p95 de 2 s au plus entre le geste d’arrêt et le texte visible dans le champ attendu**. Chronométrer séparément dernier échantillon, dernier segment livré et apparition dans le champ. Les 5 s de finalisation d’un RPC démarrent à la fin propre du flux encodé. La prise de cinq minutes et chaque famille d’éditeurs sont annoncées après qualification réelle. [Recette Windows](08-plan-de-construction.md) · [Mesure du moteur](../research/interactive-pcm-dictation.md#independent-sdk-timing-seam)
