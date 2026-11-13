# Local development helpers. Run `make dev-backend` and `make dev-frontend`
# in two terminals, then open http://localhost:5173

DB ?= data/cattracker.db

.PHONY: dev-backend dev-frontend seed user test check build docker-local clean-db

dev-backend:   ## Rust API on :8080, restarts on changes (needs: cargo install cargo-watch)
	cd backend && CT_COOKIE_SECURE=false CT_DB_PATH=$(DB) cargo watch -q -c -x 'run -- serve'

dev-frontend:  ## Vite dev server on :5173 with hot reload, proxies /api to :8080
	cd frontend && npm install && npm run dev

seed:          ## Demo user demo/demo + two weeks of fake entries
	cd backend && CT_DB_PATH=$(DB) cargo run -q -- seed-demo

user:          ## Create a real user: make user NAME=georgii DISPLAY=Georgii
	cd backend && CT_DB_PATH=$(DB) cargo run -q -- add-user $(NAME) --display-name "$(or $(DISPLAY),$(NAME))"

test:          ## Backend tests + frontend type check
	cd backend && cargo test
	cd frontend && npm run check

build:         ## Production build of both parts (backend serves frontend/dist)
	cd frontend && npm run build
	cd backend && cargo build --release

docker-local:  ## Run the production container locally on :8080
	docker compose -f compose.local.yml up --build

clean-db:      ## Delete the local dev database
	rm -f backend/$(DB) backend/$(DB)-shm backend/$(DB)-wal
