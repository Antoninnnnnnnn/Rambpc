# Démarrer l’application Rambpc

Ce dossier accueille la future application. Le moteur de dictée est la bibliothèque Rust dans `crates/`. L’outil en ligne de commande et les tests restent utilisables depuis la racine du dépôt.

## Vérifier le moteur

Depuis la racine du dépôt, avec [Rust, CMake et le compilateur natif](../crates/rambler-audio/BUILDING.md) installés, lance ce test avec un fichier audio généré et un serveur simulé sur ta machine.

```sh
cargo fetch --locked
cargo test --locked --offline -p rambler-cli --features audio --example minimal_app
```

## Brancher la future application

Si le `Cargo.toml` de l’application se trouve dans `app/`, ajoute `app` aux `members` du `Cargo.toml` racine et utilise ces dépendances.

```toml
[dependencies]
rambler-engine = { path = "../crates/rambler-engine" }
rambler-transport = { path = "../crates/rambler-transport" }
rambler-audio = { path = "../crates/rambler-audio" }
bytes.workspace = true
tokio.workspace = true
```

Avec un manifeste Rust dans `app/src-tauri/`, le membre devient `app/src-tauri` et les chemins commencent par `../../crates/`.

Pour le premier écran, vise **Démarrer → Terminer → afficher le texte → Copier**. Utilise d’abord un WAV, comme dans [l’exemple Rust](../crates/rambler-cli/examples/minimal_app.rs), puis branche le micro. L’app fournit de l’audio PCM signé 16 bits, mono, à 16 ou 48 kHz. Le moteur l’encode en Opus. Termine la source quand l’utilisateur arrête de dicter. Traite séparément l’annulation, les erreurs et les aperçus pendant la dictée. [Guide du SDK](../docs/research/sdk-integration-guide.md)

## Ce que vérifie GitHub

| Plateforme | Vérification du moteur |
| --- | --- |
| Windows, Linux et macOS | Suites Rust hors ligne et compilation de la CLI avec Opus. macOS couvre Intel et Apple Silicon. |
| Android | Édition de liens ARM64 avec Opus et contrôle de l’alignement 16 Kio du binaire d’exemple. L’app Android initialise [la vérification TLS native](../crates/rambler-transport/ANDROID_TLS.md) au démarrage. |
| iOS | Édition de liens ARM64 pour iPhone et tests locaux sur simulateur Apple Silicon, dont le rejet d’un certificat non approuvé. |

Le micro, les interruptions de capture et l’insertion dans une autre application seront à tester avec la future app sur de vrais appareils. [Workflow Rust](../.github/workflows/rust.yml)
