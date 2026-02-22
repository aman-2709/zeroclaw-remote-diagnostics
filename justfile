# ZeroClaw Remote Diagnostics — Build Recipes

default:
    @just --list

# Run all checks (format, lint, typecheck)
check:
    cargo fmt --all --check
    cargo clippy --workspace -- -D warnings
    cargo check --workspace

# Run all tests
test:
    cargo test --workspace

# Build all crates (debug)
build:
    cargo build --workspace

# Build edge agent (release, size-optimized)
build-edge:
    cargo build --release --profile release-edge -p zc-fleet-agent

# Build cloud API (release)
build-cloud:
    cargo build --release -p zc-cloud-api

# Format all Rust code
fmt:
    cargo fmt --all

# Run clippy with auto-fix
fix:
    cargo clippy --workspace --fix --allow-dirty

# Clean build artifacts
clean:
    cargo clean

# Run a specific crate's tests
test-crate crate:
    cargo test -p {{crate}}

# Check only protocol types
check-protocol:
    cargo check -p zc-protocol
    cargo test -p zc-protocol

# Validate Terraform
tf-validate:
    cd infra && terraform fmt -recursive -check && terraform validate

# Format Terraform files
tf-fmt:
    cd infra && terraform fmt -recursive

# Plan infrastructure changes
tf-plan:
    cd infra && terraform plan -var-file=terraform.tfvars

# Apply infrastructure changes
tf-apply:
    cd infra && terraform apply -var-file=terraform.tfvars

# Install frontend dependencies
frontend-install:
    cd frontend && pnpm install

# Run frontend dev server
frontend-dev:
    cd frontend && pnpm dev

# Build frontend for production
frontend-build:
    cd frontend && pnpm build
