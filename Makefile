.DEFAULT_GOAL := help

CARGO ?= cargo
CPUS ?= $(shell getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
export CARGO_BUILD_JOBS ?= $(CPUS)
export RUST_TEST_THREADS ?= $(CPUS)
COGNITIVE_MAX := $(shell sed -n 's/^max-cognitive *= *\([0-9]*\).*/\1/p' cccc.toml)

.PHONY: help
help: ## Display this help screen
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}'

.PHONY: all
all: fmt-check clippy complexity test ## Run all checks (format check, clippy, complexity, tests)

.PHONY: build
build: ## Build the project in debug mode
	$(CARGO) build

.PHONY: build-release
build-release: ## Build the project in release mode
	$(CARGO) build --release

.PHONY: check
check: ## Type check all targets
	$(CARGO) check --workspace --all-targets

.PHONY: fmt
fmt: ## Format all Rust source files
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting of Rust source files
	$(CARGO) fmt --all -- --check

.PHONY: clippy
clippy: ## Run Clippy with warnings treated as errors
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: clippy-fix
clippy-fix: ## Automatically fix Clippy suggestions where possible
	$(CARGO) clippy --workspace --all-targets --fix --allow-dirty --allow-staged

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean

.PHONY: test
test: ## Run the workspace tests (uses nextest when available)
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) nextest run --workspace --all-targets && $(CARGO) test --workspace --doc; \
	else \
		$(CARGO) test --workspace; \
	fi

.PHONY: complexity
complexity: ## Gate on the cognitive complexity limit set in cccc.toml
	@command -v cccc >/dev/null 2>&1 || { echo "Error: cccc not found. Install with: cargo install cccc-cli"; exit 1; }
	@out=$$(cccc .) || { echo "$$out" | jq -c '.files[]? | .path as $$path | .functions[]? | select(.cognitive > $(COGNITIVE_MAX)) | {file: $$path, line, function: .name, cognitive, cyclomatic}'; exit 1; }
