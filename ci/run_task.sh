#!/usr/bin/env bash
#
# Script used to run CI jobs, can also be used from the command line.
#
# Shellcheck can't search dynamic paths
# shellcheck source=/dev/null

set -euo pipefail

REPO_DIR=$(git rev-parse --show-toplevel)

# Make cargo invocations verbose unless in quiet mode.
# Also control bash debug output based on log level.
case "${MAINTAINER_TOOLS_LOG_LEVEL:-verbose}" in
    quiet)
        export CARGO_TERM_VERBOSE=false
        export CARGO_TERM_QUIET=true
        ;;
    *)
        export CARGO_TERM_VERBOSE=true
        export CARGO_TERM_QUIET=false
        set -x
        ;;
esac

# Use the current `Cargo.lock` file without updating it.
cargo="cargo --locked"

usage() {
    cat <<EOF
Usage:

    ./run_task.sh TASK

TASK
  - stable          Run tests with stable toolchain.
  - nightly         Run tests with nightly toolchain.
  - msrv            Run tests with MSRV toolchain.
  - lint            Run the linter (clippy).
  - docs            Build docs with stable toolchain.
  - docsrs          Build docs with nightly toolchain.
  - bench           Run the bench tests.

Environment Variables:
  MAINTAINER_TOOLS_LOG_LEVEL    Control script and cargo output verbosity.
    verbose (default)           Show all script and cargo messages.
    quiet                       Suppress script messages, reduce cargo output.
EOF
}

main() {
    local task="${1:-usage}"
    local crates_script="$REPO_DIR/contrib/crates.sh"

    # FIXME: This is a hackish way to get the help flag.
    if [ "$task" = "usage" ] || [ "$task" = "-h" ] || [ "$task" = "--help" ]; then
        usage
        exit 0
    fi

    check_required_commands

    # No need for env output when run from the terminal.
    if [ -n "${GITHUB_REPOSITORY+x}" ]; then
        cargo --version
        rustc --version
        /usr/bin/env bash --version
        locale
        env
    fi

    verbose_say "Repository: $REPO_DIR"
    verbose_say "Script invocation: $0 $task"

    if [ -e "$crates_script" ]; then
        verbose_say "Sourcing $crates_script"
        # can't find the file because of the ENV var
        # shellcheck source=/dev/null
        . "$crates_script"
        for crate in $CRATES; do
            verbose_say "Found crate: $crate"
        done
    else
        err "Missing file $crates_script"
    fi

    case $task in
    stable)
        # Test, run examples, do feature matrix.
        # crate/contrib/test_vars.sh is sourced in this function.
        build_and_test
        ;;

    nightly)
        build_and_test
        ;;

    msrv)
        build_and_test
        ;;

    lint)
        do_lint_workspace
        do_lint_crates
        do_dup_deps
        ;;

    docs)
        build_docs_with_stable_toolchain
        ;;

    docsrs)
        build_docs_with_nightly_toolchain
        ;;

    bench)
        do_bench
        ;;

    *)
        err "Error: unknown task $task"
        ;;
    esac
}

# Build and test for each crate, done with each toolchain.
build_and_test() {
    for crate in $CRATES; do
        local test_vars_script="$REPO_DIR/$crate/contrib/test_vars.sh"

        # Clean variables and also make sure they are defined.
        FEATURES_WITH_STD=""
        FEATURES_WITH_NO_STD=""
        FEATURES_WITHOUT_STD=""
        EXAMPLES=""

        verbose_say "Sourcing $test_vars_script"
        if [ -e "$test_vars_script" ]; then
            # Set crate specific variables.
            # can't find the file because of the ENV var
            # shellcheck source=/dev/null
            . "$test_vars_script"

            verbose_say "Got test vars:"
            verbose_say "FEATURES_WITH_STD: ${FEATURES_WITH_STD}"
            verbose_say "FEATURES_WITH_NO_STD: ${FEATURES_WITH_NO_STD}"
            verbose_say "FEATURES_WITHOUT_STD: ${FEATURES_WITHOUT_STD}"
            verbose_say "EXAMPLES: ${EXAMPLES:-}"
            if [[ -v EXACT_FEATURES && ${#EXACT_FEATURES[@]} -gt 0 ]]; then
                verbose_say "EXACT_FEATURES: ${EXACT_FEATURES[*]}"
            fi
        fi
        pushd "$REPO_DIR/$crate" > /dev/null

        do_test
        do_feature_matrix

        popd > /dev/null
    done
}

do_test() {
    # Defaults / sanity checks
    $cargo build
    $cargo test

    if [ -n "${EXAMPLES}" ]; then
        for example in $EXAMPLES; do # EXAMPLES is set in contrib/test_vars.sh
            name="$(echo "$example" | cut -d ':' -f 1)"
            features="$(echo "$example" | cut -d ':' -f 2)"
            $cargo run --example "$name" --features="$features"
        done
    fi

    if [ -e ./contrib/extra_tests.sh ];
    then
        . ./contrib/extra_tests.sh
    fi
}

# Each crate defines its own feature matrix test so feature combinations
# can be better controlled.
do_feature_matrix() {
    # For crates that have unusual feature requirements (e.g. `corepc`).
    if [[ -v EXACT_FEATURES && ${#EXACT_FEATURES[@]} -gt 0 ]]; then
        for features in "${EXACT_FEATURES[@]}"; do
            $cargo build --no-default-features --features="$features"
            $cargo test --no-default-features --features="$features"
        done
    # rust-miniscript only: https://github.com/rust-bitcoin/rust-miniscript/issues/681
    elif [ -n "${FEATURES_WITH_NO_STD}" ]; then
        $cargo build --no-default-features --features="no-std"
        $cargo test --no-default-features --features="no-std"

        loop_features "no-std" "${FEATURES_WITH_NO_STD}"
    else
        $cargo build --no-default-features
        $cargo test --no-default-features
    fi

    $cargo build --all-features
    $cargo test --all-features

    if [ -n "${FEATURES_WITH_STD}" ]; then
        loop_features "std" "${FEATURES_WITH_STD}"
    fi

    if [ -n "${FEATURES_WITHOUT_STD}" ]; then
        loop_features "" "$FEATURES_WITHOUT_STD"
    fi
}

# Build with each feature as well as all combinations of two features.
#
# Usage: loop_features "std" "this-feature that-feature other"
loop_features() {
    local use="${1:-}"          # Allow empty string.
    local features="$2"         # But require features.

    # All the provided features including $use
    $cargo build --no-default-features --features="$use $features"
    $cargo test --no-default-features --features="$use $features"

    read -r -a array <<< "$features"
    local len="${#array[@]}"

    if (( len > 1 )); then
        for ((i = 0 ; i < len ; i++ ));
        do
            $cargo build --no-default-features --features="$use ${array[i]}"
            $cargo test --no-default-features --features="$use ${array[i]}"

            if (( i < len - 1 )); then
               for ((j = i + 1 ; j < len ; j++ ));
               do
                   $cargo build --no-default-features --features="$use ${array[i]} ${array[j]}"
                   $cargo test --no-default-features --features="$use ${array[i]} ${array[j]}"
               done
            fi
        done
    fi
}

# Lint the workspace.
do_lint_workspace() {
    need_nightly
    $cargo clippy --workspace --all-targets --all-features --keep-going -- -D warnings
    $cargo clippy --workspace --all-targets --keep-going -- -D warnings
}

# Run extra crate specific lints, e.g. clippy with no-default-features.
do_lint_crates() {
    need_nightly
    for crate in $CRATES; do
        pushd "$REPO_DIR/$crate" > /dev/null
        if [ -e ./contrib/extra_lints.sh ]; then
            . ./contrib/extra_lints.sh
        fi
        popd > /dev/null
    done
}

# We should not have any duplicate dependencies. This catches mistakes made upgrading dependencies
# in one crate and not in another (e.g. upgrade bitcoin_hashes in bitcoin but not in secp).
do_dup_deps() {
    # We can't use pipefail because these grep statements fail by design when there is no duplicate,
    # the shell therefore won't pick up mistakes in your pipe - you are on your own.
    set +o pipefail

    # Contains dependencies that are expected to be duplicates.
    local duplicate_deps_script="$REPO_DIR/contrib/whitelist_deps.sh"

    # Only show the actual duplicated deps, not their reverse tree, then
    # whitelist the 'syn' crate which is duplicated but it's not our fault.
    local tree_cmd="cargo tree  --target=all --all-features --duplicates \
            | grep '^[0-9A-Za-z]' \
            | grep -v 'syn'"

    # Add any duplicate dependencies to ignore.
    if [ -e "$duplicate_deps_script" ]; then
        verbose_say "Sourcing $duplicate_deps_script"
        # can't find the file because of the ENV var
        # shellcheck source=/dev/null
        . "$duplicate_deps_script"

        if [ -n "${DUPLICATE_DEPS+x}" ]; then
            for dep in "${DUPLICATE_DEPS[@]}"; do
                tree_cmd+=" | grep -v $dep"
            done
        else
            err "parsed $duplicate_deps_script but failed to find DUPLICATE_DEPS array"
        fi
    fi

    tree_cmd+="| wc -l"

    duplicate_dependencies=$(eval "$tree_cmd")

    if [ "$duplicate_dependencies" -ne 0 ]; then
        cargo tree  --target=all --all-features --duplicates
        err "Dependency tree is broken, contains duplicates"
    fi

    set -o pipefail
}

# Build the docs with a nightly toolchain, in unison with the function
# below this checks that we feature guarded docs imports correctly.
build_docs_with_nightly_toolchain() {
    need_nightly
    # -j1 is because docs build fails if multiple versions of `bitcoin_hashes` are present in dep tree.
    RUSTDOCFLAGS="--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links" $cargo doc --all-features -j1
}

# Build the docs with a stable toolchain, in unison with the function
# above this checks that we feature guarded docs imports correctly.
build_docs_with_stable_toolchain() {
    local cargo="cargo +stable --locked" # Can't use global because of `+stable`.
    RUSTDOCFLAGS="-D warnings" $cargo doc --all-features -j1
}

# Bench only works with a non-stable toolchain (nightly, beta).
do_bench() {
    verbose_say "Running bench tests for: $CRATES"

    for crate in $CRATES; do
        pushd "$REPO_DIR/$crate" > /dev/null
        # Unit tests are ignored so if there are no bench test then this will just succeed.
        RUSTFLAGS='--cfg=bench' cargo bench
        popd > /dev/null
    done
}

# Check all the commands we use are present in the current environment.
check_required_commands() {
    need_cmd cargo
    need_cmd rustc
    need_cmd jq
    need_cmd cut
    need_cmd grep
    need_cmd wc
}

say() {
    echo "run_task: $1"
}

verbose_say() {
    case "${MAINTAINER_TOOLS_LOG_LEVEL:-verbose}" in
        quiet)
            # Suppress verbose output.
            ;;
        *)
            say "$1"
            ;;
    esac
}

err() {
    echo "$1" >&2
    exit 1
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1
    then err "need '$1' (command not found)"
    fi
}

need_nightly() {
    cargo_ver=$(cargo --version)
    if echo "$cargo_ver" | grep -q -v nightly; then
        err "Need a nightly compiler; have $(cargo --version)"
    fi
}

#
# Main script
#
main "$@"
exit 0

