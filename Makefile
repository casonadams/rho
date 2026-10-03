.DEFAULT_GOAL := help

CARGO ?= cargo
CPUS ?= $(shell getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
export CARGO_BUILD_JOBS ?= $(CPUS)
export RUST_TEST_THREADS ?= $(CPUS)

.PHONY: help
help: ## Display this help screen
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}'

.PHONY: all
all: fmt-check clippy complexity quality crap ## Run all checks (format check, clippy, complexity, quality regressions, CRAP/tests)

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

.PHONY: coverage
coverage: ## Generate LCOV test coverage trace (uses nextest when available)
	@ulimit -n 10240 2>/dev/null || ulimit -n 4096 2>/dev/null || true; \
	if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) llvm-cov nextest --workspace --no-report && $(CARGO) llvm-cov report --workspace --lcov --output-path target/lcov.info; \
	else \
		$(CARGO) llvm-cov --workspace --lcov --output-path target/lcov.info; \
	fi

.PHONY: crap
crap: coverage ## Evaluate CRAP metrics and gate on functions exceeding threshold 30 (runs tests via coverage)
	@$(CARGO) crap --path . --lcov target/lcov.info --threshold 30 --format json 2>/dev/null | jq -e '([.entries[] | select(.crap > 30) | {file, line, function, crap: (.crap * 10 | round / 10), cc: .cyclomatic, cov: (.coverage * 10 | round / 10)}]) as $$v | if ($$v | length) > 0 then ($$v | halt_error(1)) else $$v end'

.PHONY: complexity
complexity: ## Evaluate Cognitive and Cyclomatic complexity with cccc and gate on cognitive <= 15
	@command -v cccc >/dev/null 2>&1 || { echo "Error: cccc not found. Install with: cargo install cccc-cli"; exit 1; }
	@cccc --max-cognitive 15 . | jq -e '([.files[]? | .path as $$path | .functions[]? | select(.cognitive > 15) | {file: $$path, line: .line, function: .name, cognitive: .cognitive, cyclomatic: .cyclomatic}]) as $$v | if ($$v | length) > 0 then ($$v | halt_error(1)) else $$v end'

.PHONY: quality
quality: ## Measure quality regressions against git HEAD with ripwire
	@command -v ripwire >/dev/null 2>&1 || { echo "Error: ripwire not found"; exit 1; }
	@ripwire . --quality-delta

