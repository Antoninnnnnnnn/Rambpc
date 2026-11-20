# Architecture de l’application Windows

Construire un hôte **Rust avec Tauri 2**. React/TypeScript et Vite servent aux réglages, au brouillon et à l’historique. Un contrôleur Rust suit la prise du premier échantillon jusqu’à l’unique tentative d’insertion. Pour toute reconnaissance, il appelle directement le SDK Rambler du dépôt. L’installateur et le ZIP portable exécutent ce même code.

La [recommandation multiplateforme du 13 novembre](../architecture/application-desktop-multiplateforme.md) envisageait encore de valider le choix du transport. Le SDK utilise aujourd’hui Tokio, HTTP/2 via `h2`, TLS via Rustls sur Windows et des types gRPC de tonic. L’application y accède par les API de haut niveau. Python et la CLI servent de références de conformité et de diagnostic. [Manifeste du transport](../../crates/rambler-transport/Cargo.toml) · [SDK et exemple compilé](04-integration-moteur.md)

## Les choix et leurs alternatives

| Décision | Alternatives crédibles | Motif et condition de réexamen |
|---|---|---|
| **Tauri 2, WebView2 et hôte Rust** | C# + WinUI 3 avec une façade C du moteur, Electron avec module natif ou processus compagnon, interface Rust avec egui | WinUI convient à XAML, à l’accessibilité et à un produit exclusivement Windows, au prix d’une interface interlangage à maintenir. Electron fournit son Chromium et demande une liaison Rust. egui garde un seul langage. Tauri permet ici de lier le SDK tel quel et de réaliser les écrans en HTML accessible. Pour le petit indicateur sans activation, choisir une fenêtre Win32 pilotée par Rust. Vérifier mémoire totale, Narrateur et installation sur le pilote. [Modèle Tauri](https://v2.tauri.app/concept/process-model/) · [WinUI 3](https://learn.microsoft.com/en-us/windows/apps/winui/winui3/) · [Electron](https://www.electronjs.org/docs/latest/tutorial/process-model) · [egui](https://github.com/emilk/egui) |
| **React, TypeScript et Vite** | Svelte ou TypeScript sans framework | Les réglages, la recherche et la récupération partagent assez d’états pour justifier des composants et formulaires testables. Vite produit des ressources locales utilisables sans serveur web installé. La logique de dictée reste testable en Rust. Svelte conviendrait aussi, sans résoudre de difficulté Windows supplémentaire. [Intégration Vite](https://v2.tauri.app/start/frontend/vite/) · [Exemple de Handy](02-architectures-ouvertes.md) |
| **CPAL sur WASAPI et liaisons Win32 en Rust** | WASAPI écrit directement, micro de la WebView, service Windows | CPAL donne un flux à adapter au SDK sans passer d’audio par JavaScript. Les fonctions dépendant du champ, du bureau ou des notifications utilisent les API Windows via la crate `windows`. Un seul processus natif utilisateur suffit à la V1. Descendre vers WASAPI directement pour une limite matérielle reproduite. [CPAL](https://github.com/RustAudio/cpal) · [Rust for Windows](https://github.com/microsoft/windows-rs) · [Essais matériels](05-integration-windows.md) |
| **SDK Rambler lié dans le processus** | Lancer la CLI à chaque prise, serveur HTTP local ou service Windows permanent | La liaison directe conserve annulation, segments récupérables, erreurs structurées et réutilisation de connexion. Une CLI imposerait de superviser un processus et ses sorties pouvant contenir du texte. Une API locale ajouterait un point d’entrée à sécuriser et à versionner. Un adaptateur fin suffit entre les états du SDK et ceux du bureau. [Cycle de vie du SDK](../research/sdk-integration-guide.md) |
| **SQLite via rusqlite pour l’historique, DPAPI par entrée** | JSON plat, IndexedDB dans la WebView, base chiffrée entière | Avec SQLite, ajout, purge et migration peuvent être transactionnels. Le texte est protégé avant d’atteindre SQLite. Les préférences non sensibles tiennent dans un fichier local versionné. Le code natif est seul propriétaire des écritures. Réévaluer le chiffrement intégral si l’on veut aussi masquer volume et dates ou indexer de grands historiques. [rusqlite](https://github.com/rusqlite/rusqlite) · [Schéma et recherche](06-confidentialite-et-continuite.md) |

## Où placer le code

Arborescence **à créer** par lots. Aujourd’hui, le seul fichier d’interface dans `app/` est le guide d’intégration. Un seul paquet natif suffit au début. Les frontières ci-dessous peuvent être des modules.

~~~text
app/
  src/                  écrans React, formulaires, annonces accessibles
  package.json          Vite, TypeScript, outils de développement et de test
  src-tauri/
    Cargo.toml          paquet Windows et workspace Cargo de l’application
    capabilities/       commandes permises à chaque fenêtre locale
    src/
      session/          machine à états, temporisations et brouillon
      rambler/          adaptation du SDK unique, segments et issues
      text/             profils, lexique, expressions et rendu déterministe
      windows/          capture, raccourci, champ actif, écriture, DPAPI
      storage/          répertoire du profil, réglages, historique et purge
      lib.rs            commandes Tauri et assemblage des composants
    tests/support/      réponses contrôlées et erreurs pour les tests
crates/                 SDK existant et CLI de référence
~~~

Choisir un **workspace Cargo autonome dans `app/src-tauri/`**, avec sa table `[workspace]` et son `Cargo.lock`. Déclarer les dépendances par chemins `../../crates/rambler-engine`, `../../crates/rambler-audio` et `../../crates/rambler-transport`. Garder des versions compatibles de Tokio et des types qui traversent l’interface. Comparer les résolutions avec `cargo tree -d` et les versions qualifiées dans le verrou du SDK. Un workspace commun, décrit dans le guide générique, ajouterait l’interface aux commandes de la CI du moteur sur plusieurs systèmes. Un travail CI Windows distinct et un verrou npm donnent à l’application son propre cycle de qualification. [Guide générique](../../app/README.md) · [Manifeste racine](../../Cargo.toml) · [Matrice réelle](../../.github/workflows/rust.yml) · [Workspaces Cargo](https://doc.rust-lang.org/cargo/reference/workspaces.html)

## Qui possède quoi

~~~mermaid
flowchart LR
  H[Raccourci ou bouton] --> C[Contrôleur Rust]
  C --> M[Capture Windows]
  M --> Q[File PCM bornée]
  Q --> A[Adaptateur du SDK Rambler]
  A -->|audio| R[Rambler/Jetson de Google]
  R -->|réponses| A
  A -->|segments admis et issue| C
  C -->|si cible admissible| I[Insertion Windows]
  C -->|si récupération| B[Brouillon temporaire]
  C -->|si consentement| D[Historique local]
  C -->|état et vue demandée| U[Interface Tauri]
~~~

Le raccourci et la fenêtre envoient des intentions au **même** contrôleur. Le bouton « Dicter ici » de Sambler crée un brouillon interne. Seul un déclenchement qui a identifié le champ externe peut armer une insertion. Ouvrir les réglages désarme la cible comme tout changement de champ. Un seul processus par session utilisateur possède le micro et les raccourcis. Relancer le même profil réaffiche sa fenêtre. Si l’autre édition est déjà ouverte avec un profil différent, indiquer laquelle fermer avant de démarrer. Chaque édition conserve son propre répertoire de données. [Instance unique Tauri](https://v2.tauri.app/plugin/single-instance/) · [Coexistence des éditions](09-distribution-et-version-portable.md)

L’adaptateur reçoit la langue et un flux PCM mono normalisé. Le SDK encode en Opus, transmet à Rambler et rend des segments et une issue typée. La session fige le profil Naturel ou Fidèle et la version des règles de texte au départ. Les règles s’appliquent sur le texte admis, hors du callback audio. Un retrait de l’autorisation d’envoi interrompt immédiatement la prise. Si Rambler devient indisponible, le contrôleur libère la capture, explique l’incident et rend les segments déjà acquis dans le brouillon. Une nouvelle action de dictée ouvre la prise suivante. Des doubles et un serveur en boucle locale injectent réponses tardives, segmentation et refus pour les tests. [Contrat détaillé](04-integration-moteur.md) · [Coupures et reprise](06-confidentialite-et-continuite.md)

## Un contrôleur pour arbitrer les événements

| État | Événement et transition | Effet autorisé |
|---|---|---|
| Disponible | Déclenchement → Préparation | Attribuer un identifiant, vérifier l’autorisation d’envoyer à Rambler, figer langue, micro, profil et règles, qualifier le champ. Une prise ordinaire succède à la précédente sans dialogue. Si un échec a laissé du texte en attente de récupération, proposer de le copier ou d’accepter son remplacement. |
| Préparation | Moteur prêt et premières trames acceptées → Écoute | Annoncer que la personne peut parler. Un second appui, une annulation, un refus de permission ou un délai dépassé libèrent les ressources, y compris un micro qui s’ouvrirait tardivement. |
| Écoute | Fin volontaire ou limite de durée → Finalisation | Fermer la source après ses dernières trames. Un changement de champ ordinaire désarme l’insertion pour cette prise, même si le premier champ reprend le focus. Un verrouillage ou un champ protégé identifié interrompt la prise. |
| Finalisation | Fin propre avec texte admissible → Texte disponible | Vérifier que tous les segments attendus sont présents et admis. Si le contrôle de cible tient encore, consommer le droit à l’unique tentative d’écriture **avant** d’appeler Windows. Sinon, proposer le brouillon. |
| Texte disponible | Retour Windows ou action explicite dans Sambler | Un nombre complet d’événements injectés atteste leur mise en file. Certains éditeurs ne permettent pas de confirmer le texte affiché. Un doute laisse accessible la copie manuelle. Un échec d’historique ne redéclenche jamais l’insertion. |
| Tout état actif | Annuler, échec ou interruption | Annuler invalide d’abord l’identifiant et efface le brouillon de la prise. Un incident ordinaire laisse les seuls segments déjà admis en récupération. Verrouiller ou quitter efface le brouillon. Une réponse tardive porte un identifiant périmé. |

Utiliser un seul propriétaire des transitions, par exemple une tâche Tokio alimentée par une file bornée. Les commandes « Terminer », « Annuler » et « Quitter » doivent rester délivrables même si des mises à jour visuelles s’accumulent. Le niveau du micro se remplace par la valeur la plus récente. Le callback audio, la boucle de messages Win32 et la WebView restent disponibles pendant le traitement réseau ou disque. Toute ressource démarrée par une prise est fermée ou rejointe sur chacun de ses chemins de sortie. [Contrat du callback](../research/rust-pcm-capture.md) · [Limites de SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)

Le choix de conservation se revérifie au moment d’écrire. Une désactivation ou un effacement intervenus pendant la finalisation priment sur l’autorisation mémorisée au départ. Le même identifiant sert de clé d’idempotence locale pour un enregistrement d’historique. Après redémarrage, relire les réglages et les entrées valides sans rejouer de dictée ni d’insertion. [Cycle de vie des données](06-confidentialite-et-continuite.md)

## Contrat entre interface et code natif

| Surface proposée | Ce qui la traverse |
|---|---|
| Commandes de la fenêtre principale | Démarrer un brouillon interne, terminer ou annuler une prise identifiée, lire l’état, changer le micro, la langue ou le profil, gérer les expressions et le lexique, ouvrir le brouillon, copier ou supprimer une entrée par identifiant. |
| Événements adressés à la vue concernée | Identifiant éphémère, phase, niveau borné, disponibilité d’un résultat, catégorie d’échec. Le texte ne traverse l’interface qu’à la consultation de la vue qui doit l’afficher. La dernière phase peut toujours être redemandée après réouverture. |
| Capacités réservées au natif | Capture, audio, accès à Rambler, désignation d’un champ tiers, décision d’insérer, stockage protégé par DPAPI et ouverture de liens de support préétablis. La WebView ne choisit ni un handle ni une chaîne arbitraire à taper dans un autre logiciel. |

Déclarer les commandes d’application dans `AppManifest::commands` et attribuer les permissions par fenêtre locale. L’enregistrement d’une commande via `invoke_handler` la rend sinon utilisable par les fenêtres de l’application par défaut. Restreindre la navigation et les ressources par CSP, afficher les dictées comme texte et réserver l’accès aux données à la fenêtre principale. Un indicateur natif n’a besoin d’aucune commande WebView. [Capacités Tauri](https://v2.tauri.app/security/capabilities/) · [Politique de contenu](https://v2.tauri.app/security/csp/) · [Canaux et événements](https://v2.tauri.app/develop/calling-frontend/)

## Stockage, installation et mise à niveau

Résoudre le répertoire de données **avant la première WebView**. L’édition installée utilise le profil de l’utilisateur. Le ZIP extrait sélectionne `Data/` grâce à `portable.flag` à côté de l’exécutable, indépendamment du dossier de travail. Y affecter aussi le dossier de données de WebView2. Sur support non inscriptible, proposer le mode éphémère et afficher ses limites. Les réglages ordinaires sont versionnés et écrits atomiquement. Un lexique ou une expression peut révéler un patronyme ou une signature. Protéger ces valeurs et les transcriptions de l’historique via DPAPI en portée utilisateur. Un profil transféré garde sa langue et son raccourci même si ses contenus confidentiels sont illisibles sous le nouveau compte. [Données du portable](06-confidentialite-et-continuite.md) · [Essais du profil](09-distribution-et-version-portable.md)

Un build Windows x64 produit les deux éditions depuis le même tag. **L’installateur NSIS par utilisateur et le ZIP portable** offrent les mêmes fonctions de dictée. Le ZIP s’appuie sur WebView2 Evergreen présent sur le poste. Le poste de construction utilise Rust/MSVC, CMake, Node et les outils de paquetage. Réserver MSI/MSIX à un besoin de déploiement en parc. [Options Tauri](https://v2.tauri.app/distribute/windows-installer/) · [Distribution WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) · [Chaîne Opus du dépôt](../../crates/rambler-audio/BUILDING.md)

Distribuer les contributions originales sous MIT et préparer GitHub Releases à budget nul. Pour la V1, la mise à niveau est manuelle après fermeture de Sambler. Lancer le nouvel installateur ou extraire le ZIP dans un autre dossier et y transférer le profil. Vérifier les migrations, l’échec de mise à niveau et le choix d’effacement. Documenter le statut réel de la signature et publier les empreintes SHA-256 des artefacts finaux. Une signature Authenticode gratuite dépend de l’éligibilité du projet. [Distribution et recette](09-distribution-et-version-portable.md) · [Licence du dépôt](../../LICENSE.md)
