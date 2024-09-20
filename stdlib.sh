# rust-bitcoin stdlib for Bash (of sorts).
#
# Much of the code here was originally stolen from the old `rustup.sh` script
# https://github.com/rust-lang-deprecated/rustup.sh/blob/master/rustup.sh
#
# If you have never read the comments at the top of that file, consider it, they are gold.
#
# No shebang, this file should not be executed.
# shellcheck disable=SC2148
#
# Disable because `flag_verbose` is referenced but not assigned, however we check it is non-zero.
# shellcheck disable=SC2154

set -u

err() {
    echo "$1" >&2
    exit 1
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1
    then err "need '$1' (command not found)"
    fi
}

need_ok() {
    if [ $? != 0 ]; then err "$1"; fi
}

assert_nz() {
    if [ -z "$1" ]; then err "assert_nz $2"; fi
}

# Run a command that should never fail. If the command fails execution
# will immediately terminate with an error showing the failing
# command.
ensure() {
    "$@"
    need_ok "command failed: $*"
}

# This is just for indicating that commands' results are being
# intentionally ignored. Usually, because it's being executed
# as part of error handling.
ignore() {
    run "$@"
}

# Assert that we have a nightly Rust toolchain installed.
need_nightly() {
    cargo_ver=$(cargo --version)
    if echo "$cargo_ver" | grep -q -v nightly; then
        err "Need a nightly compiler; have $(cargo --version) (use RUSTUP_TOOLCHAIN=+nightly cmd)"
    fi
}
