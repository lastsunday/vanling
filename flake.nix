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
        # sherpa-onnx prebuilt static libraries (must match Cargo.lock version)
        sherpaOnnxVersion = "1.13.4";
        sherpaOnnxUrlBase = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v${sherpaOnnxVersion}";

        # Host platform sherpa-onnx prebuilt static library
        sherpaOnnxLib = let
          archiveName = {
            x86_64-linux   = "sherpa-onnx-v${sherpaOnnxVersion}-linux-x64-static-lib.tar.bz2";
            aarch64-linux  = "sherpa-onnx-v${sherpaOnnxVersion}-linux-aarch64-static-lib.tar.bz2";
            x86_64-darwin  = "sherpa-onnx-v${sherpaOnnxVersion}-osx-x64-static-lib.tar.bz2";
            aarch64-darwin = "sherpa-onnx-v${sherpaOnnxVersion}-osx-arm64-static-lib.tar.bz2";
          }.${system};
          hash = {
            x86_64-linux   = "sha256-bGDnnCS3JQoltQ7FDi4k6ozt20B3Xasu54/UIExSKjQ=";
            aarch64-linux  = "sha256-PQiDQxMuFU31NIzqaDIiAstfbB/OpPAxg7mlyvoERu0=";
            x86_64-darwin  = "sha256-RXSXvLMxdACgZZRpHY8oNb/H6cJn3izY19+jfJt7AOA=";
            aarch64-darwin = "sha256-n9foeCcAesFJT1RcCgW4a3xFGaSy3Qw+/6CVcVeiiD0=";
          }.${system};
        in pkgs.fetchzip {
          url = "${sherpaOnnxUrlBase}/${archiveName}";
          inherit hash;
          stripRoot = true;
        };

        # aarch64-linux sherpa-onnx prebuilt static library (for cross-compilation)
        sherpaOnnxLibArm64 = pkgs.fetchzip {
          url = "${sherpaOnnxUrlBase}/sherpa-onnx-v${sherpaOnnxVersion}-linux-aarch64-static-lib.tar.bz2";
          hash = "sha256-PQiDQxMuFU31NIzqaDIiAstfbB/OpPAxg7mlyvoERu0=";
          stripRoot = true;
        };

        # Cross-compilation nixpkgs for aarch64
        pkgsArm64 = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          crossSystem = nixpkgs.lib.systems.examples.aarch64-multiplatform;
        };

        chobits-server-arm64 = pkgsArm64.rustPlatform.buildRustPackage {
          pname = "chobits-server";
          version = "dev";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildOptions = [ "--package" "chobits-server" "--bin" "chobits-server" ];
          SHERPA_ONNX_LIB_DIR = "${sherpaOnnxLibArm64}/lib";
          nativeBuildInputs = [
            pkgsArm64.pkg-config
          ];
          buildInputs = with pkgsArm64; [
            openssl
          ];
          # Cross-compilation environment variables
          CARGO_BUILD_TARGET = "aarch64-unknown-linux-gnu";
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
          CC_aarch64_unknown_linux_gnu = "${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc";
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER = "${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc";
          doCheck = false;
        };

      in {
        packages = let
          chobits-server = pkgs.rustPlatform.buildRustPackage {
            pname = "chobits-server";
            version = "dev";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildOptions = [ "--package" "chobits-server" "--bin" "chobits-server" ];
            SHERPA_ONNX_LIB_DIR = "${sherpaOnnxLib}/lib";
            nativeBuildInputs = [
              pkgs.pkg-config
            ];
            buildInputs = with pkgs; [
              openssl
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];
            doCheck = false;
          };

          docker-amd64 = pkgs.dockerTools.buildImage {
            name = "chobits";
            tag = "dev-amd64";
            copyToRoot = [ chobits-server ];
            config = {
              Cmd = [ "/bin/chobits-server" ];
              WorkingDir = "/app";
              ExposedPorts = { "3000/tcp" = {}; };
            };
          };

          docker-arm64 = pkgsArm64.dockerTools.buildImage {
            name = "chobits";
            tag = "dev-arm64";
            architecture = "arm64";
            copyToRoot = [ chobits-server-arm64 ];
            config = {
              Cmd = [ "/bin/chobits-server" ];
              WorkingDir = "/app";
              ExposedPorts = { "3000/tcp" = {}; };
            };
          };
        in {
          inherit moon chobits-server chobits-server-arm64 docker-amd64 docker-arm64;
        };

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

          # Local-only cross-compilation shell for x86_64-unknown-linux-gnu (static)
          # CI uses `nix build .#chobits-server` instead
          gnu64 = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.cmake
              pkgs.pkg-config
              pkgs.ninja
              moon
              pkgs.sccache
            ];

            buildInputs = [
              pkgs.openssl
            ];

            shellHook = ''
              export CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu
              export CC_x86_64_unknown_linux_gnu="${pkgs.stdenv.cc}/bin/cc"
              export CXX_x86_64_unknown_linux_gnu="${pkgs.stdenv.cc}/bin/g++"
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${pkgs.stdenv.cc}/bin/cc"
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-feature=+crt-static -L ${if pkgs.stdenv.isLinux then "${pkgs.glibc.static}/lib" else ""}"

              export CARGO_BUILD_RUSTC_WRAPPER=sccache
              echo "✦ chobits gnu64 static-compilation shell"
              echo "  Target: x86_64-unknown-linux-gnu"
              echo "  CC: $CC_x86_64_unknown_linux_gnu"
              echo "  CXX: $CXX_x86_64_unknown_linux_gnu"
            '';
          };

          # Local-only cross-compilation shell for aarch64-unknown-linux-gnu (static)
          # CI uses `nix build .#chobits-server-arm64` instead
          gnu64-arm64 = let
            pkgsArm64 = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
              crossSystem = nixpkgs.lib.systems.examples.aarch64-multiplatform;
            };
          in pkgsArm64.mkShell {
            packages = [
              rustToolchain
              pkgsArm64.cmake
              pkgsArm64.pkg-config
              pkgsArm64.ninja
              moon
              pkgs.sccache
            ];

            buildInputs = [
              pkgsArm64.openssl
            ];

            shellHook = ''
              export CARGO_BUILD_TARGET=aarch64-unknown-linux-gnu
              export HOST_CC="${pkgs.stdenv.cc}/bin/cc"
              export CC_aarch64_unknown_linux_gnu="${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc"
              export CXX_aarch64_unknown_linux_gnu="${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}g++"
              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc"
              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-feature=+crt-static -L ${if pkgsArm64.stdenv.isLinux then "${pkgsArm64.glibc.static}/lib -L ${pkgsArm64.stdenv.cc.cc.lib}/lib" else ""}"

              export CARGO_BUILD_RUSTC_WRAPPER=sccache
              echo "✦ chobits gnu64-arm64 cross-compilation shell"
              echo "  Target: aarch64-unknown-linux-gnu"
              echo "  CC: $CC_aarch64_unknown_linux_gnu"
              echo "  CXX: $CXX_aarch64_unknown_linux_gnu"
            '';
          };
        };
      });
}
