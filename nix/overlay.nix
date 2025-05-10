prev: final: {
  demostf-sync = final.callPackage ./package.nix {};
  demostf-sync-docker = final.callPackage ./docker.nix {};
}
