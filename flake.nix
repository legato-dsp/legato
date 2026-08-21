{
  description = "The toolchain for Legato with Rust nightly";

  nixConfig = {
    extra-substituters = [ "https://legato-dsp.cachix.org" ];
    extra-trusted-public-keys = [
      "legato-dsp.cachix.org-1:fUg2O/uwyu1SeJsxonkCjJa9c735WnjqUTVuBGlvizc="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, crane, rust-overlay, ... }:
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

          craneLib = (crane.mkLib pkgs).overrideToolchain nightly;

          src = nixpkgs.lib.fileset.toSource {
            root = ./crates;
            fileset = nixpkgs.lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./crates)
              (nixpkgs.lib.fileset.fileFilter (file: file.hasExt "legato") ./crates)
            ];
          };

          commonArgs = {
            inherit src;
            strictDeps = true;

            nativeBuildInputs = with pkgs; [ clang pkg-config ];
            buildInputs = with pkgs; [
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ udev alsa-lib jack2 ffmpeg_6-full ];

            RUSTFLAGS = if pkgs.stdenv.isx86_64 then "-C target-cpu=x86-64-v3" else "";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in f { inherit pkgs system nightly craneLib commonArgs cargoArtifacts; });
    in
    {
      devShells = forEachSystem ({ pkgs, nightly, commonArgs, ... }: {
        default = pkgs.mkShell {
          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs ++ [
            nightly
            pkgs.pre-commit
            pkgs.nodejs
            pkgs.pnpm
            pkgs.uv
          ];
        };
      });

      packages = forEachSystem ({ craneLib, commonArgs, cargoArtifacts, ... }: {
        default = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        generate-docs = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "--features docs --bin export-docs";
        });
      });
    };
}
