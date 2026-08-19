{
  description = "A minimal development and testing environment for Legato with Rust nightly";

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

          env = {
            RUSTFLAGS = "-C target-cpu=native";
          };

          shellHook = ''
            unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
          '';
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
