#!/usr/bin/env bash
#
# Bash shell script template.
#
# Shellcheck can't search dynamic paths
# shellcheck source=/dev/null

# Note we don't use `set -x` because error handling is done manually.
# If you use pipes you may want to use `set -euxo pipefail` instead.
set -u

main() {
    set_globals
    assert_cmds
    handle_command_line_args "$@"
}

set_globals() {
    # The git repository where script is run from.
    # repo_dir=$(git rev-parse --show-toplevel)

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
}

handle_command_line_args() {
    local _help=false

    local _arg
    for _arg in "$@"; do
        case "${_arg%%=*}" in
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

    if [ "$_help" = true ]; then
        print_help
        exit 0
    fi
    
    verbose_say "Enabled verbose output"
}

assert_cmds() {
    need_cmd cat
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
Usage: $script [--verbose]

Options:

        --verbose,                        Enable verbose output
        --help, -h                        Display usage information
EOF
}

# Main script
main "$@"
