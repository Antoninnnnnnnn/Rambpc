# Entrées fichier : préflight borné et types pris en charge

Les sources fichier du backend acceptent des fichiers réguliers : WAV strict
PCM16LE mono16k (`WavAudio`) et Ogg/Opus validé dans la CLI. Un lien symbolique vers
un fichier régulier est permis ; le descripteur ouvert est ensuite conservé.
Pour un flux vivant, fournir un `AudioSource` asynchrone coopératif : les commandes
fichier ne constituent pas une interface FIFO, pipe, console ou périphérique.

## Défaut reproduit

Sur le binaire Linux natif du snapshot R5, une FIFO sans producteur passée à
`dictate` puis `dictate-wav`, avec `--timeout 1`, ne terminait pas. Chaque processus
a dû être arrêté par une limite externe de2s (`timeout` exit124, stdout vide).
Le délai RPC n'avait pas encore commencé : `File::open` attendait le producteur.
Deux essais locaux seulement, sans Google, credentials ni données audio.

Une première correction fondée seulement sur `metadata().is_file()` a encore
échoué sur un pipe Windows : un des deux tests a dépassé5s. Le test tue son enfant
sur timeout ; il n'immobilise pas le runtime de test dans un open système bloqué.
Cette tentative n'est pas présentée comme un succès Windows.

## Correction

- Unix : `O_NONBLOCK` à l'ouverture, puis vérification du type sur le descripteur.
  Les fichiers réguliers conservent leur sémantique normale. La vérification ne
  dépend pas d'un contrôle de chemin vulnérable à un remplacement par une FIFO.
- Windows : `GetFileType` exige `FILE_TYPE_DISK` avant metadata/lecture. Une simple
  metadata peut ne pas distinguer correctement un pipe. L'appel FFI n'utilise
  qu'un handle vivant emprunté et ne le ferme ni ne conserve de pointeur.
- Échec avant lecture : fermeture attendue du fichier possédé, erreur source,
  aucun RPC ni résultat. Aucun EOF ou EOS synthétique.

`libc` (Unix) et `windows-sys` (Windows) étaient déjà présents dans le lock ; ils
sont maintenant des dépendances directes ciblées pour ces constantes/API. La
feature Tokio `process` ajoutée à la CLI est **dev-only**, pour tuer les enfants
de test sur timeout, pas pour exécuter un encodeur ou interpréteur au runtime.

## Filets de régression

`crates/rambler-cli/tests/file_inputs.rs` vérifie : répertoire, FIFO Unix sans
producteur, lien vers FIFO, `/dev/null`, `/dev/zero`, et pipe Windows sans données.
Chaque commande Windows utilise une instance fraîche. Les tests exigent erreur
JSON audio, absence de chemin/texte publié, stderr vide et zéro connexion TCP.
Les tests WAV/Opus existants vérifient que les fichiers réguliers restent intacts.
Le snapshot Linux R6 passe également la suite : le vrai binaire natif rejette
ensuite les deux commandes FIFO avec exit1 et53octets de JSON, sans attendre
la limite externe de2s. Les anciennes captures exit124 sont conservées séparément.

Cette correction ne promet pas de rendre annulable toute opération disque, tout
partage réseau ou toute source utilisateur non coopérative. Le délai réseau ne
constitue pas une garantie absolue sur le noyau ou le matériel sous-jacent.
