# Opus Release sous Windows MSVC : qualification locale

Le 16 novembre 2026, une comparaison synthétique hors ligne sur Windows x64 a qualifié localement un changement de configuration du build natif Opus Release. Sur les trois signaux et les deux paires mesurées, `native_frames` est 3,0 à 3,6 fois plus rapide en temps écoulé ; le temps écoulé du pipeline audio complet aux pages par défaut diminue de 67,4 à 71,6 %. Ce résultat est local, pas une garantie de production ou multiplateforme. Au commit `2334b76`, les 14 jobs CI sont verts, dont le contrôle fonctionnel VS17 ; deux jobs macOS ARM ont nécessité une unique relance après indisponibilité des runners, sans aucune étape exécutée lors de la première tentative.

[Preuves publiques et mesures brutes](../evidence/rust-opus-release-build-windows-20261116.json) · [Instructions de build et reproduction](../../crates/rambler-audio/BUILDING.md) · [Source du banc](../../crates/rambler-audio/examples/profile_pipeline.rs) · [Profilage audio antérieur](rust-audio-profiling.md)

## Comparaison et provenance

A est une baseline fraîche et B le candidat : même source Rust (`8cb381e`), même lockfile, même profil et même exemple audio. Configuration candidate locale identifiée par le SHA-256 `6e4e7dc9…` ; la documentation était à `0c5daf4`. La configuration et son contrôle CI sont intégrés par `95ba795`, puis le repérage des sorties Cargo est corrigé par `2334b76`. Les builds utilisent Windows x64, Visual Studio 2022/17, CMake 3.31.10 et MSVC 14.44. Il ne s'agit pas d'une comparaison avec les anciens chiffres Windows : l'environnement effectif des compilations C historiques et l'association archive–exécutable n'y sont pas suffisamment attestés.

Les 152 enregistrements effectifs de compilation C d'Opus ne portent pas d'option d'optimisation explicite pour A ; ceux de B portent `/O2`, `/Ob2` et `NDEBUG`. `/MD`, `/fp:precise`, `/GS`, les options de durcissement, RTCD et SIMD restent comparables ; la duplication de `/Brepro` passe de deux à une, comme attendu. Aucune modification de qualité, débit, complexité, durée des trames ou valeur par défaut des pages n'est recherchée. Ces éléments ne constituent pas une attestation du code objet ou du dispatch CPU réellement exécuté ; les sources sélectionnées ne représentent pas tous les outils et entrées transitives.

## Protocole et résultat

Trois signaux synthétiques, silence, harmonique et bruit, comportent chacun 80 017 échantillons PCM16LE mono à 16 kHz. Le codec utilise VoIP, VBR 24 kbit/s et des trames de 20 ms ; les pages par défaut sont mesurées, et des pages de 1 000 ms servent de contrôle legacy, sans changement du défaut.

Avant réservation des mesures, les six preuves legacy et leurs métadonnées sont égales entre builds : notamment SHA des paquets et du PCM décodé à 48 kHz, Ogg canonique, pages, paquets, granule et pre-skip. Opus est avec perte : l'égalité porte sur les résultats décodés entre builds, pas sur une égalité avec le PCM original.

L'ordre réel du budget réservé de quatre runs est A1, B1, B2, A2 ; chacun comporte sept répétitions de cinq itérations. Toutes les 588 mesures d'attribution, valeurs aberrantes comprises, sont conservées dans les preuves ; aucun cinquième run. Le critère préenregistré exigeait au moins 10 % de gain natif pour chaque signal dans les deux paires et rejetait toute régression du total d'au moins 10 % pour un signal dans l'une ou l'autre paire. Il est satisfait localement.

Les plages annoncées en tête couvrent les trois signaux et les deux comparaisons A1/B1 et A2/B2, sur leurs médianes par run ; `total_off` désigne le pipeline complet avec observateur de progrès désactivé. Les valeurs détaillées par signal et paire figurent dans les preuves publiques.

`native_frames` inclut la boucle wrapper et le stockage des résultats, pas uniquement des appels de bibliothèque isolés. Les fenêtres de mesure ne sont pas additives. Génération, hashing, décodage, création du runtime et affichage sont hors chronomètre ; les périmètres exacts figurent dans les en-têtes du banc. Ce sont des temps écoulés, non des cycles ni du CPU processus. Pas d'extrapolation WAN, ASR, voix naturelle, qualité de reconnaissance, allocation, RSS ou optimalité globale.

## Tests, incident et Debug

La première phase candidate a échoué sur une attente codée en dur de cinq tests d'exemple : huit passaient effectivement, cinq directs et trois tests Ogg inclus. Le rapport d'échec `bce3` et le contrôleur original `8ceb` restent conservés sans modification. Le supplément ciblé et revu `7357` a poursuivi uniquement les étapes non encore exécutées, sans nouvelle tentative de build ou de test ; les 12 tests d'encodage et les huit tests d'exemple Release ont tous réussi. Cette correction ne transforme pas l'échec initial en succès historique.

Une qualification Debug distincte, auditée localement sur 11 captures et 96 artefacts conservés, rapporte huit tests d'exemple réussis sous stable et, pour le workspace, 429 réussis, zéro échec et deux ignorés aussi bien sous stable que sous MSRV Rust 1.88. Les métadonnées anciennes et nouvelles correspondent exactement pour propriétés, sources et tokens après normalisation de leur propre racine uniquement. Les 152 commandes Debug portent `/Od`, `/Ob0`, `/RTC1`, `/Zi`, `/MD`, `/GS` et `/fp:precise`, avec un `/Brepro` et trois sources AVX2. Les exécutables Release, archives natives et dossiers de mesure scellés sont restés inchangés. L'égalité de métadonnées historiques n'atteste pas l'identité du compilateur exécuté à l'époque ; aucun gain Debug n'est revendiqué.

## Portée du correctif et reproduction

La configuration Cargo du dépôt fournit pour la cible Windows MSVC x86_64 le générateur Visual Studio 17 2022 non forcé. Elle permet le build avec VS 2022 sans Ninja ; le choix de Ninja par le build script reste prioritaire. `force = false` ne protège qu'une variable d'environnement de clé exactement identique : une clé générique de générateur peut être masquée et la variante à tirets a priorité. Pour une autre version de VS, définir explicitement la clé appropriée ; voir les détails et commandes dans [BUILDING](../../crates/rambler-audio/BUILDING.md).

Cargo découvre cette configuration depuis son répertoire courant dans le workspace, pas automatiquement pour un consommateur en aval ou une invocation extérieure avec `--manifest-path`. Ceux-ci doivent fournir leur propre Ninja ou la variable exacte de générateur. Un cache CMake déjà créé peut conserver l'ancien générateur : préserver d'abord les artefacts utiles, puis utiliser un target neuf ou un nettoyage ciblé du profil selon BUILDING, jamais un nettoyage aveugle de `target`.

Le résultat concerne uniquement Windows x64 et les versions locales indiquées. La clé Windows n'est pas utilisée pour Linux/macOS ; autres architectures et versions VS non qualifiées. Le job fonctionnel explicite sur `windows-2022` ne mesure pas les performances et les huit jobs natifs ordinaires restent distincts. Aucune acceptation de production n'est ici anticipée.

## Incident CI conservé et vérification fonctionnelle

Au commit `95ba795`, 13 des 14 jobs passent, dont les huit jobs natifs ordinaires ; le nouveau job VS17 compile et réussit ses huit tests, puis échoue sur `expected_one_opus_cache`. Ce résultat reste un échec, non une qualification VS17. Le journal montre un agencement Cargo différent du glob attendu ; aucune relance aveugle ni mesure supplémentaire n'a été faite.

Le correctif `2334b76` exploite le [message JSON documenté](https://doc.rust-lang.org/cargo/reference/external-tools.html#build-script-output) `build-script-executed` et son `out_dir`, sans supposer l'emplacement des fichiers privés de Cargo. Trente tests synthétiques passent. Le [job VS17 hébergé](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43917647762/job/133476039222) réussit : huit tests Release, VS17/x64/host=x64, 152 commandes C, trois sources AVX2, un `/Brepro` par commande, définitions requises et identité des archives construite/installée vérifiés. Le SHA d'archive concerne ce checkout, pas une identité binaire entre machines.

La première tentative du commit corrigé termine avec 12 jobs réussis et deux jobs macOS ARM annulés avant toute étape : GitHub signale explicitement un manque de capacité et aucun runner attribué. Une seule relance ciblée de ces deux jobs non exécutés réussit. GitHub expose les huit succès Rust réutilisés sous de nouveaux identifiants de vue ; leurs timestamps d’exécution et étapes sont identiques aux originaux, sans réexécution, notamment du job VS17. Les quatre succès Python restent ceux de leur première tentative.

Les [dix jobs Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43917647762) — huit natifs, VS17 et lint — et les [quatre jobs Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43917647705) sont ainsi vérifiés au commit exact `2334b768fd42c7451692daa827ee31f14d37b31d`. Les métadonnées finales et les 14 journaux sont conservés avec leurs SHA-256 ; les échecs initiaux restent des échecs historiques. Le CLI Windows local initial reste intact ; les builds CI utilisent leurs propres artefacts. Aucun nouveau chronométrage ni appel au service réel.
