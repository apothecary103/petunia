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
            ];

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            PROTOC = "${pkgs.protobuf}/bin/protoc";

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
