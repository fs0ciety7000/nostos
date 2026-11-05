# Nostos

Carnet de voyage cartographique : les photos Fujifilm posées sur un globe,
l'itinéraire retracé, et une galerie aux couleurs des recettes de film.

Étapes livrées : 0 (lecture des métadonnées) et 1a (API et base de données).

## Contenu

| Chemin | Rôle |
|---|---|
| `crates/nostos-exif` | Bibliothèque Rust sans dépendance : GPS, date, exposition et réglages Fujifilm (simulation de film, grain, Color Chrome, tonalités, balance des blancs, plage dynamique) depuis un JPEG ou un RAF. |
| `crates/nostos-api` | API HTTP (axum) sur PostgreSQL + PostGIS : voyages, envoi des photos, lecture en tâche de fond, miniatures, palette, escales et itinéraire. |
| `crates/nostos-cli` | Commande `nostos` : `scan` (GeoJSON d'un dossier, hors ligne) et `import` (envoi d'un dossier à une instance). |
| `scripts/` | Fixtures de test, démo, tests de bout en bout, extraction du fond de carte. |

## Commande `nostos`

```bash
cargo install --path crates/nostos-cli      # installe la commande nostos
```

**Lire un dossier, hors ligne** :

```bash
nostos scan ~/Photos/Quebec -o quebec.geojson
```

Les photos sans GPS sont gardées avec `"geometry": null`. Un résumé (nombre de
vues par simulation de film) s'affiche sur la sortie d'erreur.

**Envoyer un dossier à Nostos** :

```bash
export NOSTOS_API_URL=https://nostos-api.fs0ciety.org
export NOSTOS_OWNER_TOKEN=…                  # le jeton propriétaire de l'instance
nostos import ~/Photos/Quebec --trip quebec-2026 --title "L'été indien" --wait
```

La commande lit d'abord les photos sur place et annonce ce qu'elle a trouvé
(GPS, simulations, fichiers illisibles), crée le voyage si `--title` est donné,
saute les fichiers déjà envoyés (même nom, même taille), envoie en parallèle
(`--jobs`, 4 par défaut) directement vers R2 avec reprise en cas d'erreur
réseau, puis avec `--wait` suit le traitement et liste les escales.
`--dry-run` montre ce qui serait envoyé sans rien envoyer. Relancer la même
commande reprend là où un envoi s'est arrêté. Le jeton n'est lu que dans
l'environnement, jamais en argument (il finirait dans l'historique du shell).

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

## Atlas web (étape 1b)

SvelteKit 2 (Svelte 5) dans `web/`, MapLibre GL 5 en projection globe :

- au repos, les continents en points et un halo orangé, la carte Protomaps
  apparaissant en zoomant ; l'itinéraire porte une lueur qui le parcourt ;
- choisir une odyssée cadre ses escales, choisir une escale fait voler la caméra
  jusqu'au lieu et ouvre la galerie, dont l'accent vient de la palette de la vue ;
- la recette (grain, Color Chrome, tonalités, balance des blancs, plage dynamique)
  s'affiche comme dans le menu du boîtier ; `?trip=…&stop=…` donne un lien direct.

Le jeton propriétaire est saisi une fois sur `/login` et gardé dans un cookie
`httpOnly` ; le serveur SvelteKit relaie les appels à l'API, le navigateur ne
voit jamais le jeton. Configuration : `web/.env.example`.

```bash
cd web && npm install
NOSTOS_API_URL=http://127.0.0.1:8080 npm run dev
```

**Fond de carte** : `scripts/make-basemap.sh` extrait du planet Protomaps les
régions de `tiles/regions.geojson` (Québec, Lisbonne) avec l'outil officiel
`pmtiles`, à déposer ensuite sur R2 (`PUBLIC_BASEMAP_URL`). Sans cette variable,
la route `/basemap.pmtiles` relaie le planet du jour (`BASEMAP_UPSTREAM`), pour
le développement seulement.

**En place sur Cloudflare** (novembre 2026) :

| Élément | Valeur |
|---|---|
| Bucket R2 | `nostos-media` (région ENAM) |
| Domaine public | `https://nostos-media.fs0ciety.org` |
| Fond de carte | `tiles/nostos-basemap.pmtiles` : planet Protomaps du 4 novembre 2026, régions de `tiles/regions.geojson`, zoom 0 à 14, 500 Mo |
| CORS | `GET`/`HEAD` avec `Range`, depuis `https://nostos.fs0ciety.org`, `http://localhost:5173`, `http://127.0.0.1:4173` |

Hors des deux régions, la carte détaillée s'arrête au zoom 6 ; les points des
continents restent en filigrane jusqu'au zoom 9 pour que la limite se voie peu.

**Originaux privés** : le domaine public sert tout le bucket `nostos-media`.
L'API range donc les originaux (GPS complet) dans un second bucket, sans domaine
public (`S3_BUCKET_ORIGINALS`), et seulement les miniatures, aperçus et tuiles
dans `nostos-media` (`S3_BUCKET_MEDIA`). En local, ce sont deux dossiers,
`originals/` et `media/`, et seul le second est servi.

**Tests** :

```bash
cd web && npm test && npm run check     # unitaires et types
scripts/e2e.sh                          # bout en bout : base jetable, API, démo, build, Playwright
```

`scripts/e2e.sh` crée une base PostGIS jetable, lance l'API, envoie un voyage
de démo (`scripts/seed-demo.py` : aperçus de RAF publics CC0 auxquels exiftool
donne positions, dates et simulations), sert le build de production et lance
Playwright sur ordinateur et mobile. Requiert PostgreSQL + PostGIS, exiftool et Node 22.
