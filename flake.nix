{
  description = "KeystrokeNoise — mechanical keyboard/mouse sounds without keylogging";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = manifest.name;
          version = manifest.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.alsa-lib ];
          postInstall = ''
            install -Dm644 packaging/keystroke-noise.service \
              $out/lib/systemd/user/keystroke-noise.service
            mkdir -p $out/share/keystroke-noise/sounds
            cp -r assets/*.wav $out/share/keystroke-noise/sounds/ 2>/dev/null || true
            # Prefer flat category WAVs; also keep buckle/mouse trees if present
            if [ -d assets/buckle ]; then
              mkdir -p $out/share/keystroke-noise/sounds/buckle
              cp -r assets/buckle/*.wav $out/share/keystroke-noise/sounds/buckle/ 2>/dev/null || true
            fi
            if [ -d assets/mouse ]; then
              mkdir -p $out/share/keystroke-noise/sounds/mouse
              cp -r assets/mouse/*.wav $out/share/keystroke-noise/sounds/mouse/ 2>/dev/null || true
            fi
          '';
          meta = with pkgs.lib; {
            description = manifest.description;
            homepage = "https://github.com/r3dg0d/KeystrokeNoise";
            license = licenses.mit;
            mainProgram = "keystroke-noise";
            platforms = platforms.linux;
          };
        };

        packages.keystroke-noise = self.packages.${system}.default;

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/keystroke-noise";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc cargo rustfmt clippy rust-analyzer pkg-config alsa-lib
          ];
        };
      });
}
