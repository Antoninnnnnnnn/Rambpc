# Options RecognizerInfo S3 : présence explicite, sans promesse serveur

Implémentation initiale sur `fdc9fd9`, vérifiée offline le 2026-11-14,
puis intégrée à la CLI et au runner natif. L'init S3 utilise l'extension **294500**.
Aucune exécution d'APK, aucun nouveau backend ni changement des politiques de
résultat. La vérification distante complémentaire est décrite séparément ci-dessous.

## API et encodage

Champs publics de `ProtocolConfig`, utilisables avec `..Default::default()` :

| Champ Rust | Champ RecognizerInfo | `None` | Valeurs explicites |
|---|---|---|---|
| `block_offensive_words: Option<bool>` | **7, INT32**, wire0 | omis | `Some(false)` → 0 (`38 00`), `Some(true)` → 2 (`38 02`) |
| `auto_punctuation: Option<bool>` | **40, BOOL**, wire0 | omis | `Some(false)` → 0 (`c0 02 00`), `Some(true)` → 1 (`c0 02 01`) |
| `secondary_routing: Option<String>` | **64, STRING**, wire2 | omis | chaîne UTF-8 non vide, clé `82 04`, longueur en octets |

Les trois valeurs par défaut sont `None`. **Absence n'est pas false** : le membre
associé au champ7 dans le schéma APK a la valeur initiale **2**. L'API ne traduit
pas l'omission en une valeur ou politique serveur présumée. Le champ7 n'est ni
un BOOL ni une nouvelle interprétation des intentions de VoiceEdit.

Exemple d'assemblage local seulement :

```rust
use rambler_protocol::{initial_request, ProtocolConfig};

let config = ProtocolConfig {
    block_offensive_words: Some(true),
    auto_punctuation: Some(false),
    secondary_routing: Some("caller-secondary-routing".into()),
    ..Default::default()
};
let bytes = initial_request(&config, 0)?;
# Ok::<(), rambler_protocol::Error>(())
```

Le routage principal reste `routing`, champ50, indépendant du champ64. Le client
Android observé écrit le champ64 sous `kmy.n` (Jetson) et seulement si `muq.B`
n'est pas vide. L'API Rust expose cette chaîne vérifiée sans inventer de flag
Jetson, de couplage forcé avec une clé principale inconnue, ni de mode implicite.
C'est à l'appelant de fournir une clé adaptée ; aucune clé distante n'est déduite.

### Bornes et confidentialité

- `Some("")` est rejeté comme le routage principal ; aucune interprétation de
  chaîne vide en mode spécial. `None` reste valide.
- Même validation que `routing` : chaîne non vide, au plus `MAX_MESSAGE_BYTES`
  (**8 MiB**) d'UTF-8. Cette borne défensive SDK n'est pas une limite serveur
  observée. Une chaîne individuellement valide peut encore dépasser la borne
  du sous-message ou de la requête complète avec leurs enveloppes.
- Les deux clés et les autres extensions restent soumises aux bornes cumulées de
  l'assembleur. Les espaces, NUL, accents combinants et caractères supplémentaires
  ne sont ni normalisés ni supprimés, comme pour `routing`.
- Les trois nouvelles options sont exclues du `Debug` manuel de `ProtocolConfig`.
  Les erreurs ne contiennent pas leur contenu. Le `Debug` historique des champs
  déjà visibles, dont `routing`, n'est pas modifié.
- Ces champs sont init-only ; ni hints ni options ne sont copiés vers le contexte
  tardif, l'audio ou END. Aucun changement de sélection, de portée d'édition ni
  d'application de texte.

## Preuves statiques et provenance

Référence APK et niveaux de preuve : [rust-feature-parity.md](rust-feature-parity.md).
SHA-256 APK **repris de cette matrice**, pas présenté comme un nouveau hachage :
`a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`.
Les noms obfusqués et numéros de ligne concernent exclusivement ce build et les
extractions indiquées ; ce ne sont pas des identifiants de protocole universels.

Extraits locaux relus (texte uniquement) :

- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx/sources/defpackage/aapf.java`
  :22,29,44 : `g=2`, extension294500, schéma associant `g`→7 INT32,
  `l`→40 BOOL et `i`→64 STRING (`h`→50 STRING).
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-knv-simple/sources/defpackage/knv.java`
  :35,49–59 : valeur issue de `sba.s` vers `l` avec présence explicite ;
  `kmy.h` vers `g` avec présence, false→0, true→2 (:376–378).
  :87–105 : branche `kmy.n`, clé principale `muq.A` vers `h`, puis clé
  secondaire `muq.B` non vide vers `i`.
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java`
  :2000–2004 : bundle `session_type="jetson"`.
- `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ihv/sources/defpackage/ihv.java`
  :106–128 : construction du header puis `ihuVar.a(aaegVar)` avant émission.

Chaîne d'appel vérifiée par Kimi, et passages ci-dessous relus localement dans
les extractions JADX (classe, lignes ; non-exécution) :

1. `fgr`,98–103 → `ffh.u`,2000–2004 fournit le bundle Jetson.
2. `kla`,183–250 lit ce bundle et alimente les paramètres Jetson (`kmy.n`).
3. `kki`,63–85 choisit conditionnellement les fournisseurs Jetson, dont NewS3 ;
   ce n'est pas une garantie que tout appareil choisira ce fournisseur.
4. `knn`,25–35 crée `knt` lorsque le gate accepte `kmy.n`.
5. `knt`,74–110 injecte **`new knv(context, kmyVar)`** dans `iho` ; :340 lance
   `iho.j`.
6. `iho`,280–304 appelle `ihv.a(..., g, ...)` pour le flux concerné, y compris
   sa branche avec flux secondaire.
7. `ihv`,106–128 applique `ihuVar.a(builder)` ; le mutateur injecté est `knv.a`.

Cela établit une capacité de requête S3 concrète, pas seulement un champ de
schéma isolé. Cela **ne valide pas l'effet du service** sur la ponctuation, le
filtrage lexical ou le routage. Aucun test Google n'avait été effectué dans la
tranche protocole initiale ; les essais complémentaires ci-dessous prouvent
seulement l'acceptation des requêtes7/40 sur le profil testé.

## Vérification offline initiale du protocole

Fichiers du lot uniquement :

- `crates/rambler-protocol/src/request.rs` : configuration, validation, encodeur init.
- `crates/rambler-protocol/tests/recognizer_options.rs` : **7 nouveaux tests**.
- `docs/research/rust-recognizer-options.md` : ce document.

Les nouveaux tests emploient un lecteur wire indépendant du module `wire`
produit et des vecteurs hexadécimaux littéraux. Ils vérifient :

- omission des trois champs et golden natif inchangé octet pour octet ; les deux
  anciens goldens Python/custom restent vérifiés par la suite `requests` existante ;
- champ7 absent/0/2 et champ40 absent/false/true, y compris les zéros explicites ;
- **18 combinaisons** de présence/valeurs, avec hints, préférences et langues
  secondaires ; aucune autre extension ne change et la configuration est intacte ;
- champ64 UTF-8 exact, clé principale intacte, espaces/NUL/non-normalisation ;
- validation identique au routage principal, vide/oversize rejetés, bornes UTF-8,
  requête de taille exactement 8 MiB acceptée et +1 octet rejeté, somme des clés
  trop grande rejetée même si chaque chaîne est valide ;
- confidentialité `Debug`/erreurs et vecteurs contexte/audio/END inchangés.

Commande depuis la worktree, cible courte `.t`, sans symboles debug ni incrémental :

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR="$PWD/.t" cargo test --offline --locked -p rambler-protocol
```

Résultat : **63 tests réussis, 0 échec**, doctest compris. `rustfmt --check`
sur les deux fichiers Rust du lot et `cargo clippy --offline --locked
-p rambler-protocol --tests -- -D warnings` passent également avec les mêmes
profils sans debug. La cible temporaire `.t` est supprimée après vérification.
Aucun build/test moteur, transport, audio, CLI, Opus ou CMake. Aucun changement
de manifest ou de lockfile.

## Frontières inchangées

Le champ7 de **RecognizerInfo** est distinct du champ30 de sécurité des
**hypothèses de réponse**. Aucune modification de `check_safety`, des refus de
rendu/proposition, ou de leurs tests : l'option n'est pas un contournement sécurité.
Aucune implémentation COMPOSE, intention0/1/5/6, flag opaque ou mode serveur inféré.

La sélection legacy est déjà encodable avec `context.before = selection` et
`cursor_aware = false`. Aucun flag `SessionOptions`, aucune proposition destructive
ni nouvelle portée de remplacement n'est ajouté. Les options restent opt-in. La CLI intégrée expose ensuite
`--block-offensive-words true|false`, `--auto-punctuation true|false` et
`--secondary-routing VALEUR` pour les deux commandes. Le runner `session_reuse`
accepte les trois membres correspondants dans son manifeste, sans les publier
dans ses sorties ni changer ses limites. Les tests loopback inspectent les
champs init effectivement reçus pour valeurs omises, fausses et vraies.

## Vérification distante bornée après intégration

[Rapport public](../evidence/rust-recognizer-options-public.json) : trois sessions
sur le même clip synthétique ordinaire, un moteur et un appel de connexion explicite,
zéro retry. Champs7/40 omis, puis false/false, puis true/true, déclarés avant
l'exécution. Toutes terminent avec PCM réellement consommés/hashés, EOF et
fermeture, END S3 et statut gRPC OK ; stderr vide. Les trois textes ont le même
hash que la référence connue : aucun changement de formatage observé ici.

Cela **ne démontre pas** l'efficacité du filtre lexical ni de l'auto-punctuation.
Le texte n'est pas offensant ; aucune tentative de contourner un blocage n'est
faite. Le serveur pourrait traiter ces préférences différemment selon routage ou
profil. Field64 reste omis : aucune clé inconnue n'a été devinée. Les hashes des
sources/lock/binaire et le plan avant exécution distinguent cette preuve des tests
protocoles et des anciennes observations.
