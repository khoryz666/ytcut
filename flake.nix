{
  description = "ytcut: local web frontend for downloading and trimming YouTube videos";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      runtimeDeps = pkgs: [ pkgs.yt-dlp pkgs.ffmpeg ];
    in
    {
      packages = forAllSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "ytcut";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/ytcut --prefix PATH : ${pkgs.lib.makeBinPath (runtimeDeps pkgs)}
          '';
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = runtimeDeps pkgs ++ [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.rust-analyzer ];
        };
      });
    };
}
