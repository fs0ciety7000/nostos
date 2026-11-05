# Nostos

Carnet de voyage cartographique : les photos Fujifilm posées sur un globe,
l'itinéraire retracé, et une galerie aux couleurs des recettes de film.

Étapes livrées : 0 (lecture des métadonnées) et 1a (API et base de données).

## Contenu

| Chemin | Rôle |
|---|---|
| `crates/nostos-exif` | Bibliothèque Rust sans dépendance : GPS, date, exposition et réglages Fujifilm (simulation de film, grain, Color Chrome, tonalités, balance des blancs, plage dynamique) depuis un JPEG ou un RAF. |
| `crates/nostos-api` | API HTTP (axum) sur PostgreSQL + PostGIS : voyages, envoi des photos, lecture en tâche de fond, miniatures, palette, escales et itinéraire. |
| `crates/nostos-cli` | `nostos-scan <dossier>` : GeoJSON des photos d'un dossier, triées par date. |
| `scripts/` | Fabrication des fixtures de test et de leurs références exiftool. |

## Utilisation

```bash
cargo run --release -p nostos-cli -- ~/Photos/Quebec -o quebec.geojson
```

Les photos sans GPS sont gardées avec `"geometry": null`, pour le recalage
sur une trace GPX à l'étape suivante. Un résumé (nombre de vues par
simulation de film) s'affiche sur la sortie d'erreur.

## Principes du lecteur

- **Lecture minimale** : 256 Ko au début d'un JPEG ; pour un RAF, l'en-tête
  puis le début de l'aperçu JPEG intégré. Les données du capteur ne sont
  jamais lues (testé sur un RAF factice de 40 Mo : moins de 300 Ko lus).
- **Aucune panique** sur un fichier corrompu : toutes les lectures sont
  bornées, et les tests tronquent et corrompent les fixtures des milliers de fois.
- **Pas de `unsafe`**, pas de dépendance (option `serde` pour la sérialisation),
  donc compilable en WebAssembly pour l'aperçu dans le navigateur.

## Tests

```bash
cargo test --workspace
```

`tests/golden.rs` compare chaque champ à la lecture d'exiftool 12.76 sur :

- l'en-tête Exif de 10 RAF publics (X-T5, X100VI, X-H2, X-S20, X-T4, X100V,
  X-Pro3, X-E4, X-T30 II, GFX100S), issus de [raw.pixls.us](https://raw.pixls.us)
  sous licence CC0 ;
- 4 variantes où exiftool a écrit des recettes (Classic Chrome, Acros+R,
  Nostalgic Neg. en Kelvin) et une position GPS.

Pour ajouter un de tes fichiers comme fixture (exiftool requis) :

```bash
scripts/make-fixture.py ~/Photos/DSCF1234.RAF mon-x-t5-classic-chrome
```

## Reste à valider sur de vrais fichiers

- Les réglages monochromes (Warm/Cool, Magenta/Green) : exiftool ne peut pas
  les ajouter à une note qui ne les contient pas, ils ne sont pas couverts par
  les fixtures.
- L'échelle ×20 du décalage de balance des blancs, sur ton boîtier.

## API (étape 1a)

La configuration se lit dans les variables d'environnement listées dans
`.env.example` (noms et valeurs factices uniquement). La base doit avoir
l'extension PostGIS ; les migrations s'appliquent au démarrage.

```bash
cargo run --release -p nostos-api
```

Toutes les routes sauf `/health` et `/media` demandent
`Authorization: Bearer <NOSTOS_OWNER_TOKEN>`.

| Méthode | Route | Rôle |
|---|---|---|
| `GET` | `/health` | État du service et de la base |
| `GET` `POST` | `/trips` | Lister, créer (`{slug, title}`) |
| `GET` | `/trips/{slug}` | Voyage, compteurs, chants (jours), escales, simulations, km |
| `GET` | `/trips/{slug}/photos` | Photos prêtes, en GeoJSON |
| `GET` | `/trips/{slug}/route` | Itinéraire en LineString |
| `POST` | `/trips/{slug}/uploads` | Réserve un envoi (`{file_name, byte_size}`) et renvoie l'URL où déposer le fichier |
| `PUT` | `/photos/{id}/content` | Dépôt du fichier en stockage local (en production : URL signée R2) |
| `POST` | `/photos/{id}/complete` | Vérifie la taille et met la photo en file d'attente |
| `GET` | `/photos/{id}` | Une photo, avec son statut et son éventuelle erreur |
| `PATCH` | `/stops/{id}` | Renommer une escale (`{name}`) ; le nom survit au recalcul |
| `POST` | `/trips/{slug}/stops/recompute` | Recalcule les escales |

**Escales** : regroupement spatial `ST_ClusterDBSCAN` (1,2 km), puis découpage
chronologique : revenir au même endroit un autre jour crée une nouvelle escale.
Une photo sans GPS rejoint l'escale la plus proche dans le temps (3 h). Moins de
3 photos en moins de 20 minutes : point de passage (`is_waypoint`).

**RAF** : seul l'aperçu JPEG intégré est téléchargé depuis le stockage, pour les
métadonnées comme pour les miniatures.

**Image Docker** pour Coolify : `docker build -t nostos-api .` à la racine.

Tests d'intégration (une base jetable par test, supprimée ensuite) :

```bash
NOSTOS_TEST_DATABASE_URL=postgres://nostos:nostos@localhost/postgres cargo test --workspace
```

Sans cette variable, les tests de l'API sont ignorés.
