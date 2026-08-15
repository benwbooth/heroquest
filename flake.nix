{
  description = "HeroQuest-compatible 3D dungeon board in Rust, SDL3, wgpu, and Rapier";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              cargo
              clippy
              cmake
              ninja
              pkg-config
              rustc
              rustfmt
            ];

            buildInputs = with pkgs; [
              alsa-lib
              dbus
              libGL
              libdecor
              libpulseaudio
              libusb1
              libxkbcommon
              sdl3
              systemd
              vulkan-loader
              wayland
              libx11
              libxscrnsaver
              libxcursor
              libxext
              libxfixes
              libxi
              libxrandr
              libxrender
              libxtst
            ];

            LD_LIBRARY_PATH = nixpkgs.lib.makeLibraryPath (with pkgs; [
              libGL
              libdecor
              libxkbcommon
              sdl3
              vulkan-loader
              wayland
            ]);

            FLAKE_INPUTS = builtins.concatStringsSep ":" (
              builtins.attrValues (builtins.mapAttrs (_: input: input.outPath)
                (builtins.removeAttrs self.inputs [ "self" ]))
            );
          };
        });
    };
}
