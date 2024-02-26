# Makefile provides an API for CI related tasks
# Using the makefile is not required however CI
# uses the specific targets within the file.
# Therefore may be useful in ensuring a change
# is ready to pass CI checks.

RUSTFLAGS = --cfg tokio_unstable
CARGO = RUSTFLAGS='${RUSTFLAGS}' cargo

RELEASE_LEVEL ?= minor

# ECS environment to deploy image to
DEPLOY_ENV ?= dev

# Deploy target to use for CD manager job
DEPLOY_TARGET ?= latest

# Docker image tag to deploy
DEPLOY_TAG ?= latest

# Whether or not this is a manual deployment
MANUAL_DEPLOY ?= false

DATABASE_URL ?= sqlite://checkpointer.db

.PHONY: all
all: build check-fmt check-clippy test

.PHONY: build
build:
	# Build with default features
	$(CARGO) build --locked --release
	# Build with all features
	$(CARGO) build --locked --release --all-features

.PHONY: fluence-build
fluence-build:
	# Build with default features
	fluence build

.PHONY: release
release: RUSTFLAGS += -D warnings
release:
	$(CARGO) build -p checkpointer --locked --release

# Prepare a release PR.
.PHONY: release-pr
release-pr:
	./ci-scripts/release_pr.sh ${RELEASE_LEVEL}

.PHONY: debug
debug:
	$(CARGO) build -p checkpointer --locked

.PHONY: test
test:
	# Setup scaffolding
	./ci-scripts/setup_test_env.sh
	# Test with default features
	DATABASE_URL=$(DATABASE_URL) $(CARGO) test -p checkpointer --locked --release
	# Test with all features
	DATABASE_URL=$(DATABASE_URL) $(CARGO) test -p checkpointer --locked --release --all-features
	./ci-scripts/teardown_test_env.sh

.PHONY: test-event-joiner
test-event-joiner:
	# Setup scaffolding
	IT_TEST_CHECKPOINTER=1 ./ci-scripts/setup_test_env.sh
	# Test with default features
	$(CARGO) test -p event-joiner --locked --release
	# Test with all features
	$(CARGO) test -p event-joiner --locked --release --all-features
	IT_TEST_CHECKPOINTER=1 ./ci-scripts/teardown_test_env.sh

.PHONY: check-fmt
check-fmt:
	$(CARGO) fmt --all -- --check

.PHONY: check-clippy
check-clippy:
	# Check with default features
	$(CARGO) clippy --workspace --locked --release -- -D warnings --no-deps
	# Check with all features
	$(CARGO) clippy --workspace --locked --release --all-features -- -D warnings --no-deps

.PHONY: run
run:
	RUST_LOG=WARN,checkpointer=DEBUG $(CARGO) run --all-features --locked --release --bin checkpointer

.PHONY: publish-docker
publish-docker:
	./ci-scripts/publish.sh

.PHONY: schedule-ecs-deployment
schedule-ecs-deployment:
	./ci-scripts/schedule_ecs_deploy.sh "${DEPLOY_ENV}" "${DEPLOY_TARGET}" "${DEPLOY_TAG}" "${MANUAL_DEPLOY}"

.PHONY: schedule-k8s-deployment
schedule-k8s-deployment:
	./ci-scripts/schedule_k8s_deploy.sh "${DEPLOY_ENV}" "${DEPLOY_TAG}"
