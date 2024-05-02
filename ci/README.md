# Continuous Integration

This directory contains tools used by crates in the `rust-bitcoin` org to implement Continuous
Integration. Currently this is just a script `run_task.sh` that can be called from a GitHub workflow
job to run a specific task.

TL;DR `./run_task.sh --help`

#### Table Of Contents

- [Usage](#usage)
- [Lock file](#lock-file)
- [Crates](#crates)
  * [Per crate environment variables](#per-crate-environment-variables)
  * [Additional crate specific tests](#additional-crate-specific-tests)
- [Fuzzing](#fuzzing)
- [Example workflows](#example-workflows)
  * [A job using a stable toolchain](#a-job-using-a-stable-toolchain)
  * [A job using a specific nightly toolchain](#a-job-using-a-specific-nightly-toolchain)

## Usage

The `run_task.sh` script expects a few things to be present when it runs:

In the repository root:

- A lock file: `Cargo.lock`
- A script that defines the crates: `contrib/crates.sh`

And for each crate there should exist a directory `REPO_DIR/CRATE/contrib/` containing:

- `test_vars.sh`: Defines environment variables
- Optional: `extra_tests.sh`: Additional test script.

If the repository is not a workspace then per crate files go directly in `REPO_ROOT/contrib/`.

(See [Crates`](#crates) below.)

## Lock file

Repositories MUST contain a `Cargo.lock` file before running `run_task.sh`. `cargo` is typically
called with `--locked`. If you don't care about dependency versions just run `cargo update` in your
CI job (to create a lock file) before calling `run_task.sh`.

If you do care about versions consider adding:

- `Cargo-recent.lock`: A manifest with some recent versions numbers that pass CI.
- `Cargo-minimal.lock`: A manifest with some minimal version numbers that pass CI.

Then you can use, for example:

```yaml
    strategy:
      matrix:
        dep: [minimal, recent]
    steps:

    <!-- other stuff elided -->

      - name: "Copy lock file"
        run: cp Cargo-${{ matrix.dep }}.lock Cargo.lock

```

(Tip: Create minimal lock file with`cargo +nightly build -- -Z minimal-versions`.)

## Crates

All repositories MUST include a `REPO_DIR/contrib/crates.sh` script:

```bash
#!/usr/bin/env bash

# Crates in this workspace to test (note "fuzz" is only built not tested).
CRATES=("base58" "bitcoin" "fuzz" "hashes" "internals" "io" "units")
```

`CRATES` MUST be an array. If repository is not a workspace use `CRATES=(".")`).

### Per crate environment variables

All crates MUST include a file `REPO_DIR/CRATE/contrib/test_vars.sh`

```bash
#!/usr/bin/env bash

# Test all these features with "std" enabled.
#
# Ignore this if crate does not have "std" feature.
FEATURES_WITH_STD=""

# Test all these features without "std" enabled.
#
# Use this even if crate does not have "std" feature.
FEATURES_WITHOUT_STD=""

# Run these examples.
EXAMPLES=""
```

#### The `EXAMPLES` variable

```bash
EXAPMLES="example:feature"
```

```bash
EXAPMLES="example:feature1,feature2"
```

```bash
EXAPMLES="example_a:feature1,feature2 example_b:feature1"
```


Tip: if your example does not require any features consider using "default".

```bash
EXAPMLES="example_a:default"
```

### Additional crate specific tests

Additional tests can be put in an optional `contrib/extra_tests.sh` script. This script will be run
as part of the `stable`, `nightly`, and `msrv` jobs after running unit tests.

### Duplicate dependencies

If any dependency should be ignored from the duplicate dependencies test (done when linting) specify
them in a bash array in `REPO_DIR/contrib/whitelist_deps.sh` as such:

Note, this is usually a temporary measure during upgrade.

```bash
#!/usr/bin/env bash

DUPLICATE_DEPS=("bech32")
```

## Fuzzing

Fuzz tests are expected to be in a crate called `REPO_DIR/fuzz/`. The `run_task.sh` script just
builds the fuzz crate as a sanity check.

## Example workflows

### A job using a stable toolchain

To use the `run_task.sh` script you'll want to do something like this:

```yaml
jobs:
  Stable:                       # 2 jobs, one per manifest.
    name: Test - stable toolchain
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        dep: [minimal, recent]
    steps:
      - name: "Checkout repo"
        uses: actions/checkout@v4
      - name: "Checkout maintainer tools"
        uses: actions/checkout@v4
        with:
          repository: rust-bitcoin/rust-bitcoin-maintainer-tools
          path: maintainer-tools
      - name: "Select toolchain"
        uses: dtolnay/rust-toolchain@stable
      - name: "Copy lock file"
        run: cp Cargo-${{ matrix.dep }}.lock Cargo.lock
      - name: "Run test script"
        run: ./maintainer-tools/ci/run_task.sh stable
```

### A job using a specific nightly toolchain

Have a file in the repository root with the nightly toolchain version to use.

```bash
$ cat nightly_version
nightly-2024-04-30
```

And use a `Prepare` job to a set an environment variable using the file.

```yaml
jobs:
  Prepare:
    runs-on: ubuntu-latest
    outputs:
      nightly_version: ${{ steps.read_toolchain.outputs.nightly_version }}
    steps:
      - name: Checkout Crate
        uses: actions/checkout@v4
      - name: Read nightly version
        id: read_toolchain
        run: echo "nightly_version=$(cat nightly-version)" >> $GITHUB_OUTPUT

  Nightly:                      # 2 jobs, one per manifest.
    name: Test - nightly toolchain
    needs: Prepare
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        dep: [minimal, recent]
    steps:
      - name: "Checkout repo"
        uses: actions/checkout@v4
      - name: "Checkout maintainer tools"
        uses: actions/checkout@v4
        with:
          repository: tcharding/rust-bitcoin-maintainer-tools
          ref: 05-02-ci
          path: maintainer-tools
      - name: "Select toolchain"
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: ${{ needs.Prepare.outputs.nightly_version }}
      - name: "Copy lock file"
        run: cp Cargo-${{ matrix.dep }}.lock Cargo.lock
      - name: "Run test script"
        run: ./maintainer-tools/ci/run_task.sh nightly
```