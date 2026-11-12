# Inventaire local de toolchain

Inventaire **initial, avant préparation du venv**, sur Windows 11 Pro ; ce n'est
pas l'état actuel des dépendances du projet. Le moteur dispose ensuite de son
propre environnement testé : voir `pyproject.toml`, `constraints-tested.txt` et
`docs/STATUS.md`. Vérifier les versions avant reproduction. Aucun paquet global
n'avait été installé et aucun téléchargement tiers exécuté lors de cet inventaire.

| Outil | Disponibilité observée | Usage potentiel / limite |
|---|---|---|
| RTK | `rtk 1.1.0` | Proxy de commandes; non lié au moteur. |
| Python | `Python 3.14.5` (`python`, `py`) | Scripts de corpus et rapports possibles. `grpc`, `pytest`, `soundfile`, `numpy` absents dans l'interpréteur isolé vérifié; tentative de sonder `google.protobuf` échouée car namespace `google` absent. Pas d'environnement Python de test prêt. |
| Rust | `rustc 1.99.1`, `cargo 1.99.1` | Meilleur candidat initial pour cœur natif et mesures monotones; disponibilité du compilateur vérifiée, mais aucun projet/dependency Rust dans le dépôt vide. |
| Java | OpenJDK `17.0.19` | JDK/Javap disponibles; `jadx` non trouvé via `command -v`. Pas de preuve d'outillage Android/JADX. |
| FFmpeg | `8.1.1` build Windows full | Disponible pour inspecter/convertir audio et normaliser fixtures; consigner la commande et les options de codec exactes. Présence de support whisper dans le build ne prouve pas un moteur Rambler exploitable. |
| Git | disponible | Dépôt initialement vide hormis `.gitignore`; aucune source d'implémentation ni test existants. |

Les vérifications Python ont été faites avec `python -I` pour éviter les modules importés depuis le répertoire courant. Les commandes et versions sont un constat à l'instant de l'inspection, pas des résultats de benchmark. Les outils absents ne doivent pas être installés globalement pour cette tâche.

## Recommandation

Démarrer en Rust pour l'exécutable/client natif et mesurer avec `std::time::Instant`, en gardant moteur, transport et source audio derrière des interfaces distinctes. Utiliser FFmpeg uniquement comme utilitaire explicite de préparation/inspection audio; Python est facultatif pour transformer les journaux en résumé, après choix d'un environnement local reproductible. Aucun choix de client gRPC ou de bibliothèque protobuf n'est justifié tant que protocole, schéma et disponibilité du service Rambler n'ont pas été prouvés. Ne pas supposer S3 = moteur. Pas de microbenchmark avant un parcours bout-en-bout fonctionnel et instrumenté.

## Reproduction de l'inventaire

Depuis un shell Windows similaire, exécuter `python --version`, `rustc --version`, `cargo --version`, `java -version`, `ffmpeg -version`, `command -v jadx`, `python -I -c "import importlib.util as u; print({x:bool(u.find_spec(x)) for x in ['grpc','pytest','soundfile','numpy']})"`. Vérifier `google.protobuf` séparément avec import protégé, car `find_spec` peut lever `ModuleNotFoundError` lorsque le parent `google` manque. Ne pas interpréter l'absence d'un exécutable dans `PATH` comme preuve qu'il ne peut être installé; ici, on s'en tient aux outils effectivement présents.
