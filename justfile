# This justfile is functional, but also showcases how to integrate cargo-rbmt.
set quiet := true

export RBMT_LOG_LEVEL := env("RBMT_LOG_LEVEL", "progress")

project := file_name(justfile_directory())
rbmt_version := `grep "^rbmt.version" Cargo.toml | cut -d'"' -f2`

_default:
  just --list

# Install workspace dev tools
tools:
  echo "{{project}} dev tools [cargo-rbmt@{{rbmt_version}}]"
  cargo install --quiet --path {{justfile_directory()}}/cargo-rbmt
  cargo rbmt toolchains
  cargo rbmt tools

# Run cargo-rbmt with given args
rbmt *args: tools
  cargo rbmt {{args}}

# Update minimal and maximum lockfiles
lock: (rbmt "lock --lockfiles recent,minimal,maximum")

# Check docs
docs: (rbmt "docs --lockfile maximum")

# Format worksapce
fmt: (rbmt "fmt")

# Check workspace lints
lint: (rbmt "lint --lockfile existing")

# Test workspace
test: (rbmt "test --lockfile minimal")

# Check prerelease
prerelease: (rbmt "prerelease --force")
