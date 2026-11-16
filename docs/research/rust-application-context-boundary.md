# Contexte applicatif Android : frontière S3 déjà encodable

## Conclusion — 16 novembre 2026

**Aucun champ S3 manquant établi.** Sur la chaîne statique examinée, le texte de
contexte applicatif Android rejoint **S3Request → extension 1203 InputBoxInfo →
champ 5, chaîne répétée**. Rust expose et encode déjà cette représentation par
`ProtocolConfig::preferences`. Cela ne démontre ni activation runtime de cette
branche, ni interprétation ou effet du texte côté serveur.

La collecte Android (`EditorInfo`, informations de l'application), ses gates et
son formatage ne sont pas portés. Ce sont des intégrations de plateforme absentes,
pas une capacité protobuf à ajouter. Aucun nouveau switch, champ, prompt envoyé
ou essai distant n'est justifié par cette seule trace.

## Chaîne statique vérifiable

Les noms concernent le build APK identifié dans la [matrice de parité](rust-feature-parity.md#sources-et-niveaux-de-preuve).
Les fichiers décompilés sont des données locales non exécutées ; ils ne sont pas
redistribués ici.

1. `ffh.u(kmx)`, extraction ciblée `ffh-simple`, lignes 2000–2077 : sous gate
   `muq.X`, les informations applicatives alimentent un élément de la liste
   `user_preferences` du Bundle. Le gate distinct `muq.Y` ajoute des instructions
   personnalisées à la même liste, sans nouveau type de message réseau.
2. `kla.java:207–219,234–250` lit cette liste, filtre les entrées nulles et la
   transmet par `kmw.j`. `kmw.java:122–127` / `kmy.java:25–42,65–68` la nomment
   `userPreferences` et la stockent dans `kmy.o`.
3. `knt.java:78,98–110` transmet `kmy.o` par `iib.e` au constructeur `iic`, membre
   `g`, puis à `iho`. `iho.java:280–304` appelle l'initialiseur `ihv.a`.
4. Le flot d'instructions récupéré d'`ihv.a` sélectionne le reconnaisseur NewS3
   aux lignes 15–27, copie `iic.g` dans `aamo.e` aux lignes 155–164 et attache
   l'extension `aamo.f` aux lignes 205–212. La sortie JADX simplifiée avait une
   branche d'attachement incomplète ; elle n'est pas utilisée comme preuve négative.
5. `aamo.java:13–18` définit l'extension message **1203** du parent `aama` ; les
   métadonnées aux lignes 27–31 situent `e` au **champ 5, repeated string**.

Il s'agit de la requête **initiale**, pas du contexte d'édition tardif. Une liste
vide n'a pas de présence scalaire ; l'extension peut être omise si les autres
contenus InputBox sont également absents. Le mutateur de requête reste appliqué
après cet assemblage ; aucune observation dynamique de ses effets n'est déduite.

## Rust existant et preuve de sérialisation

- `crates/rambler-protocol/src/request.rs:11–31` expose `preferences: Vec<String>`.
- `initial_request`, lignes 172–180, encode chaque préférence au champ 5, chaque
  biais au champ 3, puis les enveloppe dans `INPUT_BOX_INFO` (1203).
- Le golden existant `hint_goldens_empty_entries_unicode_order_duplicates`, dans
  `crates/rambler-protocol/tests/requests.rs:191–221`, fixe la préférence `P` à
  `2a 01 50` dans cette extension et couvre ordre, doublons et Unicode.
- Les préférences restent séparées du contexte tardif. Aucun membre dédié
  `application_context` n'est nécessaire pour représenter ces mêmes octets.

La vérification de cette frontière est une lecture statique ciblée, non un nouveau
run de tests ou un appel serveur. Elle serait réfutée par une trace montrant que
ce même élément rejoint un autre champ sur cette route, ou par un échec du golden
à émettre extension 1203/champ 5. Les flags effectifs, la sélection runtime de la
route et l'effet serveur restent inconnus. Aucune parité Android bout en bout,
fonction COMPOSE, collecte OS automatique ou qualité supplémentaire n'est affirmée.
