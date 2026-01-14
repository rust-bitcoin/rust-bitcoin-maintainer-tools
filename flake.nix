{
  description = "Rust Bitcoin Maintainer Tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }: {
    nixosModules.fuzzing = { config, lib, pkgs, ... }:
      with lib;
      let
        cfg = config.services.cargo-fuzz;
        # Getting a nightly version of rust since cargo-fuzz requires
        # it in order to set some unstable compile flags.
        rust = fenix.packages.${pkgs.system}.minimal.toolchain;
      in {
        options.services.cargo-fuzz = {
          enable = mkEnableOption "cargo-fuzz continuous fuzzing services" // {
            description = lib.mdDoc ''
              Enable continuous fuzzing services using cargo-fuzz.

              This module sets up systemd services that continuously fuzz configured
              targets from Rust projects. Each fuzz target runs as a separate service
              with configurable resource limits (CPU quota and memory maximum).

              The fuzzing happens in `/var/lib/fuzz/<project>/<target>/` with a
              dedicated `fuzz` user for isolation. The projects *must* follow
              cargo-fuzz conventions.
            '';
          };

          email = {
            enable = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Whether to send email notifications on fuzzing failures.

                When enabled, failure notifications are sent to the configured address.
                Requires a system sendmail implementation.
              '';
            };

            address = mkOption {
              type = types.str;
              default = "root";
              description = ''
                Email address or alias to send failure notifications to.

                Defaults to "root" following Unix convention. Configure system email
                aliases (e.g., via programs.msmtp or /etc/aliases) to route root mail
                to your actual email address.
              '';
              example = "admin@example.com";
            };
          };

          projects = mkOption {
            type = types.attrsOf (types.submodule {
              options = {
                repo = mkOption {
                  type = types.str;
                  description = ''
                    Git repository URL to clone.
                  '';
                  example = "https://github.com/rust-bitcoin/rust-bip324.git";
                };

                ref = mkOption {
                  type = types.str;
                  description = ''
                    Git ref (branch, tag, or commit) to checkout.
                  '';
                  example = "master";
                };

                targets = mkOption {
                  type = types.attrsOf (types.submodule {
                    options = {
                      cpuQuota = mkOption {
                        type = types.str;
                        default = "80%";
                        description = ''
                          CPU quota for this fuzz target.

                          Maps to systemd's CPUQuota setting, which limits CPU time as a
                          percentage of one CPU core. 200% would be two cores.

                          This prevents a single fuzz target from monopolizing system CPU,
                          allowing multiple targets to run concurrently without starving
                          each other or other system processes.
                        '';
                        example = "150%";
                      };

                      memoryMax = mkOption {
                        type = types.str;
                        default = "4G";
                        description = ''
                          Maximum memory (RAM) this fuzz target can use.

                          Maps to systemd's MemoryMax setting, which hard-limits the memory
                          available to the service. If the fuzzer tries to allocate more,
                          the kernel's OOM killer will terminate it, and systemd will
                          automatically restart it (due to Restart=always).

                          This prevents memory leaks or pathological test cases from
                          consuming all system memory and causing system instability.
                        '';
                        example = "8G";
                      };
                    };
                  });
                  default = {};
                  description = ''
                    Fuzz targets to run for this project.

                    The attribute name (key) is the exact cargo-fuzz target name.
                    Use underscores in the key to match the fuzz target name.

                    Each target can override cpuQuota (default: "80%") and memoryMax
                    (default: "4G"). An empty attribute set uses all defaults.
                  '';
                  example = literalExpression ''
                    {
                      receive_key = {
                        cpuQuota = "50%";
                        memoryMax = "2G";
                      };
                      receive_garbage = {};
                    }
                  '';
                };
              };
            });
            default = {};
            description = "Fuzzing projects with their targets";
          };
        };

        config = mkIf cfg.enable {
          users.users.fuzz = {
            isSystemUser = true;
            group = "fuzz";
            home = "/var/lib/fuzz";
            createHome = true;
          };
          users.groups.fuzz = {};

          systemd.services =
            let
              notifyService = optionalAttrs cfg.email.enable {
                "cargo-fuzz-notify@" = {
                  description = "Send email notification for failed cargo-fuzz service";
                  serviceConfig.Type = "oneshot";
                  script = ''
                    (
                      echo "To: ${cfg.email.address}"
                      echo "Subject: Fuzzing failure: $1"
                      echo ""
                      ${pkgs.systemd}/bin/journalctl -u "$1" -n 50
                    ) | ${pkgs.system-sendmail}/bin/sendmail -i -t
                  '';
                  # Pass the service name in.
                  scriptArgs = "%i";
                };
              };

              allTargets = flatten (
                mapAttrsToList (projectName: project:
                  mapAttrsToList (targetName: target: {
                    inherit projectName targetName target project;
                  }) project.targets
                ) cfg.projects
              );

              targetToService = { projectName, targetName, target, project }:
                nameValuePair "cargo-fuzz-${projectName}-${targetName}" ({
                  description = "Continuous fuzzing: ${projectName}/${targetName}";
                  after = [ "network.target" ];
                  wantedBy = [ "multi-user.target" ];
                  # cargo-fuzz needs rust toolchain and C/C++ compiler to build fuzz targets.
                  path = [ rust pkgs.stdenv.cc ];
                } // optionalAttrs cfg.email.enable {
                  onFailure = [ "cargo-fuzz-notify@%n.service" ];
                } // {
                  serviceConfig = {
                    Type = "simple";
                    User = "fuzz";
                    Group = "fuzz";
                    # A shallow clone of the code under test is created in the
                    # state directory and is where fuzzing happens.
                    StateDirectory = "fuzz/${projectName}/${targetName}";
                    WorkingDirectory = "%S/fuzz/${projectName}/${targetName}";
                    Restart = "always";
                    RestartSec = "5min";
                    CPUQuota = target.cpuQuota;
                    MemoryMax = target.memoryMax;

                    ExecStartPre = pkgs.writeShellScript "fuzz-setup-${targetName}" ''
                      set -euo pipefail

                      if [ ! -d "$STATE_DIRECTORY/.git" ]; then
                        ${pkgs.git}/bin/git clone --depth 1 --revision ${project.ref} ${project.repo} "$STATE_DIRECTORY"
                      else
                        cd "$STATE_DIRECTORY"
                        ${pkgs.git}/bin/git fetch --depth 1 origin ${project.ref}
                        ${pkgs.git}/bin/git checkout FETCH_HEAD
                      fi
                    '';

                    # cargo-fuzz compiles binaries at runtime (outside Nix's control), so those
                    # binaries don't get Nix's automatic library path patching. We work around
                    # this with LD_LIBRARY_PATH for the C++ standard library.
                    ExecStart = pkgs.writeShellScript "fuzz-run-${targetName}" ''
                      export LD_LIBRARY_PATH="${lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib ]}"
                      exec ${pkgs.cargo-fuzz}/bin/cargo-fuzz run ${targetName}
                    '';
                  };
                });
            in
              notifyService // listToAttrs (map targetToService allTargets);
        };
      };
  };
}
