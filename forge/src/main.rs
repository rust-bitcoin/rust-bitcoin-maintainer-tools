// SPDX-License-Identifier: MIT AND Apache-2.0

use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
struct Config {
    /// Forgegjo user/account name.
    username: String,
    /// API token with required permissions.
    token: String,
    /// URL to use for HTTPS requests.
    #[serde(default = "default_https_url")]
    https_url: String,
    /// URL to use for SSH requests.
    #[serde(default = "default_ssh_url")]
    ssh_url: String,
}

/// Default URL to use for HTTPS requests.
fn default_https_url() -> String { "https://gitea.bitcoin.ninja".to_string() }

/// Default URL to use for SSH requests.
fn default_ssh_url() -> String { "gitea-ssh.bitcoin.ninja".to_string() }

impl Config {
    fn api_url(&self) -> String { format!("{}/api/v1", self.https_url) }

    fn https_host(&self) -> &str {
        self.https_url.trim_start_matches("https://").trim_start_matches("http://")
    }
}

fn load_config() -> Result<Config> {
    // Try multiple config file locations in order
    let home = std::env::var("HOME").ok();
    let config_paths = vec![
        "./.forge.toml".to_string(),
        home.as_ref().map(|h| format!("{}/.config/forge.toml", h)).unwrap_or_default(),
        home.as_ref().map(|h| format!("{}/.forge.toml", h)).unwrap_or_default(),
    ];

    let mut last_error = None;
    for path in &config_paths {
        if path.is_empty() {
            continue;
        }

        match fs::read_to_string(path) {
            Ok(config_str) => {
                let config: Config = toml::from_str(&config_str)
                    .with_context(|| format!("Failed to parse config file at {}", path))?;
                return Ok(config);
            }
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    anyhow::bail!(
        "Failed to read config file. Tried locations: {}\nLast error: {}",
        config_paths.join(", "),
        last_error.map_or_else(|| "unknown".to_string(), |e| e.to_string())
    )
}

#[derive(Parser)]
#[command(name = "forge")]
#[command(about = "A CLI tool for Forgejo, similar to gh for GitHub")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pull request operations
    #[command(alias = "p")]
    Pr {
        #[command(subcommand)]
        command: PrCommands,
    },
    /// Fetch from all remotes using HTTPS (avoids SSH deploy key issues)
    #[command(alias = "f")]
    Fetch,
    /// Push using jj with HTTPS authentication (avoids SSH deploy key issues)
    Push {
        /// Push only current change (@) with -c flag
        #[arg(short = 'c', long)]
        current: bool,
    },
}

#[derive(Subcommand)]
enum PrCommands {
    /// List open pull requests in a repository
    #[command(alias = "l")]
    List {
        /// Repository in format "owner/repo" (optional, uses current repo if not specified)
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Checkout a pull request locally
    #[command(alias = "c")]
    Checkout {
        /// Pull request number
        pr_number: u64,
        /// Repository in format "owner/repo" (optional, uses current repo if not specified)
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Submit an approving ACK review on the currently checked out PR
    #[command(alias = "a")]
    Ack {
        /// Repository in format "owner/repo" (optional, uses current repo if not specified)
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Merge a pull request locally with signing
    #[command(alias = "m")]
    Merge {
        /// Pull request number
        pr_number: u64,
        /// Repository in format "owner/repo" (optional, uses current repo if not specified)
        #[arg(short, long)]
        repo: Option<String>,
        /// Branch to merge into (default: master or base branch from PR)
        #[arg(short, long)]
        branch: Option<String>,
    },
}

#[derive(Deserialize, Debug)]
struct PullRequest {
    number: u64,
    title: String,
    body: String,
    head: BranchInfo,
    base: BranchInfo,
}

#[derive(Deserialize, Debug)]
struct BranchInfo {
    #[serde(rename = "ref")]
    ref_name: String,
    repo: RepoInfo,
}

#[derive(Deserialize, Debug)]
struct RepoInfo {
    clone_url: String,
    full_name: String,
}

#[derive(Deserialize, Debug)]
struct ListItem {
    number: u64,
    title: String,
}

fn api_get<T: serde::de::DeserializeOwned>(api_url: &str, token: &str, path: &str) -> Result<T> {
    let url = format!("{}{}", api_url, path);
    let response = bitreq::get(&url)
        .with_header("Authorization", format!("token {}", token))
        .with_header("Accept", "*/*")
        // Masquerade as curl because the Forgejo instance blocked requests marked differently.
        .with_header("User-Agent", "curl/8.5.0")
        .send()
        .context("Failed to send request to Forgejo API")?;
    if response.status_code != 200 {
        anyhow::bail!("API request failed with status: {}", response.status_code);
    }
    response.json().context("Failed to parse API response")
}

fn api_post<B: Serialize>(api_url: &str, token: &str, path: &str, body: &B) -> Result<()> {
    let url = format!("{}{}", api_url, path);
    let body_json = serde_json::to_string(body).context("Failed to serialize request body")?;
    let response = bitreq::post(&url)
        .with_header("Authorization", format!("token {}", token))
        .with_header("Accept", "*/*")
        // Masquerade as curl because the Forgejo instance blocked requests marked differently.
        .with_header("Content-Type", "application/json")
        .with_header("User-Agent", "curl/8.5.0")
        .with_body(body_json.as_str())
        .send()
        .context("Failed to send request to Forgejo API")?;
    if response.status_code != 200 && response.status_code != 201 {
        anyhow::bail!("API request failed with status: {}", response.status_code);
    }
    Ok(())
}

/// RAII guard that configures `git config --local url.<https-with-token>.insteadOf`
/// for the configured host on construction, and removes the entries on drop.
///
/// Rewrites all three forms — `git@host:`, `ssh://git@host/`, and plain
/// `https://host/` — to a tokenized `https://<token>@host/` URL. Lets `git`
/// and `jj` (and any sub-command resolving URLs through git) transparently
/// authenticate over HTTPS without permanently mutating repo config — even
/// on early return or panic.
struct AuthHttpsGuard {
    key: String,
    added: Vec<String>,
}

impl AuthHttpsGuard {
    fn new(config: &Config) -> Result<Self> {
        let https_url = format!("https://{}@{}/", config.token, config.https_host());
        let key = format!("url.{}.insteadOf", https_url);
        let mut guard = Self { key: key.clone(), added: Vec::new() };

        for value in [
            format!("git@{}:", config.ssh_url),
            format!("ssh://git@{}/", config.ssh_url),
            format!("https://{}/", config.https_host()),
        ] {
            let out = Command::new("git")
                .args(["config", "--local", "--add", &key, &value])
                .output()
                .with_context(|| format!("Failed to configure git URL rewriting for {}", value))?;
            if !out.status.success() {
                // Drop will unset whatever made it in.
                anyhow::bail!("git config failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            guard.added.push(value);
        }
        Ok(guard)
    }
}

impl Drop for AuthHttpsGuard {
    fn drop(&mut self) {
        for value in &self.added {
            let _ = Command::new("git")
                .args(["config", "--local", "--unset", &self.key, value])
                .output();
        }
    }
}

fn list_prs(repo: Option<String>) -> Result<()> {
    let config = load_config()?;

    let repo = if let Some(r) = repo { r } else { get_current_repo(&config)? };

    let prs: Vec<ListItem> = api_get(
        &config.api_url(),
        &config.token,
        &format!("/repos/{}/pulls?state=open&limit=100", repo),
    )?;

    if prs.is_empty() {
        println!("No open pull requests found");
        return Ok(());
    }

    for pr in prs {
        println!("#{:<4} {}", pr.number, pr.title);
    }

    Ok(())
}

fn fetch_pr(repo: &str, pr_number: u64, config: &Config) -> Result<PullRequest> {
    api_get(&config.api_url(), &config.token, &format!("/repos/{}/pulls/{}", repo, pr_number))
}

fn fetch_pr_refs(repo: &str, refspecs: &[&str], config: &Config) -> Result<()> {
    let url = format!("{}/{}.git", config.https_url, repo);
    let mut args = vec!["fetch", &url];
    args.extend_from_slice(refspecs);
    let out = Command::new("git").args(&args).output().context("Failed to fetch PR refs")?;
    if !out.status.success() {
        anyhow::bail!("Failed to fetch: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

fn get_preferred_remote() -> String {
    // Prefer "upstream" over "origin" if it exists (for forks)
    if Command::new("git")
        .args(["remote", "get-url", "upstream"])
        .output()
        .is_ok_and(|o| o.status.success())
    {
        "upstream".to_string()
    } else {
        "origin".to_string()
    }
}

fn get_current_repo(config: &Config) -> Result<String> {
    // Try to get the current repo from git remote
    let remote = get_preferred_remote();

    let output = Command::new("git")
        .args(["remote", "get-url", &remote])
        .output()
        .context("Failed to execute git command")?;

    if !output.status.success() {
        anyhow::bail!("Not in a git repository or no {} remote found", remote);
    }

    let url = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in git remote URL")?
        .trim()
        .to_string();

    // Check for full SSH format: ssh://git@host/owner/repo
    let ssh_url_prefix = format!("ssh://git@{}/", config.ssh_url);
    if let Some(path) = url.strip_prefix(&ssh_url_prefix) {
        return Ok(path.strip_suffix(".git").unwrap_or(path).to_string());
    }

    // Check for SSH format: git@host:owner/repo
    let ssh_prefix = format!("git@{}:", config.ssh_url);
    if let Some(path) = url.strip_prefix(&ssh_prefix) {
        return Ok(path.strip_suffix(".git").unwrap_or(path).to_string());
    }

    // Check for HTTPS format: https://host/owner/repo
    let https_host = config.https_host();
    let https_prefix = format!("https://{}/", https_host);
    if let Some(path) = url.strip_prefix(&https_prefix) {
        return Ok(path.strip_suffix(".git").unwrap_or(path).to_string());
    }

    // Check for base host in URL (fallback)
    let host_slash = format!("{}/", https_host);
    if url.contains(&host_slash) {
        if let Some(path) = url.split(&host_slash).nth(1) {
            return Ok(path.strip_suffix(".git").unwrap_or(path).to_string());
        }
    }

    anyhow::bail!("Remote URL does not match configured hosts: {}", url)
}

/// Position the local branch `branch_name` at `FETCH_HEAD`, regardless of
/// whether it already exists or is currently checked out.
fn checkout_or_reset(branch_name: &str) -> Result<()> {
    let current = get_current_branch()?;

    if current == branch_name {
        println!("Resetting current branch {} to FETCH_HEAD...", branch_name);
        let output = Command::new("git")
            .args(["reset", "--hard", "FETCH_HEAD"])
            .output()
            .context("Failed to reset branch")?;
        if !output.status.success() {
            anyhow::bail!("Failed to reset: {}", String::from_utf8_lossy(&output.stderr));
        }
        return Ok(());
    }

    let exists = Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{}", branch_name)])
        .output()
        .is_ok_and(|o| o.status.success());

    if exists {
        println!("Checking out existing branch {} and resetting to FETCH_HEAD...", branch_name);
        let output = Command::new("git")
            .args(["checkout", branch_name])
            .output()
            .context("Failed to checkout PR branch")?;
        if !output.status.success() {
            anyhow::bail!("Failed to checkout: {}", String::from_utf8_lossy(&output.stderr));
        }
        let output = Command::new("git")
            .args(["reset", "--hard", "FETCH_HEAD"])
            .output()
            .context("Failed to reset branch")?;
        if !output.status.success() {
            anyhow::bail!("Failed to reset: {}", String::from_utf8_lossy(&output.stderr));
        }
    } else {
        println!("Creating branch {} from FETCH_HEAD...", branch_name);
        let output = Command::new("git")
            .args(["checkout", "-b", branch_name, "FETCH_HEAD"])
            .output()
            .context("Failed to create PR branch")?;
        if !output.status.success() {
            anyhow::bail!("Failed to checkout: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
    Ok(())
}

fn checkout_pr(pr_number: u64, repo: Option<String>) -> Result<()> {
    // Check if working directory is clean
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to check git status")?;

    if !status_output.stdout.is_empty() {
        anyhow::bail!(
            "Working directory is not clean. Please commit or stash your changes before checking out a PR.\n\
             Use 'git status' to see uncommitted changes."
        );
    }

    // Load config
    let config = load_config()?;
    let _guard = AuthHttpsGuard::new(&config)?;

    // Get repository
    let repo = if let Some(r) = repo { r } else { get_current_repo(&config)? };

    println!("Fetching PR #{} from {}...", pr_number, repo);

    // Fetch PR details
    let pr = fetch_pr(&repo, pr_number, &config)?;

    println!("PR #{}: {}", pr.number, pr.title);
    println!("From: {}/{}", pr.head.repo.full_name, pr.head.ref_name);
    println!("Into: {}/{}", pr.base.repo.full_name, pr.base.ref_name);

    let branch_name = format!("pr-{}", pr_number);
    let refspec = format!("refs/pull/{}/head", pr_number);

    println!("Fetching {} from {}/{}...", refspec, config.https_url, repo);
    fetch_pr_refs(&repo, &[&refspec], &config)?;

    checkout_or_reset(&branch_name)?;

    Command::new("git")
        .args(["config", &format!("branch.{}.pushRemote", branch_name), &pr.head.repo.clone_url])
        .output()
        .context("Failed to set branch.pushRemote config")?;

    println!("Successfully checked out PR #{}", pr_number);

    Ok(())
}

fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .context("Failed to execute git command")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get current branch");
    }

    Ok(String::from_utf8(output.stdout).context("Invalid UTF-8 in branch name")?.trim().to_string())
}

fn get_current_commit_hash() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Failed to execute git command")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get current commit hash");
    }

    Ok(String::from_utf8(output.stdout).context("Invalid UTF-8 in commit hash")?.trim().to_string())
}

fn extract_pr_number_from_branch(branch: &str) -> Result<u64> {
    if let Some(num_str) = branch.strip_prefix("pr-") {
        num_str.parse::<u64>().context("Failed to parse PR number from branch name")
    } else {
        anyhow::bail!("Not on a PR branch (branch name should be pr-<number>)")
    }
}

/// State of a Forgejo pull-request review. Only `Approved` is constructible
/// today; unknown values from the API deserialize to `Other`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PrReviewState {
    Approved,
    #[serde(other)]
    Other,
}

#[derive(Serialize)]
struct CreatePullReviewOptions {
    body: String,
    commit_id: String,
    event: PrReviewState,
}

#[derive(Deserialize, Debug)]
struct PullReview {
    body: String,
    commit_id: Option<String>,
    state: PrReviewState,
    user: CommentUser,
}

#[derive(Serialize, Deserialize, Debug)]
struct CommentUser {
    login: String,
}

#[derive(Deserialize, Debug)]
struct Comment {
    body: String,
    user: CommentUser,
}

fn get_pr_comments(repo: &str, pr_number: u64, config: &Config) -> Result<Vec<Comment>> {
    api_get(
        &config.api_url(),
        &config.token,
        &format!("/repos/{}/issues/{}/comments", repo, pr_number),
    )
}

fn get_pr_reviews(repo: &str, pr_number: u64, config: &Config) -> Result<Vec<PullReview>> {
    api_get(
        &config.api_url(),
        &config.token,
        &format!("/repos/{}/pulls/{}/reviews", repo, pr_number),
    )
}

fn scrape_acks(body: &str, login: &str, head_abbrev: &str, acks: &mut Vec<(String, String)>) {
    for line in body.lines() {
        if line.contains("ACK")
            && line.contains(head_abbrev)
            && !line.starts_with('>')
            && !line.starts_with("    ")
        {
            acks.push((login.to_string(), line.to_string()));
            break;
        }
    }
}

fn submit_pr_review(
    repo: &str,
    pr_number: u64,
    body: &CreatePullReviewOptions,
    config: &Config,
) -> Result<()> {
    api_post(
        &config.api_url(),
        &config.token,
        &format!("/repos/{}/pulls/{}/reviews", repo, pr_number),
        body,
    )
}

fn ack_pr(repo: Option<String>) -> Result<()> {
    // Load config
    let config = load_config()?;

    // Get repository
    let repo = if let Some(r) = repo { r } else { get_current_repo(&config)? };

    // Get current branch name
    let branch = get_current_branch()?;
    println!("Current branch: {}", branch);

    // Extract PR number from branch name
    let pr_number = extract_pr_number_from_branch(&branch)?;
    println!("Detected PR #{}", pr_number);

    // Get current commit hash
    let commit_hash = get_current_commit_hash()?;
    println!("Current commit: {}", commit_hash);

    // Format the ACK message
    let body = format!("ACK {}", commit_hash);

    println!("Checking existing reviews on PR #{}...", pr_number);
    let existing_reviews = get_pr_reviews(&repo, pr_number, &config)?;

    for r in &existing_reviews {
        if r.user.login == config.username
            && r.state == PrReviewState::Approved
            && r.commit_id.as_deref() == Some(commit_hash.as_str())
        {
            println!("PR already approved for this commit");
            return Ok(());
        }
    }

    println!("Submitting approving review on PR #{}...", pr_number);
    submit_pr_review(
        &repo,
        pr_number,
        &CreatePullReviewOptions { body, commit_id: commit_hash, event: PrReviewState::Approved },
        &config,
    )?;

    println!("Successfully submitted approving review!");

    Ok(())
}

fn check_for_symlinks() -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-tree", "--full-tree", "-r", "HEAD"])
        .output()
        .context("Failed to list git tree")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list git tree");
    }

    let mut symlinks = Vec::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            // File mode for symlinks is 120000 (octal)
            if let Ok(mode) = u32::from_str_radix(parts[0], 8) {
                if (mode & 0o170_000) == 0o120_000 {
                    if let Some(tab_pos) = line.find('\t') {
                        symlinks.push(line[tab_pos + 1..].to_string());
                    }
                }
            }
        }
    }

    Ok(symlinks)
}

fn compute_tree_sha512(repo_root: &std::path::Path) -> Result<String> {
    use std::io::{Read, Write};
    use std::process::Stdio;

    use sha2::{Digest, Sha512};

    // List all blobs, recursively. Mirrors tree_sha512sum()'s git ls-tree call.
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-tree", "--full-tree", "-r", "HEAD"])
        .output()
        .context("Failed to list git tree")?;
    if !output.status.success() {
        anyhow::bail!("git ls-tree failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Parse each line into (name_bytes, blob_sha). Keep names as raw bytes
    // (matching Python's bytes behaviour) so byte-sort is identical.
    let mut entries: Vec<(Vec<u8>, String)> = Vec::new();
    for line in output.stdout.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let tab =
            line.iter().position(|&b| b == b'\t').context("Malformed ls-tree line: no tab")?;
        let metadata =
            std::str::from_utf8(&line[..tab]).context("Non-UTF-8 in ls-tree metadata")?;
        let parts: Vec<&str> = metadata.split_whitespace().collect();
        if parts.len() < 3 {
            anyhow::bail!("Malformed ls-tree metadata: {}", metadata);
        }
        if parts[1] != "blob" {
            anyhow::bail!("Unexpected non-blob entry: {}", metadata);
        }
        entries.push((line[tab + 1..].to_vec(), parts[2].to_string()));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Open git-cat-file --batch for streaming blob content.
    let mut cat = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to spawn git cat-file")?;
    let mut cat_stdin = cat.stdin.take().context("Failed to take cat-file stdin")?;
    let mut cat_stdout = cat.stdout.take().context("Failed to take cat-file stdout")?;

    let mut overall = Sha512::new();
    let mut buf = vec![0u8; 65536];
    let mut hdr = Vec::with_capacity(128);

    for (name, blob_sha) in &entries {
        // Request blob.
        writeln!(cat_stdin, "{}", blob_sha).context("Failed to write to cat-file")?;
        cat_stdin.flush().context("Failed to flush cat-file stdin")?;

        // Read header line up to '\n'.
        hdr.clear();
        let mut byte = [0u8; 1];
        loop {
            cat_stdout.read_exact(&mut byte).context("Premature EOF reading cat-file header")?;
            if byte[0] == b'\n' {
                break;
            }
            hdr.push(byte[0]);
        }
        let hdr_str = std::str::from_utf8(&hdr).context("Non-UTF-8 in cat-file header")?;
        let hparts: Vec<&str> = hdr_str.split_whitespace().collect();
        if hparts.len() < 3 || hparts[0] != blob_sha || hparts[1] != "blob" {
            anyhow::bail!("Unexpected cat-file header: {}", hdr_str);
        }
        let size: usize = hparts[2].parse().context("Bad blob size in cat-file header")?;

        // Hash exactly `size` blob bytes.
        let mut intern = Sha512::new();
        let mut remaining = size;
        while remaining > 0 {
            let want = remaining.min(buf.len());
            cat_stdout.read_exact(&mut buf[..want]).context("Premature EOF reading blob")?;
            intern.update(&buf[..want]);
            remaining -= want;
        }

        // Consume the trailing LF that follows every blob.
        cat_stdout.read_exact(&mut byte).context("Failed to read trailing LF after blob")?;
        if byte[0] != b'\n' {
            anyhow::bail!("Expected LF after blob data, got 0x{:02x}", byte[0]);
        }

        // Feed hex(inner) + "  " + name + "\n" into overall hash.
        let dig = format!("{:x}", intern.finalize());
        overall.update(dig.as_bytes());
        overall.update(b"  ");
        overall.update(name);
        overall.update(b"\n");
    }

    drop(cat_stdin);
    let status = cat.wait().context("Failed to wait for git cat-file")?;
    if !status.success() {
        anyhow::bail!("git cat-file exited non-zero");
    }

    Ok(format!("{:x}", overall.finalize()))
}

fn ask_user(prompt: &str) -> Result<String> {
    use std::io::{self, Write};

    print!("{} ", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().to_string())
}

#[allow(clippy::too_many_lines)]
fn merge_pr(pr_number: u64, repo: Option<String>, branch: Option<String>) -> Result<()> {
    // Load config
    let config = load_config()?;
    let _guard = AuthHttpsGuard::new(&config)?;

    // Get repository
    let repo = if let Some(r) = repo { r } else { get_current_repo(&config)? };

    println!("Fetching PR #{} from {}...", pr_number, repo);

    // Fetch PR details
    let pr = fetch_pr(&repo, pr_number, &config)?;

    // Determine target branch
    let target_branch = branch.unwrap_or_else(|| pr.base.ref_name.clone());

    println!("PR #{}: {}", pr.number, pr.title);
    println!("Merging into branch: {}", target_branch);

    // Create temporary branch names
    let pull_str = pr_number.to_string();
    let head_branch = format!("pull/{}/head", pull_str);
    let base_branch = format!("pull/{}/base", pull_str);
    let local_merge_branch = format!("pull/{}/local-merge", pull_str);

    // Checkout target branch
    println!("Checking out {}...", target_branch);
    let output = Command::new("git")
        .args(["checkout", &target_branch])
        .output()
        .context("Failed to checkout target branch")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to checkout branch {}: {}",
            target_branch,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Fetch PR branches
    println!("Fetching PR branches...");
    let refspec = format!("+refs/pull/{}/head:refs/heads/{}", pull_str, head_branch);
    let base_refspec = format!("+refs/heads/{}:refs/heads/{}", target_branch, base_branch);
    fetch_pr_refs(&repo, &[&refspec, &base_refspec], &config)?;

    // Get head commit
    let output = Command::new("git")
        .args(["rev-parse", &head_branch])
        .output()
        .context("Failed to get head commit")?;

    let head_commit = String::from_utf8(output.stdout)?.trim().to_string();

    println!("Head commit: {}", head_commit);

    // Checkout base and create local merge branch
    Command::new("git")
        .args(["checkout", &base_branch])
        .output()
        .context("Failed to checkout base")?;

    // Delete old local merge branch if exists
    let _ = Command::new("git").args(["branch", "-D", &local_merge_branch]).output();

    Command::new("git")
        .args(["checkout", "-b", &local_merge_branch])
        .output()
        .context("Failed to create local merge branch")?;

    // Create merge commit message
    let pull_reference = format!("{}#{}", repo, pr_number);
    let first_line = format!("Merge {}: {}", pull_reference, pr.title);

    let mut message = format!("{}\n\n", first_line);

    // Add commit list
    let output = Command::new("git")
        .args([
            "log",
            "--no-merges",
            "--topo-order",
            "--pretty=format:%H %s (%an)",
            &format!("{}..{}", base_branch, head_branch),
        ])
        .output()
        .context("Failed to get commit log")?;

    message.push_str(&String::from_utf8(output.stdout)?);
    message.push_str("\n\nPull request description:\n\n  ");
    message.push_str(&pr.body.replace('\n', "\n  "));
    message.push('\n');

    // Attempt merge
    println!("Creating merge commit...");
    let output = Command::new("git")
        .args([
            "merge",
            "--commit",
            "--no-edit",
            "--no-ff",
            "--no-gpg-sign",
            "-m",
            &message,
            &head_branch,
        ])
        .output()
        .context("Failed to merge")?;

    if !output.status.success() {
        println!("ERROR: Cannot be merged cleanly");
        Command::new("git").args(["merge", "--abort"]).output().ok();
        anyhow::bail!("Merge conflict");
    }

    // Check for symlinks
    println!("Checking for symlinks...");
    let symlinks = check_for_symlinks()?;
    if !symlinks.is_empty() {
        for f in &symlinks {
            println!("ERROR: File '{}' is a symlink", f);
        }
        anyhow::bail!("Symlinks detected");
    }

    // Compute tree hash
    println!("Computing tree hash...");
    let first_sha512 = compute_tree_sha512(std::path::Path::new("."))?;
    println!("Tree-SHA512: {}", first_sha512);

    // Show merge details
    println!("\n{} {} into {}", pull_reference, pr.title, target_branch);
    Command::new("git")
        .args(["log", "--graph", "--topo-order", &format!("{}..{}", base_branch, head_branch)])
        .status()
        .ok();

    // Fetch ACKs
    println!("\nFetching ACKs...");
    let comments = get_pr_comments(&repo, pr_number, &config)?;
    let reviews = get_pr_reviews(&repo, pr_number, &config)?;

    let head_abbrev = &head_commit[0..6];
    let mut acks: Vec<(String, String)> = Vec::new();

    for c in &comments {
        scrape_acks(&c.body, &c.user.login, head_abbrev, &mut acks);
    }
    for r in &reviews {
        scrape_acks(&r.body, &r.user.login, head_abbrev, &mut acks);
    }

    // Add ACKs to message
    if acks.is_empty() {
        message.push_str("\n\nTop commit has no ACKs.\n");
        println!("\nWARNING: Top commit has no ACKs!");
    } else {
        message.push_str("\n\nACKs for top commit:\n");
        for (user, ack_msg) in &acks {
            message.push_str(&format!("  {}:\n    {}\n", user, ack_msg));
        }
        println!("\nFound {} ACK(s)", acks.len());
        for (user, msg) in &acks {
            println!("* {} ({})", msg, user);
        }
    }

    // Add tree hash to message
    message.push_str(&format!("\n\nTree-SHA512: {}", first_sha512));

    // Amend commit with full message
    Command::new("git")
        .args(["commit", "--amend", "--no-gpg-sign", "-m", &message])
        .output()
        .context("Failed to amend commit")?;

    // Interactive verification
    // Show the merge commit author and require explicit confirmation.
    // The Forgejo API does not expose enough information to validate the
    // author automatically, so this requires a human check.
    let author_out = Command::new("git")
        .args(["log", "-1", "--format=%an <%ae>"])
        .output()
        .context("Failed to read merge commit author")?;
    if !author_out.status.success() {
        anyhow::bail!("Failed to read merge commit author");
    }
    let author = String::from_utf8(author_out.stdout)?.trim().to_string();
    println!("\nMerge commit author: {}", author);
    let reply = ask_user("Is this the correct author for your Forgejo account? [y/N]:")?;
    if reply != "y" && reply != "Y" {
        println!("Author rejected. Aborting merge.");
        Command::new("git").args(["checkout", &target_branch]).output().ok();
        Command::new("git").args(["branch", "-D", &head_branch]).output().ok();
        Command::new("git").args(["branch", "-D", &base_branch]).output().ok();
        Command::new("git").args(["branch", "-D", &local_merge_branch]).output().ok();
        anyhow::bail!("Merge author not confirmed by user");
    }

    // Interactive shell for manual inspection
    println!("\nDropping you into a shell to test the merge.");
    println!("Run 'git diff HEAD~' to see changes.");
    println!("Type 'exit' when done.");

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    Command::new(shell).arg("-i").status().context("Failed to spawn shell")?;

    // Verify tree hash unchanged
    let second_sha512 = compute_tree_sha512(std::path::Path::new("."))?;
    if first_sha512 != second_sha512 {
        anyhow::bail!("ERROR: Tree hash changed unexpectedly");
    }

    // Ask for sign-off
    loop {
        let reply = ask_user("Type 's' to sign off on the merge, or 'x' to reject:")?;
        match reply.as_str() {
            "s" => {
                // Sign the commit
                println!("Signing commit...");
                let output = Command::new("git")
                    .args(["commit", "--amend", "--gpg-sign", "--no-edit"])
                    .output()
                    .context("Failed to sign commit")?;

                if !output.status.success() {
                    println!("Error signing commit, try again");
                    continue;
                }
                break;
            }
            "x" => {
                println!("Merge rejected");
                // Clean up branches
                Command::new("git").args(["checkout", &target_branch]).output().ok();
                Command::new("git").args(["branch", "-D", &head_branch]).output().ok();
                Command::new("git").args(["branch", "-D", &base_branch]).output().ok();
                Command::new("git").args(["branch", "-D", &local_merge_branch]).output().ok();
                anyhow::bail!("Merge rejected by user");
            }
            _ => {
                println!("Invalid input");
            }
        }
    }

    // Reset target branch to signed merge
    Command::new("git")
        .args(["checkout", &target_branch])
        .output()
        .context("Failed to checkout target branch")?;

    Command::new("git")
        .args(["reset", "--hard", &local_merge_branch])
        .output()
        .context("Failed to reset branch")?;

    println!("\nMerge complete! Branch {} updated.", target_branch);

    // Clean up temporary branches
    Command::new("git").args(["branch", "-D", &head_branch]).output().ok();
    Command::new("git").args(["branch", "-D", &base_branch]).output().ok();
    Command::new("git").args(["branch", "-D", &local_merge_branch]).output().ok();

    let push_url = format!("{}/{}.git", config.https_url, repo);

    // Ask about pushing
    loop {
        let reply = ask_user(&format!(
            "Type 'push' to push to {}/{}, or 'x' to exit:",
            repo, target_branch
        ))?;
        match reply.as_str() {
            "push" => {
                println!("Pushing...");
                let refspec = format!("refs/heads/{}", target_branch);
                let output = Command::new("git")
                    .args(["push", &push_url, &refspec])
                    .output()
                    .context("Failed to push")?;

                if !output.status.success() {
                    anyhow::bail!("Push failed: {}", String::from_utf8_lossy(&output.stderr));
                }
                println!("Pushed successfully!");

                // Note: If your repository has "Autodetect manual merge" enabled,
                // Forgejo will automatically detect this merge and mark the PR as merged.
                // This may take a few moments to process.
                println!(
                    "\nNote: If 'Autodetect manual merge' is enabled in your repository settings,"
                );
                println!(
                    "      Forgejo will automatically detect and mark PR #{} as merged.",
                    pr_number
                );
                println!("      This may take a few moments. Check the PR status in the web UI.");

                break;
            }
            "x" => {
                println!(
                    "Not pushing. You can push later with: git push <remote> {}",
                    target_branch
                );
                break;
            }
            _ => {
                println!("Invalid input");
            }
        }
    }

    Ok(())
}

fn fetch_all() -> Result<()> {
    // Load config
    let config = load_config()?;
    let _guard = AuthHttpsGuard::new(&config)?;

    // Get all remotes
    let output =
        Command::new("git").args(["remote"]).output().context("Failed to get git remotes")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list remotes");
    }

    let remotes = String::from_utf8(output.stdout)?;

    for remote in remotes.lines() {
        let remote = remote.trim();
        if remote.is_empty() {
            continue;
        }

        println!("Fetching from {}...", remote);

        let output =
            Command::new("git").args(["fetch", remote]).output().context("Failed to fetch")?;

        if output.status.success() {
            println!("Successfully fetched from {}", remote);
        } else {
            println!(
                "Warning: Failed to fetch from {}: {}",
                remote,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    // Also run jj git fetch if jj is available
    if Command::new("jj").arg("--version").output().is_ok_and(|o| o.status.success()) {
        println!("\nRunning jj git fetch...");

        let output = Command::new("jj")
            .args(["git", "fetch"])
            .output()
            .context("Failed to run jj git fetch")?;

        if output.status.success() {
            println!("jj git fetch completed successfully");
        } else {
            println!("Warning: jj git fetch failed: {}", String::from_utf8_lossy(&output.stderr));
        }
    }

    Ok(())
}

fn push_with_jj(current_only: bool) -> Result<()> {
    // Load config
    let config = load_config()?;

    // Check if jj is available
    if !Command::new("jj").arg("--version").output().is_ok_and(|o| o.status.success()) {
        anyhow::bail!("jj command not found. Please install jj (jujutsu) first.");
    }

    println!("Setting up HTTPS authentication for push...");
    let _guard = AuthHttpsGuard::new(&config)?;

    // Build jj command
    let mut args = vec!["git", "push"];
    if current_only {
        args.push("-c");
        args.push("@");
    }

    println!("Running jj git push{}...", if current_only { " -c @" } else { "" });

    let output = Command::new("jj").args(&args).output().context("Failed to run jj git push")?;

    // Print stdout
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    // Print stderr
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if !output.status.success() {
        anyhow::bail!("jj git push failed");
    }

    println!("Push completed successfully");

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Pr { command } => match command {
            PrCommands::List { repo } => {
                list_prs(repo)?;
            }
            PrCommands::Checkout { pr_number, repo } => {
                checkout_pr(pr_number, repo)?;
            }
            PrCommands::Ack { repo } => {
                ack_pr(repo)?;
            }
            PrCommands::Merge { pr_number, repo, branch } => {
                merge_pr(pr_number, repo, branch)?;
            }
        },
        Commands::Fetch => {
            fetch_all()?;
        }
        Commands::Push { current } => {
            push_with_jj(current)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command as Cmd;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::compute_tree_sha512;

    /// RAII guard that creates a fresh temporary directory on construction
    /// and removes it (recursively) on drop.
    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new() -> std::io::Result<Self> {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("forge-test-{}-{}", std::process::id(), n));
            std::fs::create_dir(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path { self.path.as_path() }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); }
    }

    /// Pinned vector: a repo with exactly two files:
    ///   a.txt = "hello\n"
    ///   b.txt = "world\n"
    ///
    /// Expected digest produced by github-merge.py's `tree_sha512sum()` algorithm.
    #[test]
    fn tree_sha512_matches_canonical() {
        let dir = TempDirGuard::new().unwrap();
        let root = dir.path();

        let run =
            |args: &[&str]| Cmd::new(args[0]).args(&args[1..]).current_dir(root).output().unwrap();
        run(&["git", "init"]);
        run(&["git", "config", "user.email", "test@test.com"]);
        run(&["git", "config", "user.name", "Test"]);
        std::fs::write(root.join("a.txt"), b"hello\n").unwrap();
        std::fs::write(root.join("b.txt"), b"world\n").unwrap();
        run(&["git", "add", "."]);
        run(&["git", "commit", "-m", "init"]);

        let digest = compute_tree_sha512(root).unwrap();

        // Pinned by computing tree_sha512sum() from first principles on
        // this exact tree (a.txt="hello\n", b.txt="world\n"):
        //   inner_a = SHA512("hello\n").hexdigest()
        //   inner_b = SHA512("world\n").hexdigest()
        //   overall = SHA512((inner_a + "  a.txt\n") + (inner_b + "  b.txt\n"))
        assert_eq!(
            digest,
            "0879634a7a0b2a60c156a6eaf0301db4afcf209b58d01dc28a817edcd7226bb\
             f9864299559fff718d1c88998ca1d9a1768e7b1b053149a7276bc86cf127782de"
        );
    }

    #[test]
    fn pr_review_state_serde() {
        use super::PrReviewState;
        assert_eq!(serde_json::to_string(&PrReviewState::Approved).unwrap(), "\"APPROVED\"");
        assert_eq!(
            serde_json::from_str::<PrReviewState>("\"APPROVED\"").unwrap(),
            PrReviewState::Approved
        );
        assert_eq!(
            serde_json::from_str::<PrReviewState>("\"PENDING\"").unwrap(),
            PrReviewState::Other
        );
        assert_eq!(
            serde_json::from_str::<PrReviewState>("\"COMMENT\"").unwrap(),
            PrReviewState::Other
        );
        assert_eq!(serde_json::from_str::<PrReviewState>("\"\"").unwrap(), PrReviewState::Other);
    }
}
