{
  description = "A minimal Legato downstream example";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    legato.url = "github:legato-dsp/legato";
  };

  outputs = { self, nixpkgs, legato }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs { inherit system; };
        inherit system;
      });
    in
    {
      devShells = forAllSystems ({ pkgs, system }: {
        default = pkgs.mkShell {
          # Pulling in the devShell with the nightly toolchain
          inputsFrom = [ legato.devShells.${system}.default ];

          packages = with pkgs; [
            alsa-lib
          ];

          nativeBuildInputs = [
            (pkgs.writeShellScriptBin "run-release" ''
              exec cargo run --release"$@"
            '')
          ];

          # Baseview links these dynamically
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
            libGL
            libxkbcommon
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
            wayland
          ]);
        };
      });

      packages = forAllSystems ({ pkgs, ... }: {
        # example-pkg = pkgs.callPackage ./pkg.nix {};
      });
    };
}
