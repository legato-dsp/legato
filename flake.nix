{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
      in
      {
        defaultPackage = naersk-lib.buildPackage ./.;
        devShell = with pkgs; mkShell {
          nativeBuildInputs = [
            pkg-config
            clang
          ];
          buildInputs = [ 
            cargo 
            rustc 
            rustfmt 
            alsa-lib 
            pre-commit 
            rustPackages.clippy 
            linuxKernel.packages.linux_zen.perf
            jack2
            ffmpeg_6-full
            ];
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
        };
      }
    );
}