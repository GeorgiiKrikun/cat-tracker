# 🐱 Cat tracker

A small installable web app (PWA) for logging a cat's medications, symptoms and measurements from two phones.

- **One-tap logging.** Meds, vomiting and other "it happened" items log on tap, with Undo/Edit in the toast. Scales (appetite, drinking…) take one more tap.
- **Log only what's unusual.** Appetite, stool and the like have a normal state that's assumed on days nobody logged them. A "Nothing unusual today" button covers quiet days.
- **Configurable trackers.** Add, rename, pin, reorder or archive trackers in Settings. New ones need no code change.
- **Who did what.** Every entry records who logged it, which prevents double-dosing meds.
- **Several patients.** Track a cat together, and keep your own migraine diary private. Each patient has their own trackers, and is shared only with the users you choose.
- **Vet summary.** 7/14/30-day charts per tracker, printable as a PDF, plus CSV export.
- **Works offline.** Entries logged without signal queue on the phone and sync later.

Stack: **Rust** (Axum + SQLx + SQLite, argon2 passwords, cookie sessions) serving a **Svelte + Vite** PWA. Built with **Nix**; deployed as one container behind Nginx Proxy Manager.

```
backend/    Rust server: JSON API under /api + serves the built PWA
  migrations/   SQL schema
frontend/   Svelte PWA (manifest, service worker, icons)
flake.nix   dev shell (Rust + Node) and builds: PWA, server, Docker image
compose.local.yml   run the production container locally
justfile            dev shortcuts (`just` lists them)
```

## Local development

Needs only [Nix](https://nixos.org/download) with flakes enabled. It provides Rust, Node, cargo-watch, just and sqlite. No Docker needed.

```bash
nix develop          # enter the dev shell (or `direnv allow` once, then it's automatic)
just seed            # demo user demo/demo + two weeks of fake data
just dev             # API on :8080 + UI on :5173 with hot reload, Ctrl-C stops both
```

`just dev-backend` and `just dev-frontend` run the two halves separately, if you prefer two terminals.

Open http://localhost:5173 and log in as `demo` / `demo`. The demo user sees one patient, "Demo cat".

Create real users with `just user georgii Georgii`. You'll be prompted for the password. Then create patients and decide who can see them:

```bash
just patient Murka cat          # new patient with the cat starter trackers
just patient "Georgii" none     # empty patient; add trackers in the app's Settings
just share Murka georgii        # patients are referred to by name or id
just share Murka wife
just share Georgii georgii      # private: only shared with you
just patients                   # who can see what
just unshare Murka wife         # take access away (entries are kept)
```

A user with several patients gets a switcher at the top of the app.

Other commands: `just test` (Rust tests + Svelte type check), `just clean-db`.

To run the production build without Docker: `CT_COOKIE_SECURE=false nix run . -- serve` (serves the bundled PWA on :8080, database in `./data`).

### Testing on a phone

A PWA only installs and works offline over **HTTPS** (or on `localhost`), so `http://192.168.x.x:5173` won't install. Two options:

- **Install test:** temporarily point an NPM proxy host at your laptop (e.g. `cat-dev.yourdomain.com → http://<laptop-ip>:8080`, running `nix run . -- serve`). Or use a tunnel like `cloudflared tunnel --url http://localhost:5173`.
- **UI check only:** plain `http://<laptop-ip>:5173` is fine for clicking around. Service worker and install won't work, but logging does.

The service worker only registers in production builds, so it never caches stale code during `npm run dev`.

## Docker image

Nix builds the image (`dockerTools`), so there is no Dockerfile:

```bash
just docker          # nix build .#docker + docker load → cattracker:latest
just docker-local    # same, then run it on http://localhost:8080
```

The image contains only the server, the PWA and their runtime libraries. It has no shell or base distro.

## Deploying behind Nginx Proxy Manager

Production config lives in the [infra repo](https://github.com/GeorgiiKrikun/infra) (`cattracker/compose.yml`), not here. On every push to `main`, CI (`.github/workflows/image.yml`) builds the image with Nix and pushes `ghcr.io/georgiikrikun/cattracker:sha-<7 chars>` and `:main`. The server only pulls it. The container runs as the server's shared app user (1000:1000) and keeps its database in `/srv/data/cattracker`.

First setup, on the server in `/srv/infra`:

```bash
just deploy cattracker
docker exec cattracker cattracker add-user georgii --display-name Georgii
docker exec cattracker cattracker add-user <wife> --display-name <Name>
docker exec cattracker cattracker add-patient Murka --icon 🐱 --preset cat
docker exec cattracker cattracker share Murka georgii
docker exec cattracker cattracker share Murka <wife>
```

In the Nginx Proxy Manager UI:

1. **Proxy Hosts → Add:** domain `cat.yourdomain.com`, scheme `http`, forward hostname `cattracker`, port `8080`. Turn on *Block Common Exploits*.
2. **SSL tab:** request a new Let's Encrypt certificate, and turn on *Force SSL*, *HTTP/2 Support* and *HSTS*.

Then open the URL on each phone:

- **iPhone:** in Safari, tap Share, then "Add to Home Screen".
- **Android:** in Chrome, open the ⋮ menu and choose "Install app".

Sessions last 90 days and renew while the app is used, so you won't keep logging in.

**Updating:** push to `main`, wait for CI, set the new `sha-…` tag in infra's `cattracker/compose.yml`, then run `just deploy cattracker` on the server.

**Updating dependencies:** `nix flake update` bumps the toolchain. Cargo and npm deps come from `Cargo.lock` and `package-lock.json`, so no hashes need editing.

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

## Patients and access

A **patient** is whoever is observed: the cat, or yourself. `patient_access` lists which users can see each patient, so a patient can be shared (the cat) or private (your migraines). Trackers belong to a patient, and entries belong to a patient through their tracker. Every entry records who logged it (`logged_by`).

All tracker, entry and check-in routes live under `/api/patients/{pid}/…`. A user without access to a patient gets 404, as if it didn't exist.

## How trackers work

A **tracker** is anything you log for a patient: a medication, an event, an observation or a measurement. Trackers are rows in `trackers`, each belonging to one patient, and each has a **kind** that decides how it's logged, which group it's shown in, and what a day without an entry means:

| Kind | Group | Logging | Example | Nothing logged means |
|---|---|---|---|---|
| `med` | Medications | one tap = dose given (optional default dose) | Omeprazole | not given |
| `bool` | Events | one tap = it happened | Sneezed | didn't happen |
| `count` | Events | one tap = one occurrence | Vomited | didn't happen |
| `scale` | Observations | pick from ordered options (least → most) | Appetite: none / little / normal / good | the normal option, if it has one |
| `choice` | Observations | pick from options | Stool: normal / soft / diarrhea / … | the normal option, if it has one |
| `number` | Measurements | type a value with a unit | Weight (kg), glucose (mmol/L) | unknown |
| `note` | Notes | free text | Anything unusual | nothing |

Starter sets are presets in `backend/src/patients.rs` (`--preset cat`). Everything is editable in the app. A tracker's kind can't change after creation, because that would make old entries meaningless; archive it and create a new one instead.

### Normal days

A scale or choice can mark one option as **normal** (Settings → the tracker → "On a day without an entry"). Then you only log when something is off, and a day without an entry counts as normal. A migraine "Headache" scale would have a "none" option marked normal.

"Nothing logged" only means normal on **observed** days: days where someone logged anything for the patient, or tapped **Nothing unusual today** (a check-in, stored in `checkins`). On days nobody touched the app, nothing is assumed, because nobody looked. Normal is also not assumed before the tracker was created, or for archived trackers.

In the summary, days nobody logged anything are shaded, and assumed normal days are drawn as faint dots. The CSV export lists assumed days as rows with `source` = `assumed (nothing logged)`, next to logged entries and check-ins.

### Options (scale and choice)

Options live in `tracker_options`, and entries point at them by id. That way history stays correct when options change:

- **Rename** an option by editing its label. Every past entry shows the new name.
- **Retire** an option when you replace it with something different (e.g. "soft / hard" → "diarrhea / porridge / sausage / stone"). It's no longer offered when logging. Old entries keep it, and the summary shows it as a greyed row while it has entries in the period. There's no automatic mapping, because only you know what an old "soft" meant.
- **Merge** a retired option into another one to move its entries over, for typos or duplicates. This is the only operation that rewrites history, and it asks first.
- An option with no entries can simply be removed. One with entries can't be deleted.
- The normal option can't be retired; pick another normal option (or none) first.

## API (all JSON, cookie auth)

```
POST   /api/auth/login        {username, password}
POST   /api/auth/logout
GET    /api/me
GET    /api/patients          patients you can see: [{id, name, icon, shared_with}]

Below, P = /api/patients/{pid} (404 without access to that patient):
GET    P/trackers[?include_archived=true]
POST   P/trackers          {name, kind, options?, unit?, icon?, pinned?}
PATCH  P/trackers/{id}     any of: name, options, unit, icon, pinned, archived
                           options = full ordered list [{id?, label, retired, normal}]; no id = new
POST   P/trackers/{id}/options/{option_id}/merge   {into: option_id}
PUT    P/trackers/order    [id, id, …]
GET    P/entries?from=&to=&tracker_id=&limit=
POST   P/entries           {id?: uuid, tracker_id, value? | option_id?, note?, occurred_at?}   (idempotent on id)
PATCH  P/entries/{id}      {value? | option_id?, note?, occurred_at?}
DELETE P/entries/{id}
GET    P/checkins?from=&to=
POST   P/checkins          {id?: uuid, occurred_at?}   "nothing unusual" (idempotent on id)
DELETE P/checkins/{id}
```

The CSV export is built in the browser from what the summary shows, because only the phone knows its time zone (which day an entry belongs to).

## Ideas for later

- Passkeys (Face ID / fingerprint login) via `webauthn-rs`
- Photo attachments (stool, vomit, eyes) for the vet
- Push reminders for meds (works on iOS for installed PWAs)
- A "cat profile" (name, vet contact, current dosage plan) on the summary printout
