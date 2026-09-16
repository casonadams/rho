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
	$(CARGO) test --workspace --all-targets

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

.PHONY: wasm
wasm: ## Build rho-wasm and generate JS bindings into www/hub/wasm
	@if [ -z "$$CC_wasm32_unknown_unknown" ] && [ "$$(uname -s)" = "Darwin" ]; then \
		LLVM_DIR=$$(if [ -d "/opt/homebrew/opt/llvm/bin" ]; then echo "/opt/homebrew/opt/llvm/bin"; \
			elif [ -d "/usr/local/opt/llvm/bin" ]; then echo "/usr/local/opt/llvm/bin"; \
			elif command -v brew >/dev/null 2>&1 && [ -d "$$(brew --prefix llvm 2>/dev/null)/bin" ]; then echo "$$(brew --prefix llvm)/bin"; fi); \
		if [ -n "$$LLVM_DIR" ]; then \
			CC_wasm32_unknown_unknown="$$LLVM_DIR/clang" \
			AR_wasm32_unknown_unknown="$$LLVM_DIR/llvm-ar" \
			$(CARGO) build -p rho-wasm --target wasm32-unknown-unknown --release; \
		else \
			echo "Error: Apple Clang lacks wasm32 support. Please install LLVM via 'brew install llvm'"; \
			exit 1; \
		fi; \
	else \
		$(CARGO) build -p rho-wasm --target wasm32-unknown-unknown --release; \
	fi
	@if ! command -v wasm-bindgen >/dev/null 2>&1; then \
		echo "Error: wasm-bindgen CLI not found. Please install via 'cargo install -f wasm-bindgen-cli --version 0.2.106'"; \
		exit 1; \
	fi
	wasm-bindgen target/wasm32-unknown-unknown/release/rho_wasm.wasm --out-dir www/hub/wasm --target web
	@if command -v wasm-opt >/dev/null 2>&1; then \
		wasm-opt -Oz -o www/hub/wasm/rho_wasm_bg.wasm www/hub/wasm/rho_wasm_bg.wasm; \
	fi

.PHONY: clean
clean: ## Clean cargo build artifacts
	$(CARGO) clean
