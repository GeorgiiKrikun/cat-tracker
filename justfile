# Local development helpers. Run them inside the Nix dev shell
# (`nix develop`, or automatically with direnv), e.g. `just dev`,
# then open http://localhost:5173

db := env("CT_DB_PATH", "data/cattracker.db")

default:
    @just --list

# Backend + frontend together in one terminal (Ctrl-C stops both)
dev:
    #!/usr/bin/env bash
    trap 'kill 0' EXIT
    just dev-backend &
    just dev-frontend &
    wait -n

# `just dev`, reachable from a phone on the same Wi-Fi (opens :5173 in the NixOS firewall until you stop it)
phone:
    #!/usr/bin/env bash
    set -euo pipefail
    ip=$(ip -4 route get 1.1.1.1 | grep -oP 'src \K\S+')
    if command -v nixos-firewall-tool >/dev/null; then
        sudo nixos-firewall-tool open tcp 5173
        # There is no "close one port"; reset drops all temporary rules.
        trap 'echo "Closing :5173"; sudo nixos-firewall-tool reset' EXIT
    fi
    echo
    echo "  On your phone (same Wi-Fi): http://$ip:5173"
    echo "  Browsing only: installing the app needs HTTPS, see 'just phone-install'."
    echo
    just dev

# Production build behind a temporary public HTTPS URL, to install the PWA on a phone
phone-install: build
    #!/usr/bin/env bash
    set -euo pipefail
    cd backend
    export CT_DB_PATH={{db}}
    bin=target/release/cattracker
    # The URL is public: refuse to expose the demo login or an empty user list.
    users=$($bin users)
    if grep -q '^demo ' <<<"$users"; then
        echo "The demo/demo user exists and this URL is public. Run 'just clean-db' and 'just user <name>' first." >&2
        exit 1
    fi
    if [ -z "$users" ]; then
        echo "No users yet. Create one first: just user <name> [Display Name]" >&2
        exit 1
    fi
    log=$(mktemp)
    trap 'kill 0; rm -f "$log"' EXIT
    # Only the tunnel reaches the server, so bind to localhost.
    CT_COOKIE_SECURE=true CT_BIND=127.0.0.1:8080 $bin serve &
    cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8080 >"$log" 2>&1 &
    url=""
    for _ in $(seq 60); do
        url=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$log" | head -1 || true)
        [ -n "$url" ] && break
        sleep 0.5
    done
    if [ -z "$url" ]; then
        echo "cloudflared didn't report a URL:" >&2
        cat "$log" >&2
        exit 1
    fi
    echo
    echo "  Open on your phone: $url"
    echo "  iPhone: Safari → Share → Add to Home Screen. Android: Chrome → ⋮ → Install app."
    echo "  The URL changes every run, so a new run means reinstalling. Ctrl-C stops it."
    echo
    wait -n

# Rust API on :8080, restarts on changes
[working-directory: 'backend']
dev-backend:
    CT_COOKIE_SECURE=false CT_DB_PATH={{db}} cargo watch -q -x 'run -- serve'

# Vite dev server on :5173 with hot reload, proxies /api to :8080
[working-directory: 'frontend']
dev-frontend:
    npm install
    npm run dev

# Demo user demo/demo + two weeks of fake entries
[working-directory: 'backend']
seed:
    CT_DB_PATH={{db}} cargo run -q -- seed-demo

# Create a real user (password is prompted): just user georgii Georgii
[working-directory: 'backend']
user name display=name:
    CT_DB_PATH={{db}} cargo run -q -- add-user {{name}} --display-name "{{display}}"

# Create a patient, optionally with starter symptoms: just patient Murka cat
[working-directory: 'backend']
patient name preset="none":
    CT_DB_PATH={{db}} cargo run -q -- add-patient "{{name}}" --preset {{preset}}

# Let a user see and log for a patient (id or name): just share Murka georgii
[working-directory: 'backend']
share patient user:
    CT_DB_PATH={{db}} cargo run -q -- share "{{patient}}" {{user}}

# Take a user's access to a patient away (their entries stay)
[working-directory: 'backend']
unshare patient user:
    CT_DB_PATH={{db}} cargo run -q -- unshare "{{patient}}" {{user}}

# List patients and who can see them
[working-directory: 'backend']
patients:
    CT_DB_PATH={{db}} cargo run -q -- patients

# Backend tests + frontend type check
test:
    cd backend && cargo test
    cd frontend && npm install && npm run check

# Production build of both parts (backend serves frontend/dist)
build:
    cd frontend && npm install && npm run build
    cd backend && cargo build --release

# Build the production image with Nix and load it as cattracker:latest
docker:
    nix build .#docker -o result-docker
    docker load < result-docker

# Run the production container locally on :8080
docker-local: docker
    docker compose -f compose.local.yml up

# Delete the local dev database
[working-directory: 'backend']
clean-db:
    rm -f {{db}} {{db}}-shm {{db}}-wal
