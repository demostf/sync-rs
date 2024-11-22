{ dockerTools
, demostf-sync
,
}:
dockerTools.buildLayeredImage {
  name = "demostf/sync";
  tag = "latest";
  maxLayers = 5;
  contents = [
    demostf-sync
    dockerTools.caCertificates
  ];
  config = {
    Cmd = [ "sync" ];
    ExposedPorts = {
      "80/tcp" = { };
    };
  };
}
