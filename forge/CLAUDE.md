# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust project named "forge" using Rust Edition 2024.

## Build and Development Commands

### Building
```bash
# Build the project
cargo build

# Build with optimizations (release mode)
cargo build --release
```

### Running
```bash
# Run the project
cargo run

# Run in release mode
cargo run --release
```

### Testing
```bash
# Run all tests
cargo test

# Run a specific test
cargo test <test_name>

# Run tests with output visible
cargo test -- --nocapture
```

### Code Quality
```bash
# Check code without building
cargo check

# Format code
cargo +nightly fmt

# Lint code
cargo clippy

# Lint with all warnings
cargo clippy -- -W clippy::all
```

## Project Structure

- `src/main.rs` - Main application entry point
- `Cargo.toml` - Package manifest and dependencies
- `target/` - Build artifacts (gitignored)

## Libraries

Use clap: https://docs.rs/clap/latest/clap/

## API docs

For API docs use: https://forgejo.org/docs/next/user/api-usage

## Configuration

The tool requires a `.forge.toml` file. It searches for the config file in the following locations (in order):
1. `./.forge.toml` (current directory)
2. `~/.config/forge.toml`
3. `~/.forge.toml`

Configuration format:
```toml
username = "your-username"
token = "your-api-token"

# Optional: HTTPS base URL for the Forgejo instance
# Default: "https://gitea.bitcoin.ninja"
# https_url = "https://gitea.bitcoin.ninja"

# Optional: SSH hostname for git operations
# Default: "gitea-ssh.bitcoin.ninja"
# ssh_url = "gitea-ssh.bitcoin.ninja"
```

See `.forge.toml.example` for a template configuration file.

The API URL is automatically constructed as `{https_url}/api/v1`.

## Implemented Commands

All commands support single-letter shortcuts:
- `pr` → `p`
- `checkout` → `c`
- `ack` → `a`
- `merge` → `m`
- `fetch` → `f`
- `push` → `push` (no shortcut)

Examples:
- `forge pr checkout 123` → `forge p c 123`
- `forge pr ack` → `forge p a`
- `forge pr merge 123` → `forge p m 123`
- `forge fetch` → `forge f`
- `forge push -c` (push current change)
- `forge push` (push all branches)

### `forge pr checkout <number>` (or `forge p c <number>`)
Checkout a pull request locally. Creates a local branch `pr-<number>`.

**Status**: ✅ Working

### `forge pr ack` (or `forge p a`)
Post an ACK comment on the currently checked out PR (detects PR number from branch name).

**Status**: ✅ Working

### `forge pr merge <number> [-r owner/repo] [-b branch]` (or `forge p m <number> ...`)
Merge a pull request locally with signing (similar to bitcoin-core/bitcoin-maintainer-tools/github-merge.py).

**Workflow**:
1. Fetches PR details from Forgejo API
2. Fetches ACKs from both `/issues/{pr}/comments` AND `/pulls/{pr}/reviews` endpoints
3. Creates merge commit with all ACKs included in commit message
4. Computes tree SHA512 hash
5. Drops into interactive shell for testing
6. Signs commit with GPG
7. Pushes to upstream/origin via HTTPS with token (avoids SSH deploy key issues)
8. Relies on Forgejo's "Autodetect manual merge" feature to automatically mark PR as merged

**Status**: ✅ Working
- Core merge workflow: ✅ Working (tested on PR 101, PR 103, PR 104)
- ACK detection from both comments and reviews: ✅ Working
- HTTPS push with token: ✅ Working
- **PR state management**: ✅ Working - Relies on Forgejo's "Autodetect manual merge" repository
  setting. After pushing, Forgejo automatically detects the merge and marks the PR as merged.
  This detection may take a few moments after the push completes.

### `forge fetch` (or `forge f`)
Fetches from all git remotes using HTTPS (with token) and runs `jj git fetch`.

**Purpose**: Avoids SSH deploy key authorization errors by temporarily configuring git URL
rewriting to convert `git@gitea-ssh.bitcoin.ninja:` and `git@gitea.bitcoin.ninja:` to
`https://{token}@gitea.bitcoin.ninja/` before running jj git fetch, then cleans up the config.

**Status**: ✅ Working

### `forge push [-c]`
Push using jj with HTTPS authentication (avoids SSH deploy key issues).

**Options**:
- Without `-c`: Runs `jj git push` (pushes all branches)
- With `-c`: Runs `jj git push -c @` (pushes only current change)

**Purpose**: Avoids SSH deploy key authorization errors by temporarily configuring git URL
rewriting to convert `git@gitea-ssh.bitcoin.ninja:` to `https://{token}@gitea.bitcoin.ninja/`
before running jj git push, then cleans up the config.

**Status**: ✅ Working

## Known Issues

1. **Deploy key limitations**: SSH authentication hits deploy key read-only restrictions.
   All commands use HTTPS with token authentication to work around this.

## Testing Notes

For API docs use: https://forgejo.org/docs/next/user/api-usage
Forgejo instance URL: https://gitea.bitcoin.ninja/rust-bitcoin/rust-psbt

Example API endpoints used:
- `GET /api/v1/repos/{repo}/pulls/{pr}` - Get PR details
- `GET /api/v1/repos/{repo}/issues/{pr}/comments` - Get PR comments (for ACKs)
- `GET /api/v1/repos/{repo}/pulls/{pr}/reviews` - Get PR reviews (for ACKs)
- `POST /api/v1/repos/{repo}/issues/{pr}/comments` - Post comment

## PR State Management - Debugging Notes (2026-04-11)

**Issue**: After running `forge pr merge`, PRs were being marked as "closed" instead of "merged".

**Root Cause**: The code was attempting to use `POST /api/v1/repos/{repo}/pulls/{pr}/merge` with
`{"Do":"manually-merged"}`, but this is **not a valid API call**.

**Key Findings**:
1. The `POST /api/v1/repos/{repo}/pulls/{pr}/merge` endpoint is for **performing merges via API**,
   not for marking PRs as manually merged.

2. Valid values for the `"Do"` field are:
   - `"merge"` - Standard merge commit
   - `"rebase"` - Rebase and merge (no merge commit)
   - `"rebase-merge"` - Rebase then merge with --no-ff
   - `"squash"` - Squash all commits into one
   - `"fast-forward-only"` - Fast-forward only if possible

3. **There is NO API endpoint to mark a PR as manually merged**. The "manually-merged" value does not exist.

**Solution**:
- Removed the invalid API call entirely (see `src/main.rs` lines ~1010-1018)
- Now relies on Forgejo's **"Autodetect manual merge"** repository setting
- When enabled, Forgejo automatically detects merged PRs by checking if commits exist in base branch
- Detection happens server-side, no API call needed
- May take a few moments after push for Forgejo to detect and update PR state

**How to Debug If Not Working**:
1. Verify "Autodetect manual merge" is enabled in repository settings:
   - Go to repo settings → Pull Requests → "Automatically detect manual merge"

2. Check if PR state updated in web UI after a few minutes (Forgejo runs detection periodically)

3. If PRs remain "closed" instead of "merged", check Forgejo server logs for autodetect errors

4. Test manually from command line:
   ```bash
   # Merge and push a PR
   forge pr merge <number>

   # Wait 1-2 minutes, then check PR state via API:
   curl -H "Authorization: token $(grep token ~/.config/forge.toml | cut -d'"' -f2)" \
        "https://gitea.bitcoin.ninja/api/v1/repos/{owner}/{repo}/pulls/{pr}"

   # Look for "merged": true in the JSON response
   ```

5. If autodetect is not working, there may be a Forgejo configuration issue. Check:
   - Forgejo version (autodetect was added in a specific version)
   - Server-side cron job configuration for PR tasks
   - Repository-specific settings overriding global defaults

**Related Code Locations**:
- PR merge workflow: `src/main.rs` function `merge_pr()` (lines ~712-1093)
- Push and state management: `src/main.rs` lines ~993-1018
- Previously had API calls here, now just prints informational message


