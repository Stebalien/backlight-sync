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
    in
    {
      packages = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
          src =
            let
              unfilteredRoot = ./.;
            in
            pkgs.lib.fileset.toSource {
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
          backlight-sync = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              installPhaseCommand = ''
                make install install-udev-rules PREFIX="$out" LIBEXECDIR="$out/libexec" DESTDIR=""
              '';
            }
          );
        in
        {
          inherit backlight-sync;
          default = backlight-sync;
        }
      );

      formatter = eachSystem (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);

      overlays.default = final: prev: {
        inherit (self.packages.${final.system}) backlight-sync;
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
            package = lib.mkPackageOption self.packages.${pkgs.system} "backlight-sync" { };
          };
          config = lib.mkIf cfg.enable {
            systemd = {
              packages = [ cfg.package ];
              services.backlight-sync.wantedBy = [ "graphical.service" ];
            };
          };
        };
    };
}
