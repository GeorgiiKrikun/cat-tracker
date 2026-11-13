# 🐱 Cat tracker

A small installable web app (PWA) for logging a cat's symptoms and medications from two phones.

- **One-tap logging.** Meds, vomiting and other "it happened" items log on tap, with Undo/Edit in the toast. Scales (appetite, drinking…) take one more tap.
- **Configurable symptoms.** Add, rename, pin, reorder or archive symptoms in Settings. New symptoms need no code change.
- **Who did what.** Every entry records who logged it, which prevents double-dosing meds.
- **Vet summary.** 7/14/30-day charts per symptom, printable as a PDF, plus CSV export.
- **Works offline.** Entries logged without signal queue on the phone and sync later.

Stack: **Rust** (Axum + SQLx + SQLite, argon2 passwords, cookie sessions) serving a **Svelte + Vite** PWA. One container, behind Nginx Proxy Manager.

```
backend/    Rust server: JSON API under /api + serves the built PWA
  migrations/   SQL schema + starter symptoms
frontend/   Svelte PWA (manifest, service worker, icons)
Dockerfile  builds both into one small image
compose.yml         production (Nginx Proxy Manager network)
compose.local.yml   run the production container locally
Makefile            dev shortcuts
```

## Local development

Needs Rust (stable) and Node 20+. For auto-reload: `cargo install cargo-watch`.

```bash
make seed            # demo user demo/demo + two weeks of fake data
make dev-backend     # terminal 1: API on :8080
make dev-frontend    # terminal 2: UI on :5173 with hot reload
```

Open http://localhost:5173 and log in as `demo` / `demo`.

Create real users with `make user NAME=georgii DISPLAY=Georgii`. You'll be prompted for the password.

Other commands: `make test` (Rust tests + Svelte type check), `make clean-db`.

### Testing on a phone

A PWA only installs and works offline over **HTTPS** (or on `localhost`), so `http://192.168.x.x:5173` won't install. Two options:

- **Install test:** temporarily point an NPM proxy host at your laptop (e.g. `cat-dev.yourdomain.com → http://<laptop-ip>:8080` after `make build`, running the backend with `CT_STATIC_DIR=../frontend/dist`). Or use a tunnel like `cloudflared tunnel --url http://localhost:5173`.
- **UI check only:** plain `http://<laptop-ip>:5173` is fine for clicking around. Service worker and install won't work, but logging does.

The service worker only registers in production builds, so it never caches stale code during `npm run dev`.

## Deploying behind Nginx Proxy Manager

On the server:

```bash
git clone <this repo> cat-tracker && cd cat-tracker
docker network ls                         # find NPM's network, e.g. npm_default
mkdir -p data && sudo chown 10001 data    # container runs as uid 10001
NPM_NETWORK=npm_default docker compose up -d --build
docker compose exec cattracker cattracker add-user georgii --display-name Georgii
docker compose exec cattracker cattracker add-user <wife> --display-name <Name>
```

In the Nginx Proxy Manager UI:

1. **Proxy Hosts → Add:** domain `cat.yourdomain.com`, scheme `http`, forward hostname `cattracker`, port `8080`. Turn on *Block Common Exploits*.
2. **SSL tab:** request a new Let's Encrypt certificate, and turn on *Force SSL*, *HTTP/2 Support* and *HSTS*.

Then open the URL on each phone:

- **iPhone:** in Safari, tap Share, then "Add to Home Screen".
- **Android:** in Chrome, open the ⋮ menu and choose "Install app".

Sessions last 90 days and renew while the app is used, so you won't keep logging in.

**Updating:** `git pull && docker compose up -d --build`.

**Backups:** everything is in `data/cattracker.db`. For a consistent copy while it's running:

```bash
sqlite3 data/cattracker.db ".backup data/backup-$(date +%F).db"
```

…then sync that file to Nextcloud or wherever you keep backups.

## User management (CLI)

```bash
cattracker add-user <username> [--display-name Name]   # prompts for password (or CT_PASSWORD env)
cattracker passwd <username>                           # also logs them out everywhere
cattracker del-user <username>                         # their entries stay, without a name
cattracker users
```

In Docker, prefix these with `docker compose exec cattracker`.

## Configuration (env vars)

| Variable | Default | Meaning |
|---|---|---|
| `CT_DB_PATH` | `data/cattracker.db` (`/data/…` in Docker) | SQLite file, created if missing |
| `CT_BIND` | `0.0.0.0:8080` | Listen address |
| `CT_STATIC_DIR` | `../frontend/dist` (`/app/static` in Docker) | Built PWA |
| `CT_COOKIE_SECURE` | `true` | Keep `true` behind HTTPS; `false` only for plain-http local dev |
| `RUST_LOG` | `cattracker=info` | Log level |

## How symptoms work

Symptoms are rows in `symptom_types`, and each has a **kind** that decides how it's logged:

| Kind | Logging | Example |
|---|---|---|
| `med` | one tap = dose given (optional default dose) | Omeprazole |
| `bool` | one tap = it happened | Sneezed |
| `count` | one tap = one occurrence | Vomited |
| `scale` | pick from ordered options (least → most) | Appetite: none / little / normal / good |
| `choice` | pick from options | Stool: normal / soft / diarrhea / … |
| `number` | type a value with a unit | Weight (kg), glucose (mmol/L) |
| `note` | free text | Anything unusual |

The starter set lives in `backend/migrations/0001_init.sql`. Everything is editable in the app. A symptom's kind can't change after creation, because that would make old entries meaningless; archive it and create a new one instead.

## API (all JSON, cookie auth)

```
POST   /api/auth/login        {username, password}
POST   /api/auth/logout
GET    /api/me
GET    /api/symptoms[?include_archived=true]
POST   /api/symptoms          {name, kind, options?, unit?, icon?, pinned?}
PATCH  /api/symptoms/{id}     any of: name, options, unit, icon, pinned, archived
PUT    /api/symptoms/order    [id, id, …]
GET    /api/entries?from=&to=&symptom_id=&limit=
POST   /api/entries           {id?: uuid, symptom_id, value?, note?, occurred_at?}   (idempotent on id)
PATCH  /api/entries/{id}      {value?, note?, occurred_at?}
DELETE /api/entries/{id}
GET    /api/export.csv?from=&to=
```

## Ideas for later

- Passkeys (Face ID / fingerprint login) via `webauthn-rs`
- Photo attachments (stool, vomit, eyes) for the vet
- Push reminders for meds (works on iOS for installed PWAs)
- A "cat profile" (name, vet contact, current dosage plan) on the summary printout
