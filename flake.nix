{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/release-23.11";
    flocken = {
      url = "github:mirkolenz/flocken/v2";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    flocken,
  }:
    utils.lib.eachDefaultSystem (system: let
      overlays = [
        (import ./overlay.nix)
      ];
      pkgs = (import nixpkgs) {
        inherit system overlays;
      };
      inherit (flocken.legacyPackages.${system}) mkDockerManifest;
      inherit (builtins) fromTOML readFile;
      version = (fromTOML (readFile ./Cargo.toml)).package.version;
    in rec {
      packages = rec {
        sync = pkgs.demostf-sync;
        docker = pkgs.demostf-sync-docker;
        default = sync;

        dockerManifest = mkDockerManifest {
          tags = ["latest"];
          registries = {
            "docker.io" = {
              enable = true;
              repo = "demostf/sync";
              username = "$DOCKERHUB_USERNAME";
              password = "$DOCKERHUB_TOKEN";
            };
          };
          inherit version;
          images = with self.packages; [x86_64-linux.docker aarch64-linux.docker];
        };
      };
      devShells.default = pkgs.mkShell {
        OPENSSL_NO_VENDOR = 1;

        nativeBuildInputs = with pkgs; [
          cargo
          rustc
          bacon
          cargo-edit
          cargo-outdated
          clippy
          cargo-audit
          cargo-watch
          pkg-config
          openssl
        ];
      };
    });
}
