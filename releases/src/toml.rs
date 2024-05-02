//! Types that represent a Rust crate manifest.
// Chronic NIH syndrome.

use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    package: Package,
    dependencies: Dependencies,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    authors: Vec<String>,
    license: String,
    repository: String,
    description: String,
    categories: Vec<String>,
    keywords: Vec<String>,
    readme: String,
    edition: String,
    rust_version: String,
}

#[derive(Deserialize)]
struct Dependencies {

}
