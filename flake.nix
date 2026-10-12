{
  description = "Development shell for claude-code-rust with CI's Rust channel";

  inputs = {
    # Stable, not unstable: nixpkgs-unstable (26.11) failed to evaluate on
    # x86_64-darwin (`nix flake check --all-systems`, 2026-10-12), while 26.05 is
    # the last release to support it. Move to the next stable branch when that
    # system is no longer needed.
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          # Follows rust-toolchain.toml (channel and components), so the shell
          # moves with the file instead of a second copy of the version.
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              # CI's agent-sdk jobs run Node 24 (engines.node is >=24).
              pkgs.nodejs_24
              # Debug builds spawn the bridge with `bun` from PATH
              # (docs/src/development.md), so `cargo run` needs it.
              pkgs.bun
              # tests/ and recordings/ scripts.
              pkgs.jq
              pkgs.tmux
              # CI shellchecks scripts/install/install.sh (pr.yml, npm-package-layout).
              pkgs.shellcheck
              # The cargo-deny-policy job of pr.yml, run by scripts/dev/check-all.mjs.
              pkgs.cargo-deny
            ];

            # The banner goes to stderr: under `nix develop -c cmd` stdout is the
            # command's, and a banner there corrupts piped output.
            # CARGO_BUILD_JOBS is deliberately not set: .cargo/config.toml caps
            # jobs at 1 to bound compiler and linker memory (CONTRIBUTING.md).
            shellHook = ''
              echo "claude-code-rust dev shell: $(rustc --version), node $(node --version); node scripts/dev/check-all.mjs reproduces CI; raise build parallelism with cargo's -j N" >&2
            '';
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
