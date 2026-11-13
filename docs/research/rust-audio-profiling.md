# Profilage offline Rust PCM → Opus/Ogg

Mesure du 2026-11-13, indépendante du réseau et du service. Aucun `.env`,
credential, audio privé, corpus externe ou appel distant n'est utilisé. Les trois
entrées sont synthétiques et générées en mémoire par le banc versionné.

## Protocole

Banc : `crates/rambler-audio/examples/profile_pipeline.rs`, exécutable uniquement
avec `--release`. Trois signaux PCM16LE mono16k déterministes : silence,
combinaison de triangles périodiques avec silences, bruit xorshift32 de seed
`0x12345678`. L'arithmétique du générateur est entière. Ces signaux ne sont pas
un corpus de voix naturelles et ne mesurent pas la qualité de reconnaissance.

Paramètres de cette campagne : `7 5 100 80017` : 7 répétitions, 5 opérations
encodeur/total par répétition, 100 opérations mux, 80017 échantillons
(5,0010625 s, 160034 octets PCM) par entrée ; pages 20 et 1000 ms. Les pages
1000 ms sont un contrôle, **pas** une proposition de changer le défaut 20 ms.
Un warmup par étape/configuration est exclu. L'ordre des trois étapes tourne
entre répétitions ; l'ordre des signaux/pages reste fixe et est une limite.

- **Encodeur** : création du même libopus natif, `Application::Voip`, mono16k,
  VBR 24000 bit/s, trames 320 échantillons/20 ms, conversion PCM LE, encodage,
  padding jusqu'à couvrir input + lookahead, collecte de tous les paquets et
  destruction de l'encodeur. Aucun réglage de complexité/qualité n'est changé.
- **Mux** : création du vrai mux privé compilé depuis `src/ogg.rs`, en-têtes,
  paquets préencodés, granules/EOS, collecte de chaque page dans un `Vec<u8>`,
  destruction. Il ne réencode pas l'audio.
- **Total** : véritable `Pcm16Opus` public, source mémoire par chunks 640 octets,
  runtime Tokio current-thread, yields coopératifs conservés, drainage complet,
  collecte des pages et `close().await`. Runtime créé hors mesure ; setup du codec
  inclus. Pas de lecteur WAV, pacing, disque ou transport.

Chaque opération est chronométrée avec `Instant`, puis son résultat est validé
**hors chronomètre**. `batch_ns` est la **somme** des durées des opérations de la
répétition, pas la durée continue incluant validations. Chaque résultat est
consommé dans des vecteurs et passé à `black_box` ; pas de paquet/page ignoré.
Allocations et copies des sorties sont incluses, libération du vecteur résultat
après retour exclue. Hashing, décodage et assertions de fidélité sont exclus.
Les trois temps ne sont pas des composants additifs exacts : notamment le banc
encodeur garde des vecteurs de paquets, le total travaille en streaming.

Les horloges sont des **temps écoulés offline**, pas des compteurs de cycles,
ni du CPU processus isolé. Il n'existe aucune inférence de latence réseau,
service, microphone ou de reconnaissance depuis ces mesures.

## Conditions et provenance

Révision de départ : `4fdf29e8197e76d4ea9fdf004ff74bbc7dd35be1`.
Windows 11, version hôte fournie 10.0.26300, `x86_64-pc-windows-msvc` ;
`PROCESSOR_IDENTIFIER=Intel64 Family 6 Model 154 Stepping 3, GenuineIntel`,
12 processeurs logiques annoncés par l'environnement. Rust 1.99.1
(`58cf8f9c35b355199727627d07ae34a4e2846d67`, LLVM 23.1.1), Cargo 1.99.1
(`5f94df478`), CMake 3.31.10, générateur Visual Studio 17 2022/x64,
libopus 1.6.1 bundled statique (`OPUS_BUILD_SHARED_LIBRARY=OFF`, Release).
Aucun override de profil Cargo : profil release optimisé standard du workspace.
Répertoire Cargo propre à ce worktree ; aucune réutilisation du target parent.
Le plan d'alimentation, affinité et fréquences CPU ne sont pas contrôlés.

Les fenêtres de mesure sont réservées auprès du coordinateur après compilation,
sans compilation concurrente annoncée. Cela ne garantit pas l'absence de toute
activité OS/éditeur/antivirus. Caches non purgés, warmups explicites : ce n'est
pas une mesure de démarrage froid.

SHA-256 de provenance :

```text
Cargo.lock       0fcd2757af343aeebabd8797a9a22771fb09ecb20d86853a15c3036d051d1628
encode.rs        5db989fa18b3a1126108d961d6baa638a6f2fdbd16d5095c7d223940c986a424
banc mesuré      f8319c6dcdd787b873d5ebe620b30ec292b2ac29f637f4575c4ccee4ef393194
banc livré       58ecc07212914d66bcd2bb621e13f172459c9586fdf86e79e8d8515fa79bf053
ogg.rs avant     9581bd5655771e39fd40f649954499ea529cddbac7555b31d37af6b360f9e63e
ogg.rs candidat  3733f05758211fc59899f0f51e95ca6741d337792cf37135d5ebc149f8a0d93e
baseline.exe     79a550592f3982e8964231f2e7e2100fc9aea9489749862f494ba4e43ddde1b1
candidate.exe    7adeb1404a051075892caaee78e6e252b135eb2db4e9007e47ddab92222e8e7e
```

Hashes des octets réels : `ogg.rs` et `encode.rs` utilisent CRLF dans ce
worktree, le banc LF. Le bloc candidat de l'annexe C, réinséré avec CRLF dans
le `ogg.rs` initial, reproduit exactement le hash source candidat ci-dessus.

## Mesures et décision

**Aucune optimisation de production retenue.** Le candidat remplaçait uniquement
les huit pas bit-à-bit du CRC Ogg par une table constante de 256 u32 (1 KiB).
La fidélité est identique, mais le gain mux n'est ni robuste ni uniforme ; le
coût total ne justifie pas un refactor. `src/ogg.rs` et `src/ogg_tests.rs` ont été
restaurés exactement à la révision de départ. Le profil codec, qualité, VBR,
bitrate, trames/pages par défaut, EOF, cleanup et bornes sont inchangés.

Ordre réel A1 → compilation candidate (1,81 s, hors mesure) → B1 → B2 → A2.
A = binaire original préservé, B = candidat CRC ; A2 réexécute exactement A1,
pas une recompilation. Horaires UTC du 2026-11-13 :

| Run | Début | Fin |
|---|---|---|
| A1 | 21:46:04 | 21:46:46 |
| B1 | 21:47:54 | 21:48:34 |
| B2 | 21:48:34 | 21:49:16 |
| A2 | 21:49:16 | 21:49:55 |

Chaque colonne par run est la médiane des 7 moyennes par répétition. Les colonnes
groupées réunissent 14 répétitions, **pas** la moyenne des deux médianes ; elles
incluent minimum et maximum de ces moyennes. Unité ms par clip/opération.
Arrondis à six décimales ; cette présentation n'est pas une revendication de
précision nanoseconde. Valeurs brutes entières conservées en annexe.

| Signal | Pages ms | Étape | A1 ms | B1 ms | B2 ms | A2 ms | A groupé ms (min–max) | B groupé ms (min–max) |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| silence | 20 | encode | 104.024680 | 88.722660 | 99.603820 | 88.821280 | 90.367790 (87.628160–116.908060) | 92.770420 (87.330860–109.822060) |
| silence | 20 | mux | 0.053154 | 0.050643 | 0.061088 | 0.050492 | 0.051553 (0.047738–0.078135) | 0.059595 (0.047330–0.071049) |
| silence | 20 | total | 96.599260 | 87.943240 | 94.690280 | 89.718260 | 91.792440 (88.139460–108.620980) | 89.922410 (87.019420–102.196200) |
| silence | 1000 | encode | 99.109260 | 101.302500 | 89.582120 | 89.196740 | 94.014260 (86.545200–103.876860) | 91.541950 (88.550420–123.487080) |
| silence | 1000 | mux | 0.014742 | 0.013178 | 0.014153 | 0.014400 | 0.014494 (0.011337–0.022981) | 0.013958 (0.012696–0.018043) |
| silence | 1000 | total | 102.039660 | 100.915980 | 89.406120 | 90.374600 | 97.807810 (87.796660–103.928800) | 90.047640 (86.970620–107.840940) |
| harmonic | 20 | encode | 45.694620 | 46.570480 | 44.667300 | 45.430440 | 45.565560 (44.459280–52.581420) | 46.245490 (44.376840–52.166280) |
| harmonic | 20 | mux | 0.077348 | 0.092767 | 0.080259 | 0.077021 | 0.077185 (0.073384–0.081126) | 0.081568 (0.076015–0.098439) |
| harmonic | 20 | total | 45.912180 | 50.075720 | 46.437400 | 46.135800 | 46.023990 (44.935840–51.178220) | 48.042570 (44.654060–56.517880) |
| harmonic | 1000 | encode | 45.205500 | 53.554220 | 51.137640 | 45.660520 | 45.433010 (43.844240–48.586920) | 51.425620 (45.143420–65.413520) |
| harmonic | 1000 | mux | 0.048370 | 0.052354 | 0.042897 | 0.050696 | 0.049815 (0.044109–0.053906) | 0.051075 (0.039839–0.066753) |
| harmonic | 1000 | total | 46.374040 | 52.218420 | 50.340120 | 46.738320 | 46.391100 (44.836620–49.977680) | 50.889190 (44.861640–56.505720) |
| noise | 20 | encode | 139.711000 | 122.034200 | 141.389380 | 121.117740 | 129.014940 (120.169420–144.440800) | 129.983980 (119.272380–156.232460) |
| noise | 20 | mux | 0.082164 | 0.080331 | 0.080130 | 0.078110 | 0.080816 (0.073164–0.102103) | 0.080231 (0.075930–0.095993) |
| noise | 20 | total | 148.460900 | 123.805320 | 145.943420 | 122.716000 | 133.868220 (120.317620–154.243300) | 130.107770 (119.283760–173.458100) |
| noise | 1000 | encode | 127.022060 | 121.424920 | 133.158500 | 124.099260 | 125.560660 (120.842060–149.113340) | 124.877480 (119.786980–162.775820) |
| noise | 1000 | mux | 0.043189 | 0.039980 | 0.042384 | 0.045549 | 0.044047 (0.038397–0.052948) | 0.042170 (0.038415–0.045333) |
| noise | 1000 | total | 128.504320 | 122.568120 | 126.154740 | 126.450540 | 128.082200 (121.511380–146.178560) | 124.559670 (120.329900–157.959320) |

Pages20ms : mux groupé A→B = 51,553→59,595 µs (silence, **+15,6 %**),
77,1845→81,5675 µs (harmonique, **+5,7 %**), 80,816→80,231 µs
(bruit, −0,7 %). Aux pages1000ms : −3,7 %, +2,5 %, −4,3 % ; distributions
recouvrantes. Aucun cas ne permet une affirmation générale d'amélioration.
L'encodeur **inchangé** varie lui-même d'environ −2,6 % à +13,2 % entre groupes :
les variations totales (−7,9 % à +9,7 %) ne sont pas attribuables au CRC.

Sur A, le mux est de l'ordre de 14–81 µs, contre environ 45–134 ms pour le total
(médianes groupées). Le ratio de ces mesures séparées reste inférieur à0,2 % ;
ce n'est pas une décomposition additive exacte. Pas de changement de qualité ou
de complexité du codec pour chercher un gain artificiel. Aucun autre refactor
spéculatif n'a été tenté. Aucun pourcentage de gain CPU processus, réseau ou
Rust/Python n'est établi.

Après mesure, seuls deux attributs `allow` Clippy et leurs commentaires ont été
ajoutés au banc (itérateur `chunks_exact` mesuré et rejet runtime du mode debug).
Aucun énoncé exécuté, générateur ni boucle chronométrée n'a été modifié. Les
hashes du banc mesuré et livré sont distincts et explicités ci-dessus ; les
résultats ne sont pas présentés comme un nouveau run après lint.

### Tests et vérification finale

- Candidat : `cargo test --release --locked -p rambler-audio` : **30 passés,
  1 ignoré**, 7 suites. Le test temporaire supplémentaire comparait le CRC table
  au CRC original, les 256 octets possibles, des préfixes déterministes jusqu'à
  64 KiB, vide et vecteur `123456789 → 0x89a1897f` ; retiré avec le candidat.
- Arbre final restauré : même commande : **29 passés, 1 ignoré**, 7 suites,
  1,00 s d'exécution des suites (hors compilation).
- `cargo clippy --release --locked -p rambler-audio --all-targets -- -D warnings` :
  réussi ; `cargo fmt -p rambler-audio -- --check` : réussi.
- Une première passe Clippy a signalé deux lints du nouveau banc, ensuite corrigés
  par attributs locaux sans changer le workload. Aucun échec de test fonctionnel.
- Les tests existants couvrent fragmentation/EOF, granules, CRC indépendant,
  limites, cancellation et fermeture idempotente. L'oracle PyAV indépendant
  ignoré n'a **pas** été lancé pour cette tâche ; aucun Python/PyAV/FFmpeg n'est
  impliqué dans l'encodage mesuré.


## Contrôles de fidélité

Pour **chaque** opération encodeur chronométrée : égalité exacte de tous les
paquets avec la référence déterministe. Pour **chaque** opération mux/total :
lecture par le crate indépendant `ogg` (validation du CRC réel et du framing),
égalité de tous les paquets Opus, séquence des pages et unicité du serial,
flags BOS/EOS, granule final, hash Ogg canonique. Seuls serial et champ CRC
sont mis à zéro dans le hash canonique, **après** validation du CRC réel ; les
variations de serial entre streams ne représentent pas un changement audio.

Les paquets sont hashés avec chaque longueur préfixée en u32 LE ; le PCM décodé
à 48k est hashé après retrait du pre-skip et trim au granule final, soit exactement
`80017 * 3 = 240051` échantillons. Le PCM Opus est lossy : sa fidélité est vérifiée
**avant/après optimisation**, jamais déclarée identique à l'entrée originale.

## Reproduction

Depuis un checkout/worktree propre au build, avec toolchain C/CMake disponible :

```sh
# target sous ce checkout uniquement ; ne pas partager avec un build concurrent.
export CARGO_TARGET_DIR="$PWD/target/audio-profile"
# Windows si CMake n'est pas sur PATH : export CMAKE='/chemin/absolu/cmake.exe'
cargo build --release --locked -p rambler-audio --example profile_pipeline
# Attendre la fin de TOUTE compilation, puis lancer directement le binaire.
"$CARGO_TARGET_DIR/release/examples/profile_pipeline.exe" 7 5 100 80017 > baseline.csv
# Sur Unix, retirer seulement le suffixe .exe.
# Conserver/copier ce binaire AVANT de compiler un candidat.
# Recompiler le candidat, attendre de nouveau la fin de toutes compilations.
# Exécuter les binaires préservés séquentiellement, jamais en concurrence.
```

Comparer toutes les lignes `proof,` entre les sorties, pas seulement les temps.
Les lignes `timing,` contiennent compte d'opérations, somme de nanosecondes et
moyenne par opération. Conserver les valeurs aberrantes ; petits n et ordre fixe
ne permettent pas d'estimer une longue traîne de production.


## Annexe A — preuves exactes communes aux quatre runs

Les six lignes ci-dessous ont été comparées automatiquement entre A1/B1/B2/A2 :
égalité stricte, aucune ligne omise. Par configuration/version : 35 encodages,
700 mux et 35 totaux chronométrés ; sur deux runs par version, respectivement
70, 1400 et 70 sorties entièrement vérifiées. Aucun échantillon exclu.

```csv
proof,kind=silence,page_ms=20,pcm_bytes=160034,pcm_sha256=b5e0410caf0728b0a46afb14c1eb4935d2cd0ae6fb5cd350d2df8818ec80b201,audio_packets=251,packet_bytes=2009,packet_sha256=5b2cd5e02bf347600031a29d8762ff7fa1e2bc685becf3ad192ca2efe94da518,ogg_bytes=9141,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=0cc292f0ceeed3354055fe42f1995cd395af483a519eb9f84c7271d096a9e87e,decoded_pcm48k_sha256=9271e177675df2fcdbcb2d44fbb9e21a74382b36f3d5b6cf9829c1fc9e8dce40
proof,kind=silence,page_ms=1000,pcm_bytes=160034,pcm_sha256=b5e0410caf0728b0a46afb14c1eb4935d2cd0ae6fb5cd350d2df8818ec80b201,audio_packets=251,packet_bytes=2009,packet_sha256=5b2cd5e02bf347600031a29d8762ff7fa1e2bc685becf3ad192ca2efe94da518,ogg_bytes=2526,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=38cf20f7e141b6d46d656e1bac7a4eca2d6c17a7d2abb4998cae55ec6074be42,decoded_pcm48k_sha256=9271e177675df2fcdbcb2d44fbb9e21a74382b36f3d5b6cf9829c1fc9e8dce40
proof,kind=harmonic,page_ms=20,pcm_bytes=160034,pcm_sha256=c554e95f4010e21ba354933148262aa41d6247d1910996740ea00e45b15ae24c,audio_packets=251,packet_bytes=13419,packet_sha256=c3bb3cd84699fe6f8bb3f07e4c0a3282aa3c6e02ca955678f966b3c706011697,ogg_bytes=20551,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=709e7cc7cccc7cad7b542bd844a9a07da97642e62055de5b0b035a4e449d7577,decoded_pcm48k_sha256=69da05665588f7b56fcd5c3eec30a97eb79349882565c27283b0a7d10af3f887
proof,kind=harmonic,page_ms=1000,pcm_bytes=160034,pcm_sha256=c554e95f4010e21ba354933148262aa41d6247d1910996740ea00e45b15ae24c,audio_packets=251,packet_bytes=13419,packet_sha256=c3bb3cd84699fe6f8bb3f07e4c0a3282aa3c6e02ca955678f966b3c706011697,ogg_bytes=13936,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=be0e055a556db7e1bf0ee7ed973bf094222252a0fcbe1dae90fee125fecbbae0,decoded_pcm48k_sha256=69da05665588f7b56fcd5c3eec30a97eb79349882565c27283b0a7d10af3f887
proof,kind=noise,page_ms=20,pcm_bytes=160034,pcm_sha256=8afee5c59bf517ab814636fb6e8b2c7d8925f81a6f902ce9193a7c2041f89107,audio_packets=251,packet_bytes=13460,packet_sha256=7be0bfc5488dfd66a19ec08d14351978dc08dff5e4f44fd056308bd22929ec02,ogg_bytes=20592,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=9789fe5766f6e12940f26e3c98e98b3d650b621cddae05853ba4bcf94ecf9480,decoded_pcm48k_sha256=643f3c1d92f2c40f5c1254b6eca5c72c4670e34b59fe687dfb3a178d8b811dfd
proof,kind=noise,page_ms=1000,pcm_bytes=160034,pcm_sha256=8afee5c59bf517ab814636fb6e8b2c7d8925f81a6f902ce9193a7c2041f89107,audio_packets=251,packet_bytes=13460,packet_sha256=7be0bfc5488dfd66a19ec08d14351978dc08dff5e4f44fd056308bd22929ec02,ogg_bytes=13977,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=8bd05ef6cb0f949de9e4582d6c4011c5482ae8e07ae98fa418091d59883c662d,decoded_pcm48k_sha256=643f3c1d92f2c40f5c1254b6eca5c72c4670e34b59fe687dfb3a178d8b811dfd
```

## Annexe B — sommes brutes de nanosecondes par répétition

Une ligne fournit les sept `batch_ns` entiers dans l'ordre repeat0..6. Diviser
chaque somme par `iterations`, puis par 1e6 pour obtenir les ms/opération.
Chaque run contient 126 lignes timing et six lignes proof (134 lignes avec
metadata et en-tête). Cette annexe conserve toutes les répétitions, pas uniquement
leurs médianes. Les sommes sont des durées écoulées mesurées, pas un CPU processus.

SHA-256 des CSV originaux, sorties stdout sans modification :

```text
a1.csv e0486fc5893391e9226f70889d0c5c741c986238796ea1efceddcfd0cd1ecec0
b1.csv 418dc5978a4b89902acad2653ffeafe59cf46031566c6d301667a93c41d50ddd
b2.csv b51cd99b59ae829ea29b460d0655c4062431b609f9cae0490a528bd7105fba74
a2.csv 27c10624c93bce14f8968e01ad94d2ad2c4c7cc44db4bca9232238d5a433e041
```

```csv
run,signal,page_ms,stage,iterations,repeat0_ns,repeat1_ns,repeat2_ns,repeat3_ns,repeat4_ns,repeat5_ns,repeat6_ns
a1,silence,20,encode,5,520123400,541585000,448025800,454747800,462518600,584540300,532216700
a1,silence,20,mux,100,5472200,7696300,4773800,5315400,4854000,7813500,5117600
a1,silence,20,total,5,541668700,543104900,477538800,482996300,468651400,462883900,535870800
a1,silence,1000,encode,5,495546300,501866900,450566600,496714400,502604000,443294600,432726000
a1,silence,1000,mux,100,1484800,1675200,1133700,1474200,2298100,1134400,1375700
a1,silence,1000,total,5,512145600,519644000,514245400,494403200,510198300,483674900,449637700
a1,harmonic,20,encode,5,227182500,225285500,228473100,224195900,229332700,258359000,241302400
a1,harmonic,20,mux,100,7912900,8089800,7734800,7556200,7908900,7660500,7338400
a1,harmonic,20,total,5,226995600,236307400,228379700,232794600,229070500,229560900,247038500
a1,harmonic,1000,encode,5,226027500,219221200,225627100,239662200,220820900,238375000,236017300
a1,harmonic,1000,mux,100,4533800,4644800,4837000,4410900,5077900,5306900,5294500
a1,harmonic,1000,total,5,228283700,224183100,231870200,235786300,232040800,240132500,227742900
a1,noise,20,encode,5,621279600,662126100,722204000,710233800,705540300,670973000,698555000
a1,noise,20,mux,100,7316400,8216400,7847300,8412500,8069400,10210300,8640800
a1,noise,20,total,5,656742700,742304500,771216500,665042300,703789000,742334700,749643500
a1,noise,1000,encode,5,745566700,668866700,606010500,604210300,620461000,635110300,739190800
a1,noise,1000,mux,100,4882100,4318900,3839700,4182000,4079000,5294800,4411900
a1,noise,1000,total,5,717563300,691797800,638300400,621471600,642521600,626836700,727650800
b1,silence,20,encode,5,472374100,441878700,443613300,440988100,448844100,436654300,549110300
b1,silence,20,mux,100,4773500,4947400,5064300,5076200,4733000,6267500,6193900
b1,silence,20,total,5,439716200,435097100,444713900,438792800,447109900,436512100,510981000
b1,silence,1000,encode,5,522001400,617435400,506512500,494252500,446237500,509064100,494655000
b1,silence,1000,mux,100,1299200,1269600,1311400,1571000,1446900,1317800,1410300
b1,silence,1000,total,5,504579900,538049900,539204700,455645100,444637800,446483300,509240700
b1,harmonic,20,encode,5,255084700,226411400,238773100,244850400,232852400,227228100,232319700
b1,harmonic,20,mux,100,9666500,9276700,9843900,8774000,7860600,8177400,9403400
b1,harmonic,20,total,5,282589400,246618600,236291100,258443200,267597800,233081900,250378600
b1,harmonic,1000,encode,5,225717100,281693300,250610400,277953200,257109000,270955800,267771100
b1,harmonic,1000,mux,100,5036800,4507100,6539400,5235400,5178100,6675300,6105200
b1,harmonic,1000,total,5,233389300,242677800,282528600,261092100,262220300,256578900,271992000
b1,noise,20,encode,5,694422900,596361900,605312400,669242500,610171000,634260600,604517300
b1,noise,20,mux,100,8911800,7833400,8110100,7793700,7867600,8287000,8033100
b1,noise,20,total,5,661035300,619170100,640042400,608664900,609504700,619026600,596418800
b1,noise,1000,encode,5,616775600,624550100,600980900,610208700,607124600,601280500,598934900
b1,noise,1000,mux,100,4247000,4533300,3998000,4503600,3980600,3841500,3887800
b1,noise,1000,total,5,611990000,649647600,612840600,643387200,601649500,628458900,602238900
b2,silence,20,encode,5,455330100,446916200,540003700,499234500,498019100,496470400,533467000
b2,silence,20,mux,100,6251900,6345000,5921400,6108800,5469900,7104900,5997600
b2,silence,20,total,5,438690800,452114200,459138500,497624700,488154300,499368300,473451400
b2,silence,1000,encode,5,461523300,442752100,445723400,450156500,443200100,447910600,453896200
b2,silence,1000,mux,100,1370500,1804300,1415300,1482100,1327700,1381400,1427300
b2,silence,1000,total,5,447030600,466588000,439752400,434853100,450774300,446608700,449702100
b2,harmonic,20,encode,5,222013500,221914700,223336500,230135200,260831400,221884200,233735000
b2,harmonic,20,mux,100,8135400,7819000,8136100,8432000,7601500,7917100,8025900
b2,harmonic,20,total,5,236399800,223270300,228334300,225039100,232187000,244025900,281683600
b2,harmonic,1000,encode,5,257147200,235466300,225989400,227695400,255688200,327067600,272283100
b2,harmonic,1000,mux,100,3983900,4137100,4130800,4508700,4289700,5757200,5450100
b2,harmonic,1000,total,5,243900000,251700600,224308200,252313000,236675800,276512700,259833100
b2,noise,20,encode,5,706946900,716199200,715517100,665579200,781162300,621096900,607108900
b2,noise,20,mux,100,9599300,9131000,8360300,7752800,7593000,8013000,7681100
b2,noise,20,total,5,729717100,745929900,711099000,611453700,867290500,672699500,758905600
b2,noise,1000,encode,5,795218200,624224700,736379100,626084300,813879100,638534400,665792500
b2,noise,1000,mux,100,4195600,4171200,4345000,4354300,4075100,4238400,4436000
b2,noise,1000,total,5,606222900,630773700,748064800,613428200,617137800,789796600,719862700
a2,silence,20,encode,5,508112200,448930100,439164300,439138600,444106400,438140800,447940600
a2,silence,20,mux,100,5193000,5049200,5615500,4850800,5246700,5014100,4785600
a2,silence,20,total,5,455040500,445979600,448591300,449326800,440697300,448923400,442282500
a2,silence,1000,encode,5,436093100,445983700,435272100,441203400,519384300,489576000,508611500
a2,silence,1000,mux,100,1464100,1384600,1347500,1380400,1440000,1458800,1724900
a2,silence,1000,total,5,438983300,453821500,451873000,451859900,439820000,497001300,498333500
a2,harmonic,20,encode,5,262907100,222742300,234243900,231407200,227152200,222296400,224272500
a2,harmonic,20,mux,100,8112600,7699900,7487100,7922000,7702100,7538400,7784900
a2,harmonic,20,total,5,255891100,230679000,225198800,224679200,231961200,229345900,239388900
a2,harmonic,1000,encode,5,236541500,225989300,234962900,228302600,242934600,223412700,224916500
a2,harmonic,1000,mux,100,4720400,4433400,5138000,5069600,4893400,5390600,5313400
a2,harmonic,1000,total,5,237234200,226630600,242732800,233691600,227195400,249888400,229012300
a2,noise,20,encode,5,605588700,600847100,628023300,605387600,677923800,615856500,604454100
a2,noise,20,mux,100,7811000,7639300,8760100,7617300,9036200,7753300,8093800
a2,noise,20,total,5,613580000,601588100,608952800,673639900,701601700,621736600,609560500
a2,noise,1000,encode,5,620496300,605583300,608866800,616070400,647888300,675706600,678737100
a2,noise,1000,mux,100,4554900,4185700,4023100,4397500,4757600,4908100,4738200
a2,noise,1000,total,5,621428400,622407800,607556900,632252700,679152900,727623900,730892800
```

## Annexe C — candidat CRC testé puis rejeté

Pour reproduire l'expérience rejetée, remplacer **seulement** la fonction `crc`
de `src/ogg.rs` par ce bloc, garder absolument tous les réglages codec et le même
banc, puis reconstruire un deuxième binaire et le préserver. Ce bloc n'est pas
appliqué dans l'arbre final. Table construite à la compilation, pas à l'exécution.

```rust
// Ogg uses the non-reflected 0x04c11db7 polynomial, initial value zero,
// and no final xor. A byte table replaces eight bit steps without changing CRCs.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for byte in bytes {
        let index = usize::from((crc >> 24) as u8 ^ *byte);
        crc = (crc << 8) ^ CRC_TABLE[index];
    }
    crc
}
```

Ne pas déduire de ce résultat que tout CRC table est intrinsèquement plus lent
ou qu'une autre architecture aurait les mêmes résultats. Il suffit ici pour
**ne pas conserver** cette optimisation sans preuve d'un gain utile. Une future
campagne pourrait instrumenter séparément setup/encode natif ou CPU processus,
avec profilage d'échantillonnage ; aucune telle attribution n'est inventée ici.
