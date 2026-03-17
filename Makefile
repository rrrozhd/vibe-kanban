# ─── Vibe Kanban Development Makefile ────────────────────────────────────────
#
# Quick start (local-only, SQLite backend):
#   make dev
#
# Full stack (Postgres + remote server + local frontend):
#   make dev-full
#
# ─────────────────────────────────────────────────────────────────────────────

SHELL := /bin/bash

# Ports
REMOTE_PORT     ?= 9081
FRONTEND_PORT   ?= $(shell node scripts/setup-dev-environment.js frontend 2>/dev/null || echo 3000)
BACKEND_PORT    ?= $(shell node scripts/setup-dev-environment.js backend 2>/dev/null || echo 3001)

# Remote server env
REMOTE_DB_NAME  ?= remote
REMOTE_DB_URL   ?= postgres://$(USER)@localhost/$(REMOTE_DB_NAME)
JWT_SECRET      ?= $(shell python3 -c "import base64,os;print(base64.b64encode(os.urandom(32)).decode())" 2>/dev/null)

# Files
PORTS_FILE      := .dev-ports.json
PID_DIR         := .pids
JWT_SECRET_FILE := .dev-jwt-secret

# ─── Dependency checks ──────────────────────────────────────────────────────

.PHONY: check-deps
check-deps:
	@command -v cargo   >/dev/null || { echo "ERROR: cargo not found. Install Rust: https://rustup.rs"; exit 1; }
	@command -v node    >/dev/null || { echo "ERROR: node not found. Install Node.js"; exit 1; }
	@command -v psql    >/dev/null || { echo "ERROR: psql not found. Install postgresql: brew install postgresql@17"; exit 1; }

# ─── Local development (SQLite, no Postgres) ────────────────────────────────

.PHONY: dev
dev: check-deps ## Start local dev server (frontend + backend, SQLite)
	@npx pnpm install --frozen-lockfile 2>/dev/null || true
	@echo "Starting local dev server..."
	@export FRONTEND_PORT=$$(node scripts/setup-dev-environment.js frontend) && \
	 export BACKEND_PORT=$$(node scripts/setup-dev-environment.js backend) && \
	 export PREVIEW_PROXY_PORT=$$(node scripts/setup-dev-environment.js preview_proxy) && \
	 export VK_ALLOWED_ORIGINS="http://localhost:$${FRONTEND_PORT}" && \
	 export VITE_VK_SHARED_API_BASE=$${VK_SHARED_API_BASE:-} && \
	 npx concurrently \
	   --names "backend,frontend" \
	   --prefix-colors "blue,green" \
	   "DISABLE_WORKTREE_CLEANUP=1 RUST_LOG=info cargo run --bin server" \
	   "cd packages/local-web && npx pnpm run dev -- --port $${FRONTEND_PORT}"

# ─── Full stack (Postgres + remote + local frontend) ────────────────────────

.PHONY: dev-full
dev-full: check-deps ensure-postgres ensure-remote-db ensure-jwt-secret ## Start full stack (Postgres + remote server + local frontend)
	@echo ""
	@echo "Starting full stack..."
	@echo "  Remote server: http://localhost:$(REMOTE_PORT)"
	@export FRONTEND_PORT=$$(node scripts/setup-dev-environment.js frontend) && \
	 export BACKEND_PORT=$$(node scripts/setup-dev-environment.js backend) && \
	 export PREVIEW_PROXY_PORT=$$(node scripts/setup-dev-environment.js preview_proxy) && \
	 export VK_ALLOWED_ORIGINS="http://localhost:$${FRONTEND_PORT}" && \
	 export VK_SHARED_API_BASE="http://localhost:$(REMOTE_PORT)" && \
	 npx concurrently \
	   --names "remote,backend,frontend" \
	   --prefix-colors "magenta,blue,green" \
	   "$(MAKE) --no-print-directory run-remote" \
	   "sleep 3 && DISABLE_WORKTREE_CLEANUP=1 RUST_LOG=info cargo run --bin server" \
	   "cd packages/local-web && npx pnpm run dev -- --port $${FRONTEND_PORT}"

.PHONY: dev-remote
dev-remote: check-deps ensure-postgres ensure-remote-db ensure-jwt-secret ## Start only the remote server (Postgres-backed)
	@$(MAKE) --no-print-directory run-remote

# Internal target — runs the remote binary with all env vars
.PHONY: run-remote
run-remote:
	@JWT=$$(cat $(JWT_SECRET_FILE)) && \
	 SERVER_DATABASE_URL="$(REMOTE_DB_URL)" \
	 VIBEKANBAN_REMOTE_JWT_SECRET="$$JWT" \
	 ELECTRIC_URL="http://localhost:5133" \
	 GITHUB_OAUTH_CLIENT_ID="$${GITHUB_OAUTH_CLIENT_ID:-dev-placeholder}" \
	 GITHUB_OAUTH_CLIENT_SECRET="$${GITHUB_OAUTH_CLIENT_SECRET:-dev-placeholder}" \
	 SERVER_PUBLIC_BASE_URL="http://localhost:$(REMOTE_PORT)" \
	 SERVER_LISTEN_ADDR="0.0.0.0:$(REMOTE_PORT)" \
	 RUST_LOG=info \
	 cargo run --manifest-path crates/remote/Cargo.toml --bin remote

# ─── Docker-based full stack ────────────────────────────────────────────────

.PHONY: dev-docker
dev-docker: ## Start full stack via Docker Compose (requires Docker)
	@command -v docker >/dev/null || { echo "ERROR: docker not found"; exit 1; }
	@test -f crates/remote/.env.remote || { echo "ERROR: crates/remote/.env.remote not found. Copy .env.remote.example and fill in values."; exit 1; }
	cd crates/remote && docker compose --env-file .env.remote up --build

.PHONY: dev-docker-down
dev-docker-down: ## Stop Docker Compose stack and remove volumes
	cd crates/remote && docker compose --env-file .env.remote down -v

# ─── Database management ────────────────────────────────────────────────────

.PHONY: ensure-postgres
ensure-postgres:
	@if ! pg_isready -q 2>/dev/null; then \
	  echo "Starting PostgreSQL..."; \
	  brew services start postgresql@17 2>/dev/null || \
	  pg_ctl -D /opt/homebrew/var/postgresql@17 start 2>/dev/null || \
	  { echo "ERROR: Could not start PostgreSQL. Start it manually."; exit 1; }; \
	  sleep 2; \
	fi
	@pg_isready -q || { echo "ERROR: PostgreSQL is not running"; exit 1; }

.PHONY: ensure-remote-db
ensure-remote-db: ensure-postgres
	@if ! psql -lqt 2>/dev/null | cut -d\| -f1 | grep -qw $(REMOTE_DB_NAME); then \
	  echo "Creating database '$(REMOTE_DB_NAME)'..."; \
	  createdb $(REMOTE_DB_NAME); \
	fi

.PHONY: db-reset
db-reset: ensure-postgres ## Drop and recreate the remote database (destructive!)
	@echo "Dropping database '$(REMOTE_DB_NAME)'..."
	@psql -d postgres -c "DROP DATABASE IF EXISTS $(REMOTE_DB_NAME);" 2>/dev/null
	@psql -d postgres -c "DROP ROLE IF EXISTS electric_sync;" 2>/dev/null
	@createdb $(REMOTE_DB_NAME)
	@echo "Database '$(REMOTE_DB_NAME)' recreated. Migrations will run on next server start."

# ─── JWT secret management ──────────────────────────────────────────────────

.PHONY: ensure-jwt-secret
ensure-jwt-secret:
	@if [ ! -f $(JWT_SECRET_FILE) ]; then \
	  echo "Generating JWT secret..."; \
	  python3 -c "import base64,os;print(base64.b64encode(os.urandom(32)).decode())" > $(JWT_SECRET_FILE); \
	  echo "$(JWT_SECRET_FILE)" >> .gitignore 2>/dev/null; \
	fi

# ─── Build & quality ────────────────────────────────────────────────────────

.PHONY: check
check: ## Run all type checks (frontend + backend)
	npx pnpm run check

.PHONY: lint
lint: ## Run all linters
	npx pnpm run lint

.PHONY: fmt format
fmt format: ## Format all code
	npx pnpm run format

.PHONY: test
test: ## Run Rust tests
	cargo test --workspace

.PHONY: build
build: ## Build the local NPX package
	npx pnpm run build:npx

.PHONY: generate-types
generate-types: ## Regenerate TypeScript types from Rust
	cargo run --bin generate_types
	cargo run --manifest-path crates/remote/Cargo.toml --bin remote-generate-types

# ─── Cleanup ────────────────────────────────────────────────────────────────

.PHONY: clean
clean: ## Remove build artifacts and dev state
	rm -f $(PORTS_FILE)
	rm -rf target/debug/server target/debug/remote
	@echo "Cleaned."

# ─── Help ────────────────────────────────────────────────────────────────────

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
	  awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
