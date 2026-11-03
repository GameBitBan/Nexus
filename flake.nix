{
  description = "Nexus — minimal black & white local lossless music player";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      py = pkgs.python3.withPackages (ps: [ ps.pyside6 ps.mutagen ps.numpy ps.av ps.sounddevice ]
        ++ pkgs.lib.optional (ps ? aioslsk) ps.aioslsk);   # built-in soulseek, when nixpkgs has it
    in {
      packages.${system}.default = pkgs.stdenv.mkDerivation {
        pname = "nexus";
        version = "1.12.6";
        src = ./.;
        nativeBuildInputs = [ pkgs.qt6.wrapQtAppsHook pkgs.makeWrapper ];
        buildInputs = [ pkgs.qt6.qtbase pkgs.qt6.qtwayland pkgs.qt6.qtmultimedia ];
        dontWrapQtApps = true;
        installPhase = ''
          mkdir -p $out/share/nexus $out/share/applications $out/bin
          cp -r nexus.py fonts icons skins $out/share/nexus/
          cp nexus.desktop $out/share/applications/
          for s in 16 24 32 48 64 128 256 512; do
            install -Dm644 icons/nexus-$s.png $out/share/icons/hicolor/''${s}x''${s}/apps/nexus.png
          done
        '';
        postFixup = ''
          makeWrapper ${py}/bin/python3 $out/bin/nexus \
            --add-flags $out/share/nexus/nexus.py \
            "''${qtWrapperArgs[@]}"
        '';
      };

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/nexus";
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = [ py pkgs.qt6.qtwayland ];
      };
    };
}
