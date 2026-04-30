{
  description = "seetui — TUI log viewer for systemd journals with Vi motions";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let
        pkgs = import nixpkgs { inherit system; };

        buildInputs = with pkgs; [
          systemd        # libsystemd.so — journald C library
          xorg.libxcb    # arboard clipboard (X11 backend)
          xorg.libX11
          xorg.libXext
        ];

        nativeBuildInputs = with pkgs; [
          pkg-config     # lets libsystemd-sys build script find libsystemd
        ];

        seetui = pkgs.rustPlatform.buildRustPackage {
          pname   = "seetui";
          version = "0.1.7";

          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          inherit buildInputs nativeBuildInputs;

          PKG_CONFIG_PATH = "${pkgs.systemd.dev}/lib/pkgconfig";

          meta = with pkgs.lib; {
            description = "TUI based tool to lookup logs from services (systemd) with Vi motions";
            homepage    = "https://github.com/NustyFrozen/SEE";
            license     = licenses.agpl3Only;
            platforms   = [ "x86_64-linux" ];
          };
        };

      in {
        packages = {
          default = seetui;
          seetui  = seetui;
        };

        apps.default = flake-utils.lib.mkApp {
          drv  = seetui;
          name = "seetui";
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

          packages = with pkgs; [
            rustc
            cargo
            rust-analyzer
            clippy
            rustfmt
          ];

          PKG_CONFIG_PATH = "${pkgs.systemd.dev}/lib/pkgconfig";

          shellHook = ''
            echo "seetui dev shell — $(rustc --version)"
          '';
        };
      }
    );
}
