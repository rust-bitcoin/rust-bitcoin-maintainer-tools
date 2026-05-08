use xshell::Shell;

use crate::environment::get_workspace_packages;

pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    for package in get_workspace_packages(sh, packages)? {
        sh.change_dir(&package.dir);
        if !sh.path_exists("generate-files.sh") {
            return Err(format!("Unable to generate files for {}. The script at {}/generate-files.sh doesn't exist. ", package.name, package.dir.display()).into());
        }

        let cmd = rbmt_cmd!(sh, "./generate-files.sh");
        let _ = cmd.run();

        let output = rbmt_cmd!(sh, "git diff --name-only").read()?;
        let changed: Vec<String> = output.lines().map(String::from).collect();
        if !changed.is_empty() {
            return Err(format!(
                "You have introduced changes that have resulted in changes to the following generated files:\n {}\nPlease resolve the diff in your working directory",
                changed.join("\n")).into());
        }
    }

    Ok(())
}
