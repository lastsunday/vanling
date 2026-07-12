{
  description = "chobits monorepo development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/3e41b24abd260e8f71dbe2f5737d24122f972158";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Cross-compilation for musl targets (x86_64-unknown-linux-musl)
        pkgsMusl = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          crossSystem = nixpkgs.lib.systems.examples.musl64;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        nodeMajor = builtins.head (builtins.match "([0-9]+)\\..*" (
          pkgs.lib.trim (builtins.readFile ./.node-version)
        ));
        nodejs = pkgs."nodejs_${nodeMajor}";

        # moonrepo CLI v2 — prebuilt binary from GitHub releases
        moonVersion = "2.4.2";
        moonTarget = {
          x86_64-linux   = "x86_64-unknown-linux-gnu";
          aarch64-linux  = "aarch64-unknown-linux-gnu";
          x86_64-darwin  = "x86_64-apple-darwin";
          aarch64-darwin = "aarch64-apple-darwin";
        }.${system};
        moonSha256 = {
          x86_64-linux   = "5603438fbf14be0515f27f0756d99e4a806097993e21467f9daf9b509cdf8a8d";
          aarch64-linux  = "7f922174533817a553c8c0becb0fc1fcc45f400104824af42bc0da525c4d06af";
          x86_64-darwin  = "5cb29e1e8ddde538aeb9a77c18cb3b9ace991fe7d2307ff8a8badc34c7be9de8";
          aarch64-darwin = "dcefe53256ca65c91259b6e6bc570e18231020653d421cc70680f7868250c1bf";
        }.${system};
        moon = pkgs.stdenv.mkDerivation {
          name = "moon-${moonVersion}";
          src = pkgs.fetchurl {
            url = "https://github.com/moonrepo/moon/releases/download/v${moonVersion}/moon_cli-${moonTarget}.tar.xz";
            sha256 = moonSha256;
          };
          sourceRoot = "moon_cli-${moonTarget}";
          installPhase = ''
            install -m755 -D moon "$out/bin/moon"
            install -m755 -D moonx "$out/bin/moonx"
          '';
          meta.mainProgram = "moon";
        };
      in {
        packages.moon = moon;

        devShells = {
          default = pkgs.mkShell {
            packages = with pkgs; [
              git
              rustToolchain
              nodejs
              pnpm
              just
              pkg-config
              moon
              zola
              git-cliff
              lefthook
              protobuf
              sccache
              flutter
              jdk17
              cmake
              ninja
            ];

            buildInputs = with pkgs; [
              openssl
              sqlite
              postgresql_16
              openblas
              libopus.dev
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];

            shellHook = ''
              # macOS native xcrun wrapper — override Nix xcbuild's xcrun
              # which breaks Flutter's codesign invocation.
              mkdir -p /tmp/nix-xcrun-wrapper
              printf '%s\n' '#!/bin/sh' 'unset DEVELOPER_DIR SDKROOT' 'exec /usr/bin/xcrun "$@"' > /tmp/nix-xcrun-wrapper/xcrun
              chmod +x /tmp/nix-xcrun-wrapper/xcrun
              export PATH="/tmp/nix-xcrun-wrapper:$PATH"

              # Ensure PUB_CACHE is writable (HOME may be missing in CI)
              if [ -z "$PUB_CACHE" ]; then
                if [ -n "$HOME" ]; then
                  export PUB_CACHE="$HOME/.pub-cache"
                else
                  export PUB_CACHE="/tmp/.pub-cache"
                fi
              fi

              export MOON_TOOLCHAIN_FORCE_GLOBALS=true

              echo "✦ chobits devShell (${system})"
              echo "  Rust: $(rustc --version)"
              echo "  Moon: $(moon --version)"
              echo "  Node: $(node --version)"
              echo "  pnpm: $(pnpm --version)"
              echo "  sccache: $(sccache --version | head -1)"
              echo ""
              echo "  Run: moon run <task>"
              export CARGO_BUILD_RUSTC_WRAPPER=sccache
            '';
          };

          server = pkgs.mkShell {
            packages = with pkgs; [
              git
              rustToolchain
              pkg-config
              protobuf
              sccache
            ];
            buildInputs = with pkgs; [
              openssl
              sqlite
              postgresql_16
              openblas
              libopus.dev
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];
            shellHook = ''
              export CARGO_BUILD_RUSTC_WRAPPER=sccache
            '';
          };

          frontend = pkgs.mkShell {
            packages = with pkgs; [
              git
              nodejs
              pnpm
            ];
          };

          # Cross-compilation shell for x86_64-unknown-linux-musl
          musl64 = pkgsMusl.mkShell {
            packages = [
              # Use host Rust toolchain (supports cross-compilation via targets)
              rustToolchain
              pkgsMusl.cmake
              pkgsMusl.pkg-config
              pkgsMusl.ninja
            ];

            buildInputs = [];

            shellHook = ''
              export CARGO_BUILD_TARGET=x86_64-unknown-linux-musl
              export CC_x86_64_unknown_linux_musl="${pkgsMusl.stdenv.cc}/bin/${pkgsMusl.stdenv.cc.targetPrefix}cc"
              export CXX_x86_64_unknown_linux_musl="${pkgsMusl.stdenv.cc}/bin/${pkgsMusl.stdenv.cc.targetPrefix}g++"
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="${pkgsMusl.stdenv.cc}/bin/${pkgsMusl.stdenv.cc.targetPrefix}cc"
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static"

              echo "✦ chobits musl64 cross-compilation shell"
              echo "  Target: x86_64-unknown-linux-musl"
              echo "  CC: $CC_x86_64_unknown_linux_musl"
              echo "  CXX: $CXX_x86_64_unknown_linux_musl"
            '';
          };

          # Cross-compilation shell for aarch64-unknown-linux-musl
          musl64-arm64 = let
            pkgsMuslArm64 = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
              crossSystem = nixpkgs.lib.systems.examples.aarch64-multiplatform-musl;
            };
          in pkgsMuslArm64.mkShell {
            packages = [
              rustToolchain
              pkgsMuslArm64.cmake
              pkgsMuslArm64.pkg-config
              pkgsMuslArm64.ninja
            ];

            buildInputs = [];

            shellHook = ''
              export CARGO_BUILD_TARGET=aarch64-unknown-linux-musl
              export CC_aarch64_unknown_linux_musl="${pkgsMuslArm64.stdenv.cc}/bin/${pkgsMuslArm64.stdenv.cc.targetPrefix}cc"
              export CXX_aarch64_unknown_linux_musl="${pkgsMuslArm64.stdenv.cc}/bin/${pkgsMuslArm64.stdenv.cc.targetPrefix}g++"
              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${pkgsMuslArm64.stdenv.cc}/bin/${pkgsMuslArm64.stdenv.cc.targetPrefix}cc"
              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static"
              export CRATE_CC_NO_DEFAULTS=1

              echo "✦ chobits musl64-arm64 cross-compilation shell"
              echo "  Target: aarch64-unknown-linux-musl"
              echo "  CC: $CC_aarch64_unknown_linux_musl"
              echo "  CXX: $CXX_aarch64_unknown_linux_musl"
            '';
          };
        };
      });
}
