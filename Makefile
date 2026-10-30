.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python
# Every feature, because a feature nothing runs is a feature nobody checks: the
# asynchronous adapter is off by default and would otherwise be compiled by no
# gate at all.
WORKSPACE_FLAGS := --workspace --all-features

##@ Test

.PHONY: test-fast
test-fast: ## Library tests only — the inner loop
	$(CARGO) nextest run $(WORKSPACE_FLAGS) --lib

.PHONY: test
test: ## Everything this host can run offline
	$(CARGO) nextest run $(WORKSPACE_FLAGS)

# nextest does not run doctests, so they stay on cargo's own runner rather than going unrun.
.PHONY: test-doc
test-doc: ## Documentation tests
	$(CARGO) test --doc $(WORKSPACE_FLAGS)

.PHONY: test-live
test-live: ## The tests that need the network, a container or a key from .env
	$(CARGO) nextest run $(WORKSPACE_FLAGS) --run-ignored all
	$(PYTHON) scripts/run-capabilities.py --live
	$(MAKE) snapshot-check

.PHONY: snapshot-check
snapshot-check: ## One fixed mainnet range, replayed and held against what is committed
	$(CARGO) run -q -p sting-sybil-ingestion -- snapshot --from 87210000 --to 87210003 --live 		--verify corpus/sybil/snapshots/mc-87210000-87210003.json

##@ Quality

.PHONY: fmt
fmt: ## Format
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check:
	$(CARGO) fmt --all --check

.PHONY: clippy
clippy:
	$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets -- -D warnings

.PHONY: deny
deny:
	$(CARGO) deny check --hide-inclusion-graph

.PHONY: typos
typos:
	typos

.PHONY: doc-check
doc-check:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc $(WORKSPACE_FLAGS) --no-deps

.PHONY: docs-check
docs-check: ## The mechanical documentation rules
	$(PYTHON) scripts/check-docs.py

.PHONY: lints-check
lints-check:
	$(PYTHON) scripts/check-lints.py

.PHONY: numbers
numbers: ## Re-measure the facts the documentation is allowed to state
	$(PYTHON) scripts/doc-numbers.py --update

.PHONY: capabilities
capabilities: ## Every offline capability in the inventory, run as the document states it
	$(PYTHON) scripts/run-capabilities.py

# The costly ones and the ones whose input another capability produces. Minutes, deliberately kept
# out of the ordinary gate: a gate that takes minutes is a gate people stop running.
.PHONY: capabilities-full
capabilities-full: ## The same, including the capabilities that cost minutes
	$(PYTHON) scripts/run-capabilities.py --full

.PHONY: capabilities-check
capabilities-check:
	$(PYTHON) scripts/check-capabilities.py

.PHONY: numbers-check
numbers-check:
	$(PYTHON) scripts/doc-numbers.py

.PHONY: lint
lint: fmt-check clippy deny typos doc-check docs-check lints-check capabilities-check numbers-check ## Every mechanical rule

# What a hook can afford. The full gate builds every target, which is minutes; a hook that costs
# more than the change it guards is a hook that gets bypassed.
.PHONY: pre-commit
pre-commit: fmt-check docs-check lints-check numbers-check ## The subset a git hook can run without being turned off

.PHONY: fix
fix: ## Apply clippy suggestions, then format
	$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets --fix --allow-dirty --allow-staged
	$(MAKE) fmt

##@ Documentation

.PHONY: docs
docs: ## Build and open the rustdoc
	$(CARGO) doc $(WORKSPACE_FLAGS) --no-deps --open

##@ Release

.PHONY: pr
pr: lint test test-doc capabilities ## The full gate — green before a commit closes a milestone
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
