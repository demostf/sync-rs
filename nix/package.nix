{
  stdenv,
  rustPlatform,
  lib,
  pkg-config,
  openssl,
}: let
  inherit (lib.sources) sourceByRegex;
  inherit (builtins) fromTOML readFile;
  src = sourceByRegex ../. ["Cargo.*" "(src)(/.*)?"];
  version = (fromTOML (readFile ../Cargo.toml)).package.version;
in
  rustPlatform.buildRustPackage rec {
    pname = "demostf-sync";

    inherit src version;

    buildInputs = [openssl];

    nativeBuildInputs = [pkg-config];

    cargoLock = {
      lockFile = ../Cargo.lock;
    };
  }
