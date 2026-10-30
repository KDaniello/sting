.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python
NIGHTLY ?= +nightly
WORKSPACE_FLAGS := --workspace --all-features

##@ Build

.PHONY: build
build: ## Build the release binary
	$(CARGO) build --release --bin karman

.PHONY: build-debug
build-debug: ## Build the debug binary
	$(CARGO) build --bin karman

##@ Test

.PHONY: test
test: ## Unit, property, conformance and doc tests
	$(CARGO) test $(WORKSPACE_FLAGS)

.PHONY: test-conformance
test-conformance: ## The conformance suite every mechanism must pass (docs/VERIFICATION.md)
	$(CARGO) test $(WORKSPACE_FLAGS) conformance

.PHONY: test-doc
test-doc: ## Documentation tests
	$(CARGO) test --doc $(WORKSPACE_FLAGS)

##@ Quality

.PHONY: fmt
fmt: ## Format with nightly rustfmt
	$(CARGO) $(NIGHTLY) fmt --all

.PHONY: fmt-check
fmt-check:
	$(CARGO) $(NIGHTLY) fmt --all --check

.PHONY: clippy
clippy:
	$(CARGO) clippy $(WORKSPACE_FLAGS) --lib --bins --examples --tests --benches -- -D warnings

.PHONY: deny
deny:
	$(CARGO) deny check

.PHONY: typos
typos:
	typos

.PHONY: doc-check
doc-check:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc $(WORKSPACE_FLAGS) --no-deps

.PHONY: lint
lint: fmt-check clippy deny typos doc-check ## Everything CI checks for style and architecture

.PHONY: fix
fix: ## Apply clippy suggestions, then format
	$(CARGO) clippy $(WORKSPACE_FLAGS) --fix --allow-dirty --allow-staged
	$(MAKE) fmt

##@ Profiling

.PHONY: profile
profile: build ## Profile every committed scenario (docs/PROFILER.md)
	@for scenario in testdata/scenarios/*.json; do \
	  ./target/release/karman profile --scenario $$scenario --runs 2000 || exit 1; \
	done

##@ Measurement

# The addresses are facts of the market being measured, not settings: the host that serves a
# platform's collection descriptions is what decides which collections a census is about, and the
# minter is the address whose holdings are stock rather than ownership. TON DNS is named outright
# because it is served from elsewhere.
GIFT_HOST ?= nft.fragment.com
TON_DNS   ?= 0:B774D95EB20543F186C06B371AB88AD704F7E256130CAF96189368A7D0CB6CCF
GIFT_MINTER ?= 0:158136239ADB15DD59DF90C641F9EFD312CFEB8664F218F4C3E5FCE9D95E6C07
CENSUS_DAYS ?= 90

.PHONY: census
census: ## Sweep TON into a census fixture (docs/DEAD-CAPITAL-SPEC.md). Keys in .env make it faster
	@mkdir -p out
	@set -a; [ -f .env ] && . ./.env; set +a; bash crates/karman-data/scripts/pull-ton-census.sh --into out/census --days $(CENSUS_DAYS) --served-by $(GIFT_HOST) --also $(TON_DNS) --issuer $(GIFT_MINTER)
	$(CARGO) run --release --quiet -p karman-data --bin pull-ton-census -- testdata/fixtures/ton-census.json out/census

##@ SDK

.PHONY: sdk
sdk: build ## Build the Python wheel, install it, and prove it answers like the terminal
	cd sdk/python && $(PYTHON) -m maturin build --release
	$(PYTHON) -m pip install --force-reinstall --quiet target/wheels/karman-*.whl
	$(PYTHON) -m pytest sdk/python/tests -q

##@ Reference

.PHONY: level0
level0: ## Regenerate the level-0 corpus from the reference bytecode (docs/VERIFICATION.md 1)
	$(CARGO) run --quiet -p karman-reference --bin karman-level0 > testdata/level0/morpho-blue.json

.PHONY: reference
reference: sdk ## Check the engine against the cadCAD reference (python/README.md)
	$(PYTHON) -m pip install --quiet cadCAD
	cd python/reference && $(PYTHON) compare.py --runs 1000 --ticks 500 --seed 42

##@ Documentation

.PHONY: docs
docs: ## Build and open the rustdoc
	$(CARGO) doc $(WORKSPACE_FLAGS) --no-deps --open

##@ Release

.PHONY: pr
pr: lint test test-doc ## Run before opening a pull request
	@echo "ready"

.PHONY: clean
clean:
	$(CARGO) clean

##@ Help

.PHONY: help
help:
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} \
	  /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } \
	  /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)
