{
  description = "A minimal development and testing environment for Legato with Rust nightly";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
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
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, uv2nix, pyproject-nix, pyproject-build-systems, naersk, rust-overlay, ... }:
    let
      supportedSystems = [ "x86_64-linux" "x86_64-darwin" "aarch64-darwin" "aarch64-linux" ];      
      forEachSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        inherit system;
      });
    in
    {
      devShells = forEachSystem ({ pkgs, ... }: {
        default = let
          nightly = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
            extensions = [ "rust-src" "clippy" ];
          });
          
          uvWorkspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./scripts; };
          pythonSet = (pkgs.callPackage pyproject-nix.build.packages { python = pkgs.python313; })
            .overrideScope (nixpkgs.lib.composeManyExtensions [
              pyproject-build-systems.overlays.default
              (uvWorkspace.mkPyprojectOverlay { sourcePreference = "wheel"; })
            ]);
          venv = pythonSet.mkVirtualEnv "dev-scripts-env" uvWorkspace.deps.default;
        in
        pkgs.mkShell {          
          nativeBuildInputs = with pkgs; [ 
            clang 
            pkg-config 
          ];
          
          buildInputs = with pkgs; [
            uv 
            nightly
            rustfmt 
            # audio stack
            alsa-lib 
            jack2 
            ffmpeg_6-full
            # misc
            pre-commit 
            nodejs 
            pnpm 
            venv
          ];

          env = {
            RUSTFLAGS = "-C target-cpu=native";
          };

          shellHook = ''
            echo "--- Legato Dev Environment ---"
            echo "Optimization: target-cpu=native"
          '';
        };
      });

      packages = forEachSystem ({ pkgs, system }: 
        let
          nightly = pkgs.rust-bin.selectLatestNightlyWith (t: t.default);
          naersk' = pkgs.callPackage naersk { };
          platformFlags = if pkgs.stdenv.isx86_64 then "-C target-cpu=x86-64-v3" else "";
        in {
          default = naersk'.buildPackage {
            src = ./.;
            nativeBuildInputs = [ nightly ];
            RUSTFLAGS = platformFlags;
          };
      });

      apps = forEachSystem ({ pkgs, ... }: 
        let
          mkApp = name: scriptPath: {
            type = "app";
            program = "${pkgs.writeShellScriptBin name ''
              python ${scriptPath} "$@"
            ''}/bin/${name}";
          };
        in {
          spectrogram = mkApp "spectrogram" ./scripts/dsp/spectrogram.py;
          filter-design = mkApp "filter-design" ./scripts/dsp/filter-design.py;
      });
    };
}