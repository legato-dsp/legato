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
      forEachSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: 
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          
          nightly = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
            extensions = [ "rust-src" "clippy" "rustfmt" ];
          });

          naersk' = naersk.lib.${system}.override {
            cargo = nightly;
            rustc = nightly;
          };
          
          commonArgs = {
            nativeBuildInputs = with pkgs; [ clang pkg-config ];
            buildInputs = with pkgs; [     
              # TODO: Tidy this up, currently doing this so I can still write scripts on MacOS
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ udev alsa-lib jack2 ffmpeg_6-full   ];
          };
        in f { inherit pkgs system nightly naersk' commonArgs; });
    in
    {
      devShells = forEachSystem ({ pkgs, nightly, commonArgs, ... }: {
        default = let
          uvWorkspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./scripts; };
          pythonSet = (pkgs.callPackage pyproject-nix.build.packages { python = pkgs.python313; })
            .overrideScope (nixpkgs.lib.composeManyExtensions [
              pyproject-build-systems.overlays.default
              (uvWorkspace.mkPyprojectOverlay { sourcePreference = "wheel"; })
            ]);
          venv = pythonSet.mkVirtualEnv "dev-scripts-env" uvWorkspace.deps.default;
        in
        pkgs.mkShell {
          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs ++ [
            nightly
            pkgs.pre-commit 
            pkgs.nodejs 
            pkgs.pnpm 
            pkgs.uv 
            venv
          ];

          env = {
            RUSTFLAGS = "-C target-cpu=native";
          };

          shellHook = ''
            unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
          '';
        };
      });

      packages = forEachSystem ({ pkgs, nightly, naersk', commonArgs, ... }: {
        default = naersk'.buildPackage {
          src = ./crates;
          cargo = nightly;
          rustc = nightly;

          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs;
          RUSTFLAGS = if pkgs.stdenv.isx86_64 then "-C target-cpu=x86-64-v3" else "";
        };

        generate-docs = naersk'.buildPackage {
          src = ./crates;
          cargo = nightly;
          rustc = nightly;

          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs;
          cargoBuildOptions = prev: prev ++ [ "--features" "docs" "--bin" "export-docs" ];
        };
      });

      apps = forEachSystem ({ pkgs, ... }@args: 
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
          
          generate-docs = {
            type = "app";
            program = "${self.packages.${args.system}.generate-docs}/bin/export-docs";
          };
      });
    };
}