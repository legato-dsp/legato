{
  description = "The toolchain for Legato with Rust nightly";

  nixConfig = {
    extra-substituters = [ "legato-dsp.cachix.org" ];
    extra-trusted-public-keys = [
      "legato-dsp.cachix.org-1:fUg2O/uwyu1SeJsxonkCjJa9c735WnjqUTVuBGlvizc="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, naersk, rust-overlay, ... }:
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
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ udev alsa-lib jack2 ffmpeg_6-full   ];
          };
        in f { inherit pkgs system nightly naersk' commonArgs; });
    in
    {
      devShells = forEachSystem ({ pkgs, nightly, commonArgs, ... }:
        pkgs.mkShell {
          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs ++ [
            nightly
            pkgs.pre-commit
            pkgs.nodejs
            pkgs.pnpm
            pkgs.uv
          ];
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
          singleStep = true;

          nativeBuildInputs = commonArgs.nativeBuildInputs;
          buildInputs = commonArgs.buildInputs;
          cargoBuildOptions = prev: prev ++ [ "--features" "docs" "--bin" "export-docs" ];
        };
      });
    };
}
