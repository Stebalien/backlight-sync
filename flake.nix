{
  description = "A daemon that syncs external monitor backlights with a laptop's backlight";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      eachSystem = nixpkgs.lib.genAttrs [
        "i686-linux"
        "x86_64-linux"
        "aarch64-linux"
        "armv7l-linux"
      ];
      mkPackages =
        pkgs:
        let
          craneLib = crane.mkLib pkgs;
          unfilteredRoot = ./.;
          src = pkgs.lib.fileset.toSource {
            root = unfilteredRoot;
            fileset = pkgs.lib.fileset.unions [
              (craneLib.fileset.commonCargoSources unfilteredRoot)
              ./Makefile
              ./contrib
            ];
          };
          commonArgs = {
            inherit src;
            strictDeps = true;
            buildInputs = [ pkgs.udev ];
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.m4
            ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        rec {
          backlight-sync = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              installPhaseCommand = ''
                make install install-udev-rules PREFIX="$out" LIBEXECDIR="$out/libexec" DESTDIR=""
              '';
            }
          );
          default = backlight-sync;
        };
    in
    {
      packages = eachSystem (system: mkPackages nixpkgs.legacyPackages.${system});
      formatter = eachSystem (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);

      overlays.default = final: prev: {
        inherit (mkPackages final) backlight-sync;
      };

      nixosModules.default =
        {
          lib,
          config,
          pkgs,
          ...
        }:
        let
          cfg = config.services.backlight-sync;
        in
        {
          options.services.backlight-sync = {
            enable = lib.mkEnableOption "enable the backlight-sync daemon";
            package = lib.mkPackageOption (mkPackages pkgs) "backlight-sync" { };
          };
          config = lib.mkIf cfg.enable {
            assertions = [
              {
                assertion = config.hardware.i2c.enable;
                message = "The backlight sync daemon requires i2c.";
              }
            ];
            systemd = {
              packages = [ cfg.package ];
              services.backlight-syncd.wantedBy = [ "graphical.target" ];
            };
            hardware.i2c.enable = true;
          };
        };
    };
}
