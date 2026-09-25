.DEFAULT_GOAL := help

CARGO ?= cargo
CPUS ?= $(shell getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
export CARGO_BUILD_JOBS ?= $(CPUS)
export RUST_TEST_THREADS ?= $(CPUS)

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

.PHONY: test
test: ## Run tests across the workspace
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) test --workspace --all-targets --quiet

.PHONY: test-cargo
test-cargo: ## Run standard cargo tests across all targets
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) test --workspace --all-targets

.PHONY: test-all
test-all: ## Run all tests including unit, integration, and doc tests
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) test --workspace --all-targets
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) test --workspace --doc

.PHONY: run
run: ## Run the rho CLI
	$(CARGO) run --

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean

.PHONY: coverage
coverage: ## Generate LCOV test coverage trace
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) llvm-cov --workspace --lcov --output-path target/lcov.info

.PHONY: crap
crap: coverage ## Evaluate CRAP metrics and gate on functions exceeding threshold 30
	$(CARGO) crap --path . --lcov target/lcov.info --threshold 30 --fail-above
