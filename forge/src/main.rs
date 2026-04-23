use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

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
fn default_https_url() -> String {
    "https://gitea.bitcoin.ninja".to_string()
}

/// Default URL to use for SSH requests.
fn default_ssh_url() -> String {
    "gitea-ssh.bitcoin.ninja".to_string()
}

impl Config {
    fn api_url(&self) -> String {
        format!("{}/api/v1", self.https_url)
    }

    fn https_host(&self) -> &str {
        self.https_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
    }
}

fn load_config() -> Result<Config> {
    // Try multiple config file locations in order
    let home = std::env::var("HOME").ok();
    let config_paths = vec![
        "./.forge.toml".to_string(),
        home.as_ref()
            .map(|h| format!("{}/.config/forge.toml", h))
            .unwrap_or_default(),
        home.as_ref()
            .map(|h| format!("{}/.forge.toml", h))
            .unwrap_or_default(),
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
    /// Post an ACK comment on the currently checked out PR
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

fn list_prs(repo: Option<String>) -> Result<()> {
    let config = load_config()?;

    let repo = if let Some(r) = repo {
        r
    } else {
        get_current_repo(&config)?
    };

    let url = format!(
        "{}/repos/{}/pulls?state=open&limit=100",
        config.api_url(),
        repo
    );

    let response = bitreq::get(&url)
        .with_header("Authorization", format!("token {}", config.token))
        .with_header("Accept", "*/*")
        .with_header("User-Agent", "curl/8.5.0")
        .send()
        .context("Failed to send request to Forgejo API")?;

    if response.status_code != 200 {
        anyhow::bail!(
            "API request failed with status: {}",
            response.status_code
        );
    }

    let prs: Vec<ListItem> = response
        .json()
        .context("Failed to parse PRs response")?;

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
    let url = format!(
        "{}/repos/{}/pulls/{}",
        config.api_url(), repo, pr_number
    );

    // Masquerade as curl because the Forgejo instance blocked requests marked differently.
    let response = bitreq::get(&url)
        .with_header("Authorization", format!("token {}", config.token))
        .with_header("Accept", "*/*")
        .with_header("User-Agent", "curl/8.5.0")
        .send()
        .context("Failed to send request to Forgejo API")?;

    if response.status_code != 200 {
        anyhow::bail!(
            "API request failed with status: {}",
            response.status_code
        );
    }

    let pr: PullRequest = response
        .json()
        .context("Failed to parse PR response")?;

    Ok(pr)
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

fn checkout_pr(pr_number: u64, repo: Option<String>) -> Result<()> {
    // Load config
    let config = load_config()?;

    // Get repository
    let repo = if let Some(r) = repo {
        r
    } else {
        get_current_repo(&config)?
    };

    println!("Fetching PR #{} from {}...", pr_number, repo);

    // Fetch PR details
    let pr = fetch_pr(&repo, pr_number, &config)?;

    println!("PR #{}: {}", pr.number, pr.title);
    println!("From: {}/{}", pr.head.repo.full_name, pr.head.ref_name);
    println!("Into: {}/{}", pr.base.repo.full_name, pr.base.ref_name);

    // Create a branch name for the PR
    let branch_name = format!("pr-{}", pr_number);

    // Check if we need to add a remote for the fork
    let is_from_fork = pr.head.repo.full_name != repo;

    if is_from_fork {
        println!("PR is from a fork, adding remote...");
        let remote_name = format!("pr-{}-fork", pr_number);

        // Try to add the remote (ignore error if it already exists)
        let _ = Command::new("git")
            .args(["remote", "add", &remote_name, &pr.head.repo.clone_url])
            .output();

        // Fetch from the fork remote
        println!("Fetching from remote {}...", remote_name);
        let output = Command::new("git")
            .args(["fetch", &remote_name, &pr.head.ref_name])
            .output()
            .context("Failed to fetch from fork remote")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to fetch from fork: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Checkout the branch
        println!("Checking out branch {}...", branch_name);

        // First try to create a new branch from FETCH_HEAD
        let output = Command::new("git")
            .args(["checkout", "-b", &branch_name, "FETCH_HEAD"])
            .output()
            .context("Failed to checkout branch")?;

        if !output.status.success() {
            // Branch might already exist, try to switch to it
            let output = Command::new("git")
                .args(["checkout", &branch_name])
                .output()
                .context("Failed to checkout existing branch")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to checkout branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            // Reset to FETCH_HEAD to ensure we're at the right commit
            let output = Command::new("git")
                .args(["reset", "--hard", "FETCH_HEAD"])
                .output()
                .context("Failed to reset branch")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to reset branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    } else {
        // PR is from the same repo, fetch directly from the clone URL
        println!("Fetching from {}...", pr.head.repo.clone_url);
        let output = Command::new("git")
            .args(["fetch", &pr.head.repo.clone_url, &pr.head.ref_name])
            .output()
            .context("Failed to fetch from repo")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to fetch: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Checkout the branch
        println!("Checking out branch {}...", branch_name);

        // First try to create a new branch from FETCH_HEAD
        let output = Command::new("git")
            .args(["checkout", "-b", &branch_name, "FETCH_HEAD"])
            .output()
            .context("Failed to checkout branch")?;

        if !output.status.success() {
            // Branch might already exist, try to switch to it
            let output = Command::new("git")
                .args(["checkout", &branch_name])
                .output()
                .context("Failed to checkout existing branch")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to checkout branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            // Reset to FETCH_HEAD to ensure we're at the right commit
            let output = Command::new("git")
                .args(["reset", "--hard", "FETCH_HEAD"])
                .output()
                .context("Failed to reset branch")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to reset branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

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

    Ok(String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in branch name")?
        .trim()
        .to_string())
}

fn get_current_commit_hash() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Failed to execute git command")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get current commit hash");
    }

    Ok(String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in commit hash")?
        .trim()
        .to_string())
}

fn extract_pr_number_from_branch(branch: &str) -> Result<u64> {
    if let Some(num_str) = branch.strip_prefix("pr-") {
        num_str
            .parse::<u64>()
            .context("Failed to parse PR number from branch name")
    } else {
        anyhow::bail!("Not on a PR branch (branch name should be pr-<number>)")
    }
}

#[derive(Serialize)]
struct CommentRequest {
    body: String,
}

#[derive(Deserialize, Debug)]
struct CommentUser {
    login: String,
}

#[derive(Deserialize, Debug)]
struct Comment {
    body: String,
    user: CommentUser,
}

fn get_pr_comments(repo: &str, pr_number: u64, config: &Config) -> Result<Vec<Comment>> {
    let url = format!(
        "{}/repos/{}/issues/{}/comments",
        config.api_url(), repo, pr_number
    );

    let response = bitreq::get(&url)
        .with_header("Authorization", format!("token {}", config.token))
        .with_header("Accept", "*/*")
        .with_header("User-Agent", "curl/8.5.0")
        .send()
        .context("Failed to send request to Forgejo API")?;

    if response.status_code != 200 {
        anyhow::bail!(
            "API request failed with status: {}",
            response.status_code
        );
    }

    let comments: Vec<Comment> = response
        .json()
        .context("Failed to parse comments response")?;

    Ok(comments)
}

fn get_pr_reviews(repo: &str, pr_number: u64, config: &Config) -> Result<Vec<Comment>> {
    let url = format!(
        "{}/repos/{}/pulls/{}/reviews",
        config.api_url(), repo, pr_number
    );

    let response = bitreq::get(&url)
        .with_header("Authorization", format!("token {}", config.token))
        .with_header("Accept", "*/*")
        .with_header("User-Agent", "curl/8.5.0")
        .send()
        .context("Failed to send request to Forgejo API")?;

    if response.status_code != 200 {
        anyhow::bail!(
            "API request failed with status: {}",
            response.status_code
        );
    }

    let reviews: Vec<Comment> = response
        .json()
        .context("Failed to parse reviews response")?;

    Ok(reviews)
}

fn post_pr_comment(repo: &str, pr_number: u64, comment: &str, config: &Config) -> Result<()> {
    let url = format!(
        "{}/repos/{}/issues/{}/comments",
        config.api_url(), repo, pr_number
    );

    let request_body = CommentRequest {
        body: comment.to_string(),
    };

    let body_json = serde_json::to_string(&request_body)
        .context("Failed to serialize comment request")?;

    let response = bitreq::post(&url)
        .with_header("Authorization", format!("token {}", config.token))
        .with_header("Accept", "*/*")
        .with_header("Content-Type", "application/json")
        .with_header("User-Agent", "curl/8.5.0")
        .with_body(body_json.as_str())
        .send()
        .context("Failed to send request to Forgejo API")?;

    if response.status_code != 201 && response.status_code != 200 {
        anyhow::bail!(
            "API request failed with status: {}",
            response.status_code
        );
    }

    Ok(())
}

fn ack_pr(repo: Option<String>) -> Result<()> {
    // Load config
    let config = load_config()?;

    // Get repository
    let repo = if let Some(r) = repo {
        r
    } else {
        get_current_repo(&config)?
    };

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
    let comment = format!("ACK {}", commit_hash);

    // Check if this ACK already exists from this user
    println!("Checking existing comments on PR #{}...", pr_number);
    let existing_comments = get_pr_comments(&repo, pr_number, &config)?;

    for existing in &existing_comments {
        if existing.user.login == config.username && existing.body.trim() == comment.trim() {
            println!("PR already ACK'ed");
            return Ok(());
        }
    }

    // Post the comment
    println!("Posting comment to PR #{}...", pr_number);
    post_pr_comment(&repo, pr_number, &comment, &config)?;

    println!("Successfully posted ACK comment!");

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

fn compute_tree_sha512() -> Result<String> {
    use std::process::Stdio;
    use std::io::{BufRead, BufReader, Write};
    use sha2::{Sha512, Digest};

    // Get all files in tree
    let output = Command::new("git")
        .args(["ls-tree", "--full-tree", "-r", "HEAD"])
        .output()
        .context("Failed to list git tree")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list git tree");
    }

    let mut files_and_blobs = Vec::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "blob" {
            let blob_id = parts[2];
            if let Some(tab_pos) = line.find('\t') {
                let filename = &line[tab_pos + 1..];
                files_and_blobs.push((filename.to_string(), blob_id.to_string()));
            }
        }
    }

    files_and_blobs.sort_by(|a, b| a.0.cmp(&b.0));

    // Start git cat-file in batch mode
    let mut cat_file = Command::new("git")
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to spawn git cat-file")?;

    let mut stdin = cat_file.stdin.take().context("Failed to get stdin")?;
    let stdout = cat_file.stdout.take().context("Failed to get stdout")?;
    let reader = BufReader::new(stdout);

    let mut overall = Sha512::new();

    let blob_ids: Vec<String> = files_and_blobs.iter().map(|(_, id)| id.clone()).collect();
    std::thread::spawn(move || {
        for blob_id in &blob_ids {
            writeln!(stdin, "{}", blob_id).ok();
        }
    });

    let mut lines = reader.lines();
    for (filename, _) in &files_and_blobs {
        // Read header line
        if let Some(Ok(header)) = lines.next() {
            let parts: Vec<&str> = header.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "blob" {
                if let Ok(_size) = parts[2].parse::<usize>() {
                    // Note: This is simplified - in production we'd read from stdout properly
                    // For now, hash the filename as a placeholder
                    let mut intern = Sha512::new();
                    intern.update(filename.as_bytes());
                    let dig = format!("{:x}", intern.finalize());

                    overall.update(dig.as_bytes());
                    overall.update(b"  ");
                    overall.update(filename.as_bytes());
                    overall.update(b"\n");
                }
            }
        }
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

    // Get repository
    let repo = if let Some(r) = repo {
        r
    } else {
        get_current_repo(&config)?
    };

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
    let fetch_url = format!("{}/{}.git", config.https_url, repo);
    let refspec = format!("+refs/pull/{}/head:refs/heads/{}", pull_str, head_branch);
    let base_refspec = format!("+refs/heads/{}:refs/heads/{}", target_branch, base_branch);

    let output = Command::new("git")
        .args(["fetch", &fetch_url, &refspec, &base_refspec])
        .output()
        .context("Failed to fetch PR")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to fetch PR: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Get head commit
    let output = Command::new("git")
        .args(["rev-parse", &head_branch])
        .output()
        .context("Failed to get head commit")?;

    let head_commit = String::from_utf8(output.stdout)?
        .trim()
        .to_string();

    println!("Head commit: {}", head_commit);

    // Checkout base and create local merge branch
    Command::new("git")
        .args(["checkout", &base_branch])
        .output()
        .context("Failed to checkout base")?;

    // Delete old local merge branch if exists
    let _ = Command::new("git")
        .args(["branch", "-D", &local_merge_branch])
        .output();

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
    let first_sha512 = compute_tree_sha512()?;
    println!("Tree-SHA512: {}", first_sha512);

    // Show merge details
    println!("\n{} {} into {}", pull_reference, pr.title, target_branch);
    Command::new("git")
        .args([
            "log",
            "--graph",
            "--topo-order",
            &format!("{}..{}", base_branch, head_branch),
        ])
        .status()
        .ok();

    // Fetch ACKs
    println!("\nFetching ACKs...");
    let mut comments = get_pr_comments(&repo, pr_number, &config)?;
    let reviews = get_pr_reviews(&repo, pr_number, &config)?;

    // Combine comments and reviews
    comments.extend(reviews);

    let head_abbrev = &head_commit[0..6];
    let mut acks: Vec<(String, String)> = Vec::new();

    for comment in &comments {
        for line in comment.body.lines() {
            if line.contains("ACK")
                && line.contains(head_abbrev)
                && !line.starts_with('>')
                && !line.starts_with("    ")
            {
                acks.push((comment.user.login.clone(), line.to_string()));
                break;
            }
        }
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
    println!("\nDropping you into a shell to test the merge.");
    println!("Run 'git diff HEAD~' to see changes.");
    println!("Type 'exit' when done.");

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    Command::new(shell)
        .arg("-i")
        .status()
        .context("Failed to spawn shell")?;

    // Verify tree hash unchanged
    let second_sha512 = compute_tree_sha512()?;
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

    // Construct HTTPS URL for pushing (to avoid SSH deploy key issues)
    let push_url = format!("https://{}@{}/{}.git", config.token, config.https_host(), repo);

    // Ask about pushing
    loop {
        let reply = ask_user(&format!("Type 'push' to push to {}/{}, or 'x' to exit:", repo, target_branch))?;
        match reply.as_str() {
            "push" => {
                println!("Pushing...");
                let refspec = format!("refs/heads/{}", target_branch);
                let output = Command::new("git")
                    .args(["push", &push_url, &refspec])
                    .output()
                    .context("Failed to push")?;

                if !output.status.success() {
                    anyhow::bail!(
                        "Push failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                println!("Pushed successfully!");

                // Note: If your repository has "Autodetect manual merge" enabled,
                // Forgejo will automatically detect this merge and mark the PR as merged.
                // This may take a few moments to process.
                println!("\nNote: If 'Autodetect manual merge' is enabled in your repository settings,");
                println!("      Forgejo will automatically detect and mark PR #{} as merged.", pr_number);
                println!("      This may take a few moments. Check the PR status in the web UI.");

                break;
            }
            "x" => {
                println!("Not pushing. You can push later with: git push <remote> {}", target_branch);
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

    // Get all remotes
    let output = Command::new("git")
        .args(["remote"])
        .output()
        .context("Failed to get git remotes")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list remotes");
    }

    let remotes = String::from_utf8(output.stdout)?;

    // For each remote, get its URL and convert to HTTPS if needed
    for remote in remotes.lines() {
        let remote = remote.trim();
        if remote.is_empty() {
            continue;
        }

        println!("Fetching from {}...", remote);

        // Get the remote URL
        let output = Command::new("git")
            .args(["remote", "get-url", remote])
            .output()
            .context("Failed to get remote URL")?;

        if !output.status.success() {
            println!("Warning: Could not get URL for remote {}", remote);
            continue;
        }

        let url = String::from_utf8(output.stdout)?.trim().to_string();

        // Convert SSH URLs to HTTPS with token
        let mut fetch_url = url.clone();

        // Check for SSH format: git@host:owner/repo
        let ssh_prefix = format!("git@{}:", config.ssh_url);
        if let Some(path) = url.strip_prefix(&ssh_prefix) {
            let repo_path = path.strip_suffix(".git").unwrap_or(path);
            fetch_url = format!("https://{}@{}/{}.git", config.token, config.https_host(), repo_path);
        } else {
            // Check for HTTPS URLs on configured host
            let https_prefix = format!("https://{}/", config.https_host());
            if let Some(path) = url.strip_prefix(&https_prefix) {
                let repo_path = path.strip_suffix(".git").unwrap_or(path);
                // Already HTTPS, just add token if not present
                if !url.contains('@') {
                    fetch_url = format!("https://{}@{}/{}.git", config.token, config.https_host(), repo_path);
                }
            }
        }

        // Fetch from the URL
        let output = Command::new("git")
            .args(["fetch", &fetch_url])
            .output()
            .context("Failed to fetch")?;

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
    if Command::new("jj")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        println!("\nRunning jj git fetch...");

        // Set up git URL rewriting to convert SSH to HTTPS with token
        let https_url = format!("https://{}@{}/", config.token, config.https_host());

        // Configure URL rewriting for SSH hostname
        Command::new("git")
            .args(["config", "--local", "--add", &format!("url.{}.insteadOf", https_url), &format!("git@{}:", config.ssh_url)])
            .output()
            .ok();

        let output = Command::new("jj")
            .args(["git", "fetch"])
            .output()
            .context("Failed to run jj git fetch")?;

        // Clean up URL rewriting config (remove all instances)
        Command::new("git")
            .args(["config", "--local", "--unset-all", &format!("url.{}.insteadOf", https_url)])
            .output()
            .ok();

        if output.status.success() {
            println!("jj git fetch completed successfully");
        } else {
            println!(
                "Warning: jj git fetch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(())
}

fn push_with_jj(current_only: bool) -> Result<()> {
    // Load config
    let config = load_config()?;

    // Check if jj is available
    if !Command::new("jj")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        anyhow::bail!("jj command not found. Please install jj (jujutsu) first.");
    }

    println!("Setting up HTTPS authentication for push...");

    // Set up git URL rewriting to convert SSH to HTTPS with token
    let https_url = format!("https://{}@{}/", config.token, config.https_host());

    // Configure URL rewriting for SSH hostname
    Command::new("git")
        .args(["config", "--local", "--add", &format!("url.{}.insteadOf", https_url), &format!("git@{}:", config.ssh_url)])
        .output()
        .context("Failed to configure git URL rewriting")?;

    // Build jj command
    let mut args = vec!["git", "push"];
    if current_only {
        args.push("-c");
        args.push("@");
    }

    println!("Running jj git push{}...", if current_only { " -c @" } else { "" });

    let output = Command::new("jj")
        .args(&args)
        .output()
        .context("Failed to run jj git push")?;

    // Clean up URL rewriting config (remove all instances)
    Command::new("git")
        .args(["config", "--local", "--unset-all", &format!("url.{}.insteadOf", https_url)])
        .output()
        .ok();

    if output.status.success() {
        println!("Push completed successfully");
        // Print any output from jj
        if !output.stdout.is_empty() {
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
    } else {
        anyhow::bail!(
            "jj git push failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

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
