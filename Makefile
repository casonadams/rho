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

.PHONY: wasm
wasm: ## Build rho-wasm and generate JS bindings into www/hub/wasm
	@if [ -d "/opt/homebrew/opt/llvm/bin" ]; then \
		CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
		AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar \
		$(CARGO) build -p rho-wasm --target wasm32-unknown-unknown --release; \
	else \
		$(CARGO) build -p rho-wasm --target wasm32-unknown-unknown --release; \
	fi
	@WASM="target/wasm32-unknown-unknown/release/rho_wasm.wasm"; \
	HASH_FILE="target/wasm32-unknown-unknown/release/.bindgen-hash"; \
	HASH_CMD=$$(command -v sha256sum 2>/dev/null || echo "shasum -a 256"); \
	CURRENT_HASH=$$($$HASH_CMD "$$WASM" | cut -d ' ' -f 1); \
	if [ ! -f "$$HASH_FILE" ] || [ "$$(cat "$$HASH_FILE" 2>/dev/null)" != "$$CURRENT_HASH" ] || [ ! -f "www/hub/wasm/rho_wasm.js" ]; then \
		wasm-bindgen "$$WASM" --out-dir www/hub/wasm --target web && \
		echo "$$CURRENT_HASH" > "$$HASH_FILE"; \
	fi

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean

.PHONY: coverage
coverage: ## Generate LCOV test coverage trace
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; $(CARGO) llvm-cov --workspace --lcov --output-path target/lcov.info

.PHONY: crap
crap: coverage ## Evaluate CRAP metrics and gate on baseline regressions
	$(CARGO) crap --path . --lcov target/lcov.info --baseline crap-baseline.json --fail-regression

.PHONY: crap-baseline
crap-baseline: coverage ## Regenerate crap-baseline.json from current coverage
	$(CARGO) crap --path . --lcov target/lcov.info --format json --sort file --output crap-baseline.json
	@python3 -c "import json; p='crap-baseline.json'; d=json.load(open(p)); d.pop('diagnostics', None); json.dump(d, open(p, 'w'), indent=2); open(p, 'a').write('\n')"
