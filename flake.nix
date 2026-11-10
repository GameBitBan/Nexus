{
  description = "Nexus — local lossless music player";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # libraries the window opens at run time (winit / glow load these with dlopen)
      runtimeLibs = pkgs: with pkgs; [
        libGL
        libxkbcommon
        wayland
        xorg.libX11
        xorg.libXcursor
        xorg.libXi
        xorg.libXrandr
        vulkan-loader
      ];

      # Nexus 2 — the Rust version (this branch)
      nexus2 = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "nexus";
        version = "2.0.0-dev";
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let base = baseNameOf path; in
            !(type == "directory" && (base == "target" || base == ".git" || base == "dist"));
        };
        cargoRoot = "nexus-rs";
        buildAndTestSubdir = "nexus-rs";
        cargoLock.lockFile = ./nexus-rs/Cargo.lock;
        cargoBuildFlags = [ "-p" "nexus" ];
        cargoTestFlags = [ "-p" "nexus-core" ];

        nativeBuildInputs = with pkgs; [ pkg-config rustPlatform.bindgenHook makeWrapper ];
        buildInputs = with pkgs; [ ffmpeg alsa-lib ] ++ runtimeLibs pkgs;

        postInstall = ''
          mkdir -p $out/share/nexus $out/share/applications
          cp -r fonts icons skins $out/share/nexus/
          install -Dm644 nexus.desktop $out/share/applications/nexus.desktop
          for s in 16 24 32 48 64 128 256 512; do
            install -Dm644 icons/nexus-$s.png $out/share/icons/hicolor/''${s}x''${s}/apps/nexus.png
          done
          wrapProgram $out/bin/nexus \
            --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}
        '';

        # the packaged program must find its fonts, skins and codecs
        doInstallCheck = true;
        installCheckPhase = ''
          HOME=$TMPDIR NEXUS_HOME=$TMPDIR/nexus $out/bin/nexus --selftest $TMPDIR/selftest.txt
        '';

        meta = {
          description = "Local lossless music player — text-style UI, skins, Soulseek, listen together";
          homepage = "https://github.com/GameBitBan/Nexus";
          license = pkgs.lib.licenses.mit;
          mainProgram = "nexus";
          platforms = systems;
        };
      };

      # Nexus 1 — the Python version, kept until Nexus 2 has everything
      nexus1 = pkgs:
        let
          py = pkgs.python3.withPackages (ps: [ ps.pyside6 ps.mutagen ps.numpy ps.av ps.sounddevice ]
            ++ pkgs.lib.optional (ps ? aioslsk) ps.aioslsk
            ++ pkgs.lib.optional (ps ? aiortc) ps.aiortc);
        in pkgs.stdenv.mkDerivation {
          pname = "nexus1";
          version = "1";
          src = ./.;
          nativeBuildInputs = [ pkgs.qt6.wrapQtAppsHook pkgs.makeWrapper ];
          buildInputs = [ pkgs.qt6.qtbase pkgs.qt6.qtwayland pkgs.qt6.qtmultimedia ];
          dontWrapQtApps = true;
          installPhase = ''
            mkdir -p $out/share/nexus1 $out/bin
            cp -r nexus.py fonts icons skins $out/share/nexus1/
          '';
          postFixup = ''
            makeWrapper ${py}/bin/python3 $out/bin/nexus1 \
              --add-flags $out/share/nexus1/nexus.py \
              "''${qtWrapperArgs[@]}"
          '';
        };
    in {
      packages = forAll (pkgs: {
        default = nexus2 pkgs;
        nexus = nexus2 pkgs;
        nexus1 = nexus1 pkgs;
      });

      apps = forAll (pkgs: {
        default = { type = "app"; program = "${nexus2 pkgs}/bin/nexus"; };
        nexus1 = { type = "app"; program = "${nexus1 pkgs}/bin/nexus1"; };
      });

      overlays.default = final: _prev: { nexus = nexus2 final; };

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ (nexus2 pkgs) ];
          packages = with pkgs; [ cargo rustc rustfmt clippy rust-analyzer ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);
        };
      });
    };
}
