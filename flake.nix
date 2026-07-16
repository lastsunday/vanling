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
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        nodeMajor = builtins.head (builtins.match "([0-9]+)\\..*" (
          pkgs.lib.trim (builtins.readFile ./.node-version)
        ));
        nodejs = pkgs."nodejs_${nodeMajor}";

        # moonrepo CLI v2 — prebuilt binary from GitHub releases
        moonVersion = "2.4.3";
        moonTarget = {
          x86_64-linux   = "x86_64-unknown-linux-gnu";
          aarch64-linux  = "aarch64-unknown-linux-gnu";
          x86_64-darwin  = "x86_64-apple-darwin";
          aarch64-darwin = "aarch64-apple-darwin";
        }.${system};
        moonSha256 = {
          x86_64-linux   = "368fb8ca4307cab5a0bf55013e4a3fa92e58a35934d0bdd92413e7bb49facbb6";
          aarch64-linux  = "94d8a30c31a127ceb471c295294b77b2565ce90eabb6b6728db782864451ab70";
          x86_64-darwin  = "60148eb5ee8cf8fa596852f083390ad52518f08b6ddcc4348afb85219a3d4901";
          aarch64-darwin = "82ebc8c54ff4f75a3c78068e872cefe29284cad5f05a27ca778a4a714df87f7b";
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

        # Android SDK for Flutter builds (declarative, managed by Nix)
        androidComposition = pkgs.androidenv.composeAndroidPackages {
          buildToolsVersions = [ "36.0.0" ];
          platformVersions = [ "34" "35" "36" ];
          cmakeVersions = [ "3.22.1" ];
          abiVersions = [ "armeabi-v7a" "arm64-v8a" ];
          includeNDK = true;
          ndkVersions = [ "28.2.13676358" ];
          includeEmulator = false;
          includeSystemImages = false;
        };
        androidSdk = androidComposition.androidsdk;

        # Cross-compilation nixpkgs for aarch64
        pkgsArm64 = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          crossSystem = nixpkgs.lib.systems.examples.aarch64-multiplatform;
        };

        # Crane library for workspace builds with custom toolchain
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

        # Flutter Linux desktop deps — genericClosure transitive closure (same as nixpkgs wrapper.nix)
        appRuntimeDeps = pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.atk pkgs.cairo pkgs.gdk-pixbuf pkgs.glib pkgs.gtk3
          pkgs.harfbuzz pkgs.libepoxy pkgs.pango pkgs.libx11 pkgs.libdeflate
        ];
        appBuildDeps = pkgs.lib.optionals pkgs.stdenv.isLinux (
          let
            deps = pkg: pkgs.lib.filter pkgs.lib.isDerivation ((pkg.buildInputs or []) ++ (pkg.propagatedBuildInputs or []));
            withKey = pkg: { key = pkg.outPath; val = pkg; };
            collect = pkg: pkgs.lib.map withKey ([ pkg ] ++ deps pkg);
          in
          pkgs.lib.map (e: e.val) (
            pkgs.lib.genericClosure {
              startSet = pkgs.lib.map withKey appRuntimeDeps;
              operator = item: collect item.val;
            }
          )
        );
        flutterPkgConfigPath = pkgs.lib.optionalString pkgs.stdenv.isLinux (
          pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" appBuildDeps
          + pkgs.lib.makeSearchPathOutput "dev" "share/pkgconfig" appBuildDeps
        );
        flutterIncludeFlags = pkgs.lib.optionalString pkgs.stdenv.isLinux (
          pkgs.lib.concatMapStringsSep " " (pkg: "-isystem ${pkgs.lib.getOutput "dev" pkg}/include") appBuildDeps
        );
        flutterLibraryPath = pkgs.lib.optionalString pkgs.stdenv.isLinux (
          pkgs.lib.makeSearchPath "lib" appRuntimeDeps
        );

        # Source filtering for Rust workspace (improves cache hit rates)
        # Uses fileset to include downloader manifest JSON files embedded via include_dir!
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            (pkgs.lib.fileset.fileFilter (file: file.hasExt "json") ./apps/server/src/downloader/manifests)
          ];
        };

        # Common arguments shared across all crane builds
        commonCraneArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [
            pkgs.openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];

          SHERPA_ONNX_LIB_DIR = "${sherpaOnnxLib}/lib";
          doCheck = false;
        };

        # Build workspace dependencies once for reuse across packages
        cargoArtifacts = craneLib.buildDepsOnly (commonCraneArgs // {
          pname = "chobits-workspace-deps";
        });

        # Build chobits-server binary
        chobits-server = craneLib.buildPackage (commonCraneArgs // {
          inherit cargoArtifacts;
          pname = "chobits-server";
          version = "dev";
          cargoBuildFlags = [ "--package" "chobits-server" "--bin" "chobits-server" ];
        });

        # Crane library for arm64 cross-compilation
        craneLibArm64 = (crane.mkLib pkgsArm64).overrideToolchain (_: rustToolchain);

        # Common arguments for arm64 cross-compilation
        commonCraneArgsArm64 = {
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              (craneLibArm64.fileset.commonCargoSources ./.)
              (pkgs.lib.fileset.fileFilter (file: file.hasExt "json") ./apps/server/src/downloader/manifests)
            ];
          };
          strictDeps = true;

          nativeBuildInputs = [
            pkgsArm64.pkg-config
          ];

          buildInputs = [
            pkgsArm64.openssl
          ];

          SHERPA_ONNX_LIB_DIR = "${sherpaOnnxLibArm64}/lib";

          # Cross-compilation environment variables
          CARGO_BUILD_TARGET = "aarch64-unknown-linux-gnu";
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
          CC_aarch64_unknown_linux_gnu = "${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc";
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER = "${pkgsArm64.stdenv.cc}/bin/${pkgsArm64.stdenv.cc.targetPrefix}gcc";

          doCheck = false;
        };

        # Build arm64 dependencies once for reuse
        cargoArtifactsArm64 = craneLibArm64.buildDepsOnly (commonCraneArgsArm64 // {
          pname = "chobits-workspace-deps-arm64";
        });

        # Build arm64 binary
        chobits-server-arm64 = craneLibArm64.buildPackage (commonCraneArgsArm64 // {
          inherit cargoArtifactsArm64;
          pname = "chobits-server";
          version = "dev";
          cargoBuildFlags = [ "--package" "chobits-server" "--bin" "chobits-server" ];
        });

      in {
        packages = let
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
              fvm
              curl
              unzip
              # Flutter build dependencies
              jdk17
              cmake
              ninja
              # Linux desktop (Flutter official requirements)
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              clang
              # GTK3 + transitive deps auto-computed via genericClosure in shellHook
            ] ++ [
              # Android SDK
              androidSdk
            ];

            buildInputs = with pkgs; [
              openssl
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];

            ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
            ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
            JAVA_HOME = "${pkgs.jdk17}";
            GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/36.0.0/aapt2";

            shellHook = ''
              # macOS native xcrun wrapper — override Nix xcbuild's xcrun
              # which breaks Flutter's codesign invocation.
              if [ "$(uname)" = "Darwin" ]; then
                mkdir -p /tmp/nix-xcrun-wrapper
                printf '%s\n' '#!/bin/sh' 'unset DEVELOPER_DIR SDKROOT' 'exec /usr/bin/xcrun "$@"' > /tmp/nix-xcrun-wrapper/xcrun
                chmod +x /tmp/nix-xcrun-wrapper/xcrun
                export PATH="/tmp/nix-xcrun-wrapper:$PATH"
              fi

              # Expose external Android SDK tools
              export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"

              # Ensure PUB_CACHE is writable (HOME may be missing in CI)
              if [ -z "$PUB_CACHE" ]; then
                if [ -n "$HOME" ]; then
                  export PUB_CACHE="$HOME/.pub-cache"
                else
                  export PUB_CACHE="/tmp/.pub-cache"
                fi
              fi

              # Ensure HOME is set (CI --ignore-env may strip it; FVM needs it)
              if [ -z "$HOME" ]; then
                export HOME="/tmp"
              fi

              # Auto-install Flutter version pinned in .fvmrc
              if [ -f .fvmrc ] && command -v fvm &>/dev/null; then
                echo "  [fvm] pwd=$(pwd) HOME=$HOME"
                echo "  [fvm] .fvmrc: $(cat .fvmrc)"
                echo "  [fvm] .fvm/ exists: $([ -d .fvm ] && echo yes || echo no)"
                if [ -d .fvm ]; then
                  echo "  [fvm] ls .fvm/:"
                  ls -la .fvm/ 2>&1 | sed 's/^/    /'
                fi
                echo "  [fvm] Running: fvm install"
                fvm install
                echo "  [fvm] Running: fvm use --force"
                fvm use --force
                echo "  [fvm] After fvm use, .fvm/ contents:"
                ls -la .fvm/ 2>&1 | sed 's/^/    /'
                if [ -L .fvm/flutter_sdk ]; then
                  echo "  [fvm] .fvm/flutter_sdk symlink -> $(readlink .fvm/flutter_sdk)"
                  echo "  [fvm] readlink -f = $(readlink -f .fvm/flutter_sdk 2>/dev/null || echo 'failed')"
                else
                  echo "  [fvm] .fvm/flutter_sdk is NOT a symlink (missing or regular dir)"
                fi
              fi

              if [ -f .fvmrc ] && [ -L .fvm/flutter_sdk ] && [ -d .fvm/flutter_sdk ]; then
                export PATH="$(pwd)/.fvm/flutter_sdk/bin:$PATH"
                if [ -x "$(pwd)/.fvm/flutter_sdk/bin/flutter" ]; then
                  echo "  [fvm] Flutter SDK: $(readlink -f .fvm/flutter_sdk 2>/dev/null || readlink .fvm/flutter_sdk)"
                else
                  echo "  [fvm] WARNING: .fvm/flutter_sdk/bin/flutter exists but not executable"
                fi
              else
                echo "  [fvm] WARNING: .fvm/flutter_sdk symlink not valid"
              fi

              if [ -f .fvmrc ] && command -v fvm &>/dev/null; then
                flutter precache --dart 2>/dev/null || true
              fi

              # Clear stale Flutter SDK config that overrides ANDROID_HOME
              if [ -n "$HOME" ]; then
                FLUTTER_SETTINGS="$HOME/.config/flutter/settings"
                if [ -f "$FLUTTER_SETTINGS" ] && grep -q "android-sdk" "$FLUTTER_SETTINGS" 2>/dev/null; then
                  rm -f "$FLUTTER_SETTINGS"
                fi
              fi

              export MOON_TOOLCHAIN_FORCE_GLOBALS=true

              # Flutter Linux desktop — auto-computed transitive closure (genericClosure)
              if [ "$(uname)" = "Linux" ] && [ -n "${flutterPkgConfigPath}" ]; then
                export PKG_CONFIG_PATH="${flutterPkgConfigPath}:''${PKG_CONFIG_PATH:-}"
                export CFLAGS="${flutterIncludeFlags} ''${CFLAGS:-}"
                export CXXFLAGS="$CFLAGS"
                export LIBRARY_PATH="${flutterLibraryPath}:''${LIBRARY_PATH:-}"
              fi

              echo "✦ chobits devShell (${system})"
              echo "  Rust:    $(rustc --version)"
              echo "  Moon:    $(moon --version)"
              echo "  Node:    $(node --version)"
              echo "  pnpm:   $(pnpm --version)"
              echo "  Flutter: $(flutter --version 2>/dev/null | head -1 || echo 'run fvm install')"
              echo "  sccache: $(sccache --version | head -1)"
              echo ""
              echo "  Run: moon run <task>"
              export CARGO_BUILD_RUSTC_WRAPPER=sccache
            '';
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
