# Updated flake.nix with nightly Rust toolchain integration
{
  description = "A minimal development and testing environment for Legato with Rust nightly";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    # Python infra
    pyproject-nix = {
      url = "github:pyproject-nix/pyproject.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    uv2nix = {
      url = "github:pyproject-nix/uv2nix";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    pyproject-build-systems = {
      url = "github:pyproject-nix/build-system-pkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.uv2nix.follows = "uv2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Rust nightly machinery
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs@{ self, nixpkgs, uv2nix, pyproject-nix, pyproject-build-systems, flake-parts, naersk, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "x86_64-darwin" "aarch64-darwin" "aarch64-linux" ];

      perSystem = { config, pkgs, lib, system, ... }: let
        # apply rust overlay
        overlays = [ (import rust-overlay) ];
        pkgs' = import nixpkgs { inherit system overlays; };

        # nightly toolchain
        nightly = pkgs'.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
          extensions = [ "rust-src" "clippy" ];
        });

        naersk' = pkgs'.callPackage naersk { };

        uvWorkspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./scripts; };
        overlay = uvWorkspace.mkPyprojectOverlay { sourcePreference = "wheel"; };

        pyprojectOverrides = _final: _prev: { };

        python = pkgs'.python313;
        pythonSet =
          (pkgs'.callPackage pyproject-nix.build.packages { inherit python; })
          .overrideScope (lib.composeManyExtensions [
            pyproject-build-systems.overlays.default
            overlay
            pyprojectOverrides
          ]);

        venv = pythonSet.mkVirtualEnv "development-scripts-default-env" uvWorkspace.deps.default;

      in {
        devShells.default = pkgs'.mkShell {
          RUSTFLAGS = "-C target-cpu=native";
          buildInputs = with pkgs'; [
            uv
            # rust nightly
            nightly
            cargo
            rustc
            rustfmt
            rustPackages.clippy
            # audio stack
            alsa-lib
            jack2
            ffmpeg_6-full
            # misc
            pre-commit
            nodejs
            pnpm
          ];

          nativeBuildInputs = with pkgs'; [
            clang
            pkg-config
          ];

          packages = [ venv ];
        };

        packages = {
          rust-nightly-build = naersk'.buildPackage {
          RUSTFLAGS = "-C target-cpu=native";
            src = ./.;
            nativeBuildInputs = [ nightly ];
          };
        };

        apps = {
          spectrogram = {
            type = "app";
            program = pkgs'.writeShellScriptBin "spectrogram" ''
              ${venv}/bin/python ${./scripts/dsp/spectrogram.py} "$@"
            '';
          };

          filter-design = {
            type = "app";
            program = pkgs'.writeShellScriptBin "filter-design" ''
              ${venv}/bin/python ${./scripts/dsp/filter-design.py} "$@"
            '';
          };
        };
      };
    };
}
