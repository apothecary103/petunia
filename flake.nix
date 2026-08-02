{
  description = "Petunia development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, fenix, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
    in
    {
      devShells = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system};

          rustToolchain = fenixPkgs.stable.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
            "rustfmt"
          ];

          onLinux = pkgs.stdenv.hostPlatform.isLinux;

          # What gpui, cpal and the crypto need from the system on Linux.
          #
          # macOS gets every one of these from frameworks that are simply there,
          # which is why the shell needed none of it and why the Linux build
          # failed on whichever one the linker reached first -- most visibly
          # `openssl`, since that is the one with a name anybody recognises. It
          # is not a *version* mismatch: the shell was handing the build no
          # system libraries at all, and the toolchain then found whatever the
          # host distribution happened to have, or nothing.
          linuxLibraries = with pkgs; [
            # `libsignal-net` reaches Signal's fork of BoringSSL, which builds
            # itself -- but the crates around it still ask pkg-config for a
            # system TLS, and `oo7` reaches the Secret Service over D-Bus.
            openssl
            dbus
            # gpui: the window, the surface, the keyboard and the fonts.
            wayland
            libxkbcommon
            vulkan-loader
            libX11
            libxcb
            libxcursor
            libxi
            libxrandr
            fontconfig
            freetype
            libGL
            # `cpal`, for playing a voice note and for recording one.
            alsa-lib
            zlib
            zstd
          ];

          # Wayland, the Vulkan loader and the X libraries are opened by name at
          # *run* time rather than linked, so being on the link path is not
          # enough: without this the binary builds and then cannot find a
          # surface to draw on.
          linuxRuntime = pkgs.lib.makeLibraryPath linuxLibraries;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.rust-analyzer
              # presage's protocol crates and spqr generate code from .proto
              # files at build time. A warm target/ hides this until something
              # invalidates one of them, and the failure then names spqr rather
              # than the missing tool.
              pkgs.protobuf
              # Contact discovery (presage's `cdsi` feature, which is the only
              # way to resolve a phone number to an account) reaches
              # libsignal-net, and through it Signal's fork of BoringSSL, which
              # is a cmake project with a Go-based build step.
              pkgs.cmake
              pkgs.go
            ]
            ++ pkgs.lib.optionals onLinux (
              [
                # Every `-sys` crate in the tree asks it where its library is.
                pkgs.pkg-config
                # BoringSSL's build wants a C++ toolchain of its own naming.
                pkgs.clang
              ]
              ++ linuxLibraries
            );

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            PROTOC = "${pkgs.protobuf}/bin/protoc";

            LD_LIBRARY_PATH = pkgs.lib.optionalString onLinux linuxRuntime;
            PKG_CONFIG_PATH = pkgs.lib.optionalString onLinux (
              pkgs.lib.concatMapStringsSep ":" (library: "${pkgs.lib.getDev library}/lib/pkgconfig") linuxLibraries
            );

            # gpui compiles its shaders with Xcode's `metal`, which is not in
            # nixpkgs and lives behind a cryptex mount whose path changes with
            # every toolchain update -- so it has to be asked for rather than
            # written down. A shell that replaces PATH instead of adding to it
            # satisfies protoc and breaks this, which is the failure the build
            # notes in AGENTS.md warn about.
            #
            # `xcrun` has to be asked with DEVELOPER_DIR out of the way: nixpkgs
            # points it at the SDK from the store, which carries no toolchain, so
            # `xcrun -f metal` there answers "not found". Only the lookup is
            # unset -- the build still wants the store SDK.
            shellHook = nixpkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
              metal="$(env -u DEVELOPER_DIR -u SDKROOT /usr/bin/xcrun -f metal 2>/dev/null)"
              if [ -n "$metal" ]; then
                export PATH="$(dirname "$metal"):$PATH"
              else
                echo "petunia: no Xcode metal toolchain on this machine; gpui will not build" >&2
              fi
            '';
          };
        }
      );
    };
}
