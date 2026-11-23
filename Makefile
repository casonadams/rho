.DEFAULT_GOAL := help

CARGO ?= cargo
CPUS ?= $(shell getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
export CARGO_BUILD_JOBS ?= $(CPUS)
export RUST_TEST_THREADS ?= $(CPUS)
COGNITIVE_MAX := $(shell sed -n 's/^max-cognitive *= *\([0-9]*\).*/\1/p' cccc.toml)

# Output is quiet by default (no per-crate compile lines, no PASS line per test) so `make all` stays
# readable and cheap in a coding agent's context; failures, slow tests and warnings still print.
# `make test V=1` (or any target) shows everything.
ifeq ($(V),1)
NEXTEST_LEVEL := --status-level pass
Q :=
else
export CARGO_TERM_QUIET := true
NEXTEST_LEVEL :=
Q := @
endif

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
	$(Q)$(CARGO) fmt --all -- --check

.PHONY: clippy
clippy: ## Run Clippy with warnings treated as errors
	$(Q)$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: clippy-fix
clippy-fix: ## Automatically fix Clippy suggestions where possible
	$(CARGO) clippy --workspace --all-targets --fix --allow-dirty --allow-staged

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean

.PHONY: test
test: ## Run the workspace tests (uses nextest when available); V=1 prints every test
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) nextest run --workspace --all-targets $(NEXTEST_LEVEL) && $(MAKE) --no-print-directory doctest; \
	else \
		$(CARGO) test --workspace; \
	fi

.PHONY: doctest
doctest: ## Run the documentation tests (nextest cannot); prints only on failure unless V=1
	$(Q)out=$$($(CARGO) test --workspace --doc 2>&1); status=$$?; \
	if [ $$status -ne 0 ] || [ "$(V)" = 1 ]; then echo "$$out"; fi; \
	exit $$status

.PHONY: leak-check
leak-check: ## Run the tests with a private TMPDIR and fail if they leave files behind
	@dir=$$(mktemp -d) || exit 1; \
	TMPDIR=$$dir $(MAKE) --no-print-directory test; status=$$?; \
	leaked=$$(find "$$dir" -mindepth 1 -maxdepth 1); \
	if [ -n "$$leaked" ]; then echo "Tests leaked into the temp dir ($$dir):"; echo "$$leaked"; exit 1; fi; \
	rm -rf "$$dir"; exit $$status

.PHONY: complexity
complexity: ## Gate on the cognitive complexity limit set in cccc.toml
	@command -v cccc >/dev/null 2>&1 || { echo "Error: cccc not found. Install with: cargo install cccc-cli"; exit 1; }
	@out=$$(cccc .) || { echo "$$out" | jq -c '.files[]? | .path as $$path | .functions[]? | select(.cognitive > $(COGNITIVE_MAX)) | {file: $$path, line, function: .name, cognitive, cyclomatic}'; exit 1; }
