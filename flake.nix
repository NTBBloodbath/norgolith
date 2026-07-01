{
  description = "Norg static site generator and plugin SDK";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {inherit system;};
        toolchain = pkgs.rustPlatform;
        corePackage = (pkgs.lib.importTOML "${self}/core/Cargo.toml").package;
        sdkPackage = (pkgs.lib.importTOML "${self}/sdk/Cargo.toml").package;
      in rec {
        # nix build
        packages.default = toolchain.buildRustPackage {
          pname = corePackage.name;
          version = corePackage.version;
          src = pkgs.lib.cleanSource "${self}";
          cargoLock = {
            lockFile = "${self}/Cargo.lock";
            allowBuiltinFetchGit = true;
          };
          useNextest = true;
          dontUseCargoParallelTests = true;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            libgit2
            openssl
            zlib
          ];

          env = {
            LIBGIT2_NO_VENDOR = true;
            OPENSSL_NO_VENDOR = true;
          };

          __darwinAllowLocalNetworking = true;

          meta = {
            description = corePackage.description;
            homepage = corePackage.repository;
            license = pkgs.lib.licenses.gpl2Only;
            maintainers = corePackage.authors;
          };

          # For other makeRustPlatform features see:
          # https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md#cargo-features-cargo-features
        };

        packages.norgolith-plugin-sdk = toolchain.buildRustPackage {
          pname = sdkPackage.name;
          version = sdkPackage.version;
          src = pkgs.lib.cleanSource "${self}";
          cargoLock = {
            lockFile = "${self}/Cargo.lock";
            allowBuiltinFetchGit = true;
          };
          buildNoDefaultFeatures = true;
          cargoBuildFlags = ["-p" "norgolith-plugin-sdk"];

          meta = {
            description = sdkPackage.description;
            homepage = sdkPackage.repository;
            license = pkgs.lib.licenses.gpl2Only;
            maintainers = sdkPackage.authors;
          };
        };

        # nix run
        apps.default = flake-utils.lib.mkApp {drv = packages.default;};

        # nix develop
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            (with toolchain; [
              cargo
              rustc
              rustLibSrc
            ])
            clippy
            rustfmt
            cargo-edit
            cargo-nextest
            rust-analyzer
            pkg-config # Required by git2 crate
            openssl # Required by git2 crate

            # Documentation site dev tools
            tailwindcss_4
            mprocs
            watchman
            tailwindcss-language-server
          ];

          # Many editors rely on this rust-src PATH variable
          RUST_SRC_PATH = "${toolchain.rustLibSrc}";

          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
        };
      }
    );

  nixConfig = {
    extra-substituters = ["https://ntbbloodbath.cachix.org"];
    extra-trusted-public-keys = [
      "ntbbloodbath.cachix.org-1:L4DjjGwDB6O3BJ4SmtYTZbvWKLi+1v/hRlLWKOtq+f0="
    ];
  };
}
