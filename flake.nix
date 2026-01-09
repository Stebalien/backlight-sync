{
  description = "A daemon that syncs external monitor backlights with a laptop's backlight";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ { crane, flake-parts, ... }: flake-parts.lib.mkFlake { inherit inputs; } (
    { moduleWithSystem, ... }:
    {
      systems = [
        "i686-linux"
        "x86_64-linux"
        "aarch64-linux"
        "armv7l-linux"
      ];
      imports = [ flake-parts.flakeModules.easyOverlay ];
      perSystem = { config, system, lib, pkgs, ...}:
        let
          craneLib = crane.mkLib pkgs;
          src = let
            unfilteredRoot = ./.;
          in lib.fileset.toSource {
            root = unfilteredRoot;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources unfilteredRoot)
              ./Makefile
              ./contrib
            ];
          };
          commonArgs = {
            inherit src;
            strictDeps = true;
            buildInputs = [ pkgs.udev ];
            nativeBuildInputs = [ pkgs.pkg-config  pkgs.m4 ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          backlight-sync = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            installPhaseCommand = ''
              make install install-udev-rules PREFIX="$out" LIBEXECDIR="$out/libexec" DESTDIR=""
            '';
          });
        in rec {
          packages = {
            inherit backlight-sync;
            default = packages.backlight-sync;
          };
          overlayAttrs = {
            inherit (config.packages) backlight-sync;
          };
        };
      flake.nixosModules.default = moduleWithSystem (
        perSystem@{pkgs, self', ... }:
        nixos@{lib, config, ... }:
        let
          cfg = config.services.backlight-sync;
        in
        {
          options.services.backlight-sync = {
            enable = lib.mkEnableOption "enable the backlight-sync daemon";
            package = lib.mkPackageOption self'.packages "backlight-sync" { };
          };
          config = lib.mkIf cfg.enable {
            systemd = {
              packages = [ cfg.package ];
              services.backlight-sync.wantedBy = [ "graphical.service" ];
            };
          };
        });
    });
  }
