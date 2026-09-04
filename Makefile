.DEFAULT_GOAL := help

CARGO ?= cargo

.PHONY: help
help: ## Display this help screen
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}'

.PHONY: all
all: fmt-check clippy test ## Run all checks (format check, clippy, tests)

.PHONY: build
build: ## Build the project in debug mode
	$(CARGO) build

.PHONY: build-release
build-release: ## Build the project in release mode
	$(CARGO) build --release

.PHONY: check
check: ## Type check all targets
	$(CARGO) check --all-targets

.PHONY: fmt
fmt: ## Format all Rust source files
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting of Rust source files
	$(CARGO) fmt --all -- --check

.PHONY: clippy
clippy: ## Run Clippy with warnings treated as errors
	$(CARGO) clippy --all-targets -- -D warnings

.PHONY: clippy-fix
clippy-fix: ## Automatically fix Clippy suggestions where possible
	$(CARGO) clippy --all-targets --fix --allow-dirty --allow-staged

.PHONY: test
test: ## Run tests across the workspace
	$(CARGO) test --workspace

.PHONY: test-cargo
test-cargo: ## Run standard cargo tests across all targets
	$(CARGO) test --workspace --all-targets

.PHONY: test-all
test-all: ## Run all tests including unit, integration, and doc tests
	$(CARGO) test --workspace --all-targets
	$(CARGO) test --workspace --doc

.PHONY: run
run: ## Run the rho CLI
	$(CARGO) run --

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean
