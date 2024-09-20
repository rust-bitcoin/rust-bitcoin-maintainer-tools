#!/usr/bin/env bash
#
# Create and manipulate the lock files.
#
# We use lock files `Cargo-recent.lock` and `Cargo-minimal.lock` to pin
# dependencies and check specific versions in CI.
#
# Shellcheck can't search dynamic paths
# shellcheck source=/dev/null

set -u

main() {
    set_globals
    assert_cmds
    handle_command_line_args "$@"
}

set_globals() {
    # The directory of this script.
    script_dir=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

    # Used below by `say`.
    script=${0##*/}

    # Used below by `verbose_say`.
    flag_verbose=false

    # Sourcing this file requires `flag_verbose` to be set.
    . "$script_dir/stdlib.sh"

    # Environment sanity checks
    assert_nz "$HOME" "\$HOME is undefined"
    assert_nz "$0" "\$0 is undefined"

    # Make all cargo invocations verbose.
    export CARGO_TERM_VERBOSE=true

    recent="Cargo-recent.lock"
    minimal="Cargo-minimal.lock"

    msrv="1.63.0"
}

handle_command_line_args() {
    local _no_args=false
    local _help=false
    local _create=false
    local _msrv=""
    local _update=false

    if [ $# -eq 0 ]; then
        _no_args=true
    fi

    local _arg
    for _arg in "$@"; do
        case "${_arg%%=*}" in
            create )
                _create=true
                ;;

            update )
                _update=true
                ;;

            --msrv )
                if is_value_arg "$_arg" "--msrv"; then
                    _msrv="$(get_value_arg "$_arg")"
                else
                    say_err "the --msrv option requires a toolchain version argument"
                    print_help
                    exit 1
                fi

                ;;

            -h | --help )
                _help=true
                ;;

            --verbose)
                # verbose is a global flag
                flag_verbose=true
                ;;

            *)
                echo "Unknown argument '$_arg', displaying usage:"
                echo "${_arg%%=*}"
                _help=true
                ;;

        esac

    done

    if [ "$_create" = true ]; then
        if [ -z "$_msrv" ]; then
            msrv="$_msrv"
        fi

        verbose_say "Creating lock files, MSRV: $msrv"
        create
    fi

    if [ "$_update" = true ]; then
        update
        verbose_say "Your git index will now be dirty if lock file update is required"
    fi

    if [ "$_help" = true ]; then
        print_help
        exit 0
    fi

    if [ "$_no_args" = true ]; then
        verbose_say "no option supplied, defaulting to update"
        update
    fi
}

is_value_arg() {
    local _arg="$1"
    local _name="$2"

    echo "$_arg" | grep -q -- "$_name="
    return $?
}

get_value_arg() {
    local _arg="$1"

    echo "$_arg" | cut -f2 -d=
}

# Creates the minimal and recent lock files.
#
# If this function fails you may want to be lazy and just duplicate `Cargo.lock`
# as the minimal and recent lock files.
create() {
    # Attempt to create a minimal lock file.
    rm --force Cargo.lock > /dev/null
    cargo +nightly check --all-features -Z minimal-versions
    need_ok "failed to build with -Z minimial-versions, you might have to use a recent lock file for minimal"

    # We only want to create the minimal lock file if we can build with current MSRV.
    cargo "+$msrv" --locked check --all-features
    need_ok "failed to build with minimal lock file and MSRV $_msrv"
    cp Cargo.lock "$minimal"
    
    # If that worked we can create a recent lock file.
    cargo update
    cp Cargo.lock "$recent"
}

# Updates the minimal and recent lock files.
update() {
    for file in "$minimal" "$recent"; do
        cp --force "$file" Cargo.lock
        cargo check
        cp --force Cargo.lock "$file"
    done
}

assert_cmds() {
    need_cmd cat
    need_cmd cp
    need_cmd cargo
}

say() {
    echo "$script: $1"
}

say_err() {
    say "$1" >&2
}

verbose_say() {
    if [ "$flag_verbose" = true ]; then
	say "$1"
    fi
}

print_help() {
    cat <<EOF
Usage: $script [OPITON] [COMMAND]

COMMAND:

        create                      Create new minimal and recent lock files.
        update                      Update the minimal and recent lock files (default).

OPTION:

        --msrv=version              The Rust toolchain version to use as MSRV. 
        --verbose                   Enable verbose output.
        --help, -h                  Display this help information.
EOF
}

# Main script
main "$@"
