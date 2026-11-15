{
  description = "Cat tracker: Rust API + Svelte PWA";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          inherit (pkgs) lib;
          fs = lib.fileset;
          version = (lib.importTOML ./backend/Cargo.toml).package.version;
          # Commit the build came from, shown in Settings and at /api/version (matches the image's sha- tag).
          rev = self.shortRev or self.dirtyShortRev or "unknown";

          # The built PWA (what `npm run build` puts in frontend/dist).
          frontend = pkgs.buildNpmPackage {
            pname = "cattracker-frontend";
            inherit version;
            src = fs.toSource {
              root = ./frontend;
              fileset = fs.difference ./frontend (
                fs.unions [
                  (fs.maybeMissing ./frontend/node_modules)
                  (fs.maybeMissing ./frontend/dist)
                ]
              );
            };
            # Dependencies come straight from package-lock.json, so there is no hash to update.
            npmDeps = pkgs.importNpmLock { npmRoot = ./frontend; };
            npmConfigHook = pkgs.importNpmLock.npmConfigHook;
            env.VITE_GIT_SHA = rev;
            installPhase = ''
              runHook preInstall
              cp -r dist $out
              runHook postInstall
            '';
          };

          # The Rust server on its own.
          backend = pkgs.rustPlatform.buildRustPackage {
            pname = "cattracker-backend";
            inherit version;
            src = fs.toSource {
              root = ./backend;
              fileset = fs.unions [
                ./backend/Cargo.toml
                ./backend/Cargo.lock
                ./backend/src
                ./backend/migrations
              ];
            };
            cargoLock.lockFile = ./backend/Cargo.lock;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.sqlite ];
            meta.mainProgram = "cattracker";
          };

          # Server + PWA: `cattracker serve` finds the UI without CT_STATIC_DIR.
          cattracker =
            pkgs.runCommand "cattracker-${version}"
              {
                nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
                meta.mainProgram = "cattracker";
              }
              ''
                makeBinaryWrapper ${lib.getExe backend} $out/bin/cattracker \
                  --set-default CT_STATIC_DIR ${frontend} \
                  --set-default CT_GIT_SHA ${rev}
              '';

          # Production image: `nix build .#docker && docker load < result`
          docker = pkgs.dockerTools.buildLayeredImage {
            name = "cattracker";
            tag = "latest";
            contents = [ cattracker ];
            # The server runs as uid 10001 and needs to write the database to /data.
            fakeRootCommands = ''
              mkdir -p data
              chown 10001:10001 data
            '';
            config = {
              Cmd = [
                "cattracker"
                "serve"
              ];
              User = "10001:10001";
              WorkingDir = "/data";
              Env = [
                "PATH=/bin"
                "CT_DB_PATH=/data/cattracker.db"
                "CT_BIND=0.0.0.0:8080"
                "CT_COOKIE_SECURE=true"
                "RUST_LOG=cattracker=info,tower_http=warn"
              ];
              ExposedPorts."8080/tcp" = { };
              Volumes."/data" = { };
            };
          };
        in
        {
          inherit
            frontend
            backend
            cattracker
            docker
            ;
          default = cattracker;
        }
      );

      # One shell for both halves: `nix develop`, then `just dev`.
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            rust-analyzer
            cargo-watch
            just
            cloudflared # `just phone-install`: temporary HTTPS URL
            nodejs
            sqlite # CLI, for poking at the dev database
            pkg-config
          ];
          buildInputs = [ pkgs.sqlite ]; # sqlx links against libsqlite3
          env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
