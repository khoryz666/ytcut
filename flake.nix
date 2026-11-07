{
  description = "ytcut: download YouTube songs, trim them, and play them on repeat";

  # Only the dev environment. The app itself fetches yt-dlp, ffmpeg and deno at
  # run time and updates itself; see src/tools.rs.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in {
      devShells.x86_64-linux.default = pkgs.mkShell {
        packages = [
          pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.rust-analyzer
          pkgs.nodejs
        ];
      };
    };
}
