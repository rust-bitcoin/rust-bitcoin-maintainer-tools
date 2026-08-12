// SPDX-License-Identifier: MIT AND Apache-2.0

//! Analyze the internal workspace dependency tree.

use std::collections::{BTreeSet, HashMap, HashSet};

use xshell::Shell;

use crate::environment::{get_workspace_packages, Package, ProgressGuard};
use crate::git;

/// Analyze the internal workspace dependency tree.
///
/// # Arguments
///
/// * `roots` - Root packages to analyze. If empty, analyzes the whole workspace.
/// * `baseline` - If given, only show packages with changes since that git ref.
pub fn run(
    sh: &Shell,
    roots: &[String],
    baseline: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();
    let packages = internal_packages(sh, roots)?;
    if packages.is_empty() {
        return Err("No publishable packages found".into());
    }
    print_release_waves(sh, &packages, baseline)
}

/// Collect publishable workspace packages with only their internal dependencies.
fn internal_packages(
    sh: &Shell,
    roots: &[String],
) -> Result<Vec<Package>, Box<dyn std::error::Error>> {
    // Publishable packages in a workspace.
    let mut packages: Vec<Package> =
        get_workspace_packages(sh, &[])?.into_iter().filter(|p| p.publish).collect();
    // Filter dependencies to just internal workspace memebers.
    let names: HashSet<String> = packages.iter().map(|p| p.name.clone()).collect();
    for package in &mut packages {
        package.deps.retain(|d| names.contains(d));
    }

    // If a roots are given, filter out packages not in any of the root internal dependency graphs.
    if !roots.is_empty() {
        let selected = get_workspace_packages(sh, roots)?;
        let by_name: HashMap<&str, &Package> =
            packages.iter().map(|p| (p.name.as_str(), p)).collect();

        let mut keep: HashSet<String> = HashSet::new();
        // Depth-first traversal of root internal dependency graphs.
        let mut stack: Vec<String> = selected.into_iter().map(|p| p.name).collect();
        while let Some(name) = stack.pop() {
            if keep.insert(name.clone()) {
                if let Some(package) = by_name.get(name.as_str()) {
                    stack.extend(package.deps.iter().cloned());
                }
            }
        }
        packages.retain(|p| keep.contains(&p.name));
    }

    Ok(packages)
}

/// Print packages in release waves, deepest internal dependency first.
fn print_release_waves(
    sh: &Shell,
    packages: &[Package],
    baseline: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Invert the graph, map each package to its internal dependents.
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for package in packages {
        for dep in &package.deps {
            dependents.entry(dep.as_str()).or_default().push(package.name.as_str());
        }
    }

    // Calculate each package's depth from the roots. Keep only the packages to show: all of them,
    // or only those with changes since the baseline. `depths` collects the distinct depths of the
    // shown packages.
    let mut memo: HashMap<&str, usize> = HashMap::new();
    let mut visible: Vec<(&Package, usize)> = Vec::new();
    let mut depths: BTreeSet<usize> = BTreeSet::new();
    for package in packages {
        let show = match baseline {
            Some(baseline) => git::has_changes_since(sh, baseline, &package.dir)?,
            None => true,
        };
        if show {
            let depth = package_depth(&package.name, &dependents, &mut memo);
            depths.insert(depth);
            visible.push((package, depth));
        }
    }
    if let Some(baseline) = baseline {
        if visible.is_empty() {
            println!("No packages changed since {baseline}");
            return Ok(());
        }
    }

    // Sort alphabetically; the loop below supplies the depth ordering.
    visible.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));

    // A package's *wave* is the index of its depth, which consolidates gaps left by hidden
    // (unchanged) packages. Print deepest wave first (the ones that have to be released first).
    for (wave, depth) in depths.iter().enumerate().rev() {
        for (package, _) in visible.iter().filter(|(_, d)| d == depth) {
            println!("{:>2}  {}", wave, package.name);
        }
    }

    Ok(())
}

/// Recursively compute the depth of a package, with memoization.
///
/// A package with no internal dependents has depth 0. Otherwise, its depth is *one more than the
/// maximum depth of its dependents*. Cargo guarantees the dependency graph is acyclic, so not
/// checking for cycles.
fn package_depth<'a>(
    name: &'a str,
    dependents: &HashMap<&'a str, Vec<&'a str>>,
    memo: &mut HashMap<&'a str, usize>,
) -> usize {
    if let Some(&depth) = memo.get(name) {
        return depth;
    }

    let depth = match dependents.get(name) {
        None => 0,
        Some(deps) =>
            deps.iter().map(|dep| package_depth(dep, dependents, memo)).max().unwrap_or(0) + 1,
    };

    memo.insert(name, depth);
    depth
}
