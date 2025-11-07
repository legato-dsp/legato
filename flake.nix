{
  description = "A minimal development and testing environment for Legato";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-parts.url = "github:hercules-ci/flake-parts";

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
  };

  outputs = inputs@{ self, nixpkgs, uv2nix, pyproject-nix, pyproject-build-systems, flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "x86_64-darwin" "aarch64-darwin" "aarch64-linux" ];

      perSystem = { config, pkgs, lib, system, ... }: let
        # define uv workspace for flake
        uvWorkspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./scripts; };

        overlay = uvWorkspace.mkPyprojectOverlay {
          sourcePreference = "wheel";
        };

        pyprojectOverrides = _final: _prev: {
          # Implement build fixups here.
        };

        python = pkgs.python313;

        pythonSet = 
          (pkgs.callPackage pyproject-nix.build.packages {
            inherit python;
          }).overrideScope(
            lib.composeManyExtensions [
              pyproject-build-systems.overlays.default
              overlay
              pyprojectOverrides
            ]
          );

        venv = pythonSet.mkVirtualEnv "development-scripts-default-env" uvWorkspace.deps.default;

      in {

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ 
            uv
            cargo
            rustc
            rustfmt 
            alsa-lib
            pre-commit
            rustPackages.clippy
            jack2
            ffmpeg_6-full           
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
            clang
          ];
          packages = [ venv ];
        };

        apps = {
          spectrogram = {
            type = "app";
            program = pkgs.writeShellScriptBin "spectrogram" ''
              ${venv}/bin/python ${./scripts/spectrogram.py} "$@"
            '';
          };
        };
      };
    };

}