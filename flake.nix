{
  description = "ytcut: download YouTube songs, trim them, and play them on repeat";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      runtimeDeps = pkgs: [ pkgs.yt-dlp pkgs.ffmpeg ];
    in
    {
      packages = forAllSystems (pkgs: rec {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "ytcut";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/ytcut --prefix PATH : ${pkgs.lib.makeBinPath (runtimeDeps pkgs)}
          '';
          meta.mainProgram = "ytcut";
        };

        # Open ~/Music in an app window of the first Chromium-based browser found, with its
        # own profile. The server stops by itself once every window is closed and no
        # download runs; a second copy exits at once.
        open = pkgs.writeShellApplication {
          name = "ytcut-open";
          runtimeInputs = [ pkgs.coreutils pkgs.util-linux ];
          text = ''
            port=''${PORT:-8765}
            url="http://127.0.0.1:$port"
            data="''${XDG_DATA_HOME:-$HOME/.local/share}/ytcut"

            browser=""
            for candidate in "''${YTCUT_BROWSER:-}" google-chrome-stable google-chrome chromium \
              chromium-browser brave-browser brave microsoft-edge-stable microsoft-edge \
              vivaldi-stable vivaldi; do
              if [ -n "$candidate" ] && command -v "$candidate" >/dev/null; then
                browser=$candidate
                break
              fi
            done
            # yt-dlp's name for the browser, which decides where it finds the cookie key.
            case $browser in
              *chromium*) kind=chromium ;;
              *brave*) kind=brave ;;
              *edge*) kind=edge ;;
              *vivaldi*) kind=vivaldi ;;
              *) kind=chrome ;;
            esac
            profile="$data/$kind"

            up() { (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; }
            if ! up; then
              cookies=()
              if [ -n "$browser" ]; then
                cookies=(--cookies-from-browser="$kind:$profile/Default")
              fi
              setsid ${default}/bin/ytcut --exit-when-idle "''${cookies[@]}" "$HOME/Music" \
                >/dev/null 2>&1 </dev/null &
              for _ in $(seq 50); do up && break; sleep 0.1; done
            fi

            if [ -z "$browser" ]; then
              exec xdg-open "$url"
            fi
            exec "$browser" --app="$url" --class=ytcut --user-data-dir="$profile" \
              --no-first-run --no-default-browser-check
          '';
        };

      });

      apps = forAllSystems (pkgs:
        let
          app = name: {
            type = "app";
            program = pkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.${name};
          };
        in
        { default = app "open"; open = app "open"; });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = runtimeDeps pkgs ++ [
            pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.rust-analyzer
          ];
        };
      });
    };
}
