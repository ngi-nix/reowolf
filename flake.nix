{
  description = "This library builds upon the previous Reowolf 1.0 implementation, with the intention of incrementally moving towards Reowolf 2.0. The goals of Reowolf 2.0 are to provide a generalization of sockets for communication over the internet, to simplify the implementation of algorithms that require distributed consensus, and to have a clear delineation between protocol and data whenever information is transmitted over the internet.";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable-small";

  outputs = { self, nixpkgs }:
    let
      pname = "reowolf";
      version = "1.1.0";

      supportedSystems = [ "x86_64-linux" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f system);

      nixpkgsFor = forAllSystems (system: import nixpkgs { inherit system; overlays = [ self.overlay ]; });
    in
    {
      overlay = final: prev: {
        reowolf = with final; rustPlatform.buildRustPackage rec {
          inherit pname version;

          src = builtins.path { path = ./.; name = pname; };

          cargoLock = { lockFile = ./Cargo.clib.lock; };
          postPatch = ''
            cp ${./Cargo.clib.lock} Cargo.lock
          '';

          outputs = [ "out" "headers" ];

          postFixup = ''
            mkdir -p $headers/include

            cp reowolf.h $headers/include/
            cp pseudo_socket.h $headers/include/
          '';
        };
      };

      packages = forAllSystems (system:
        {
          inherit (nixpkgsFor.${system}) reowolf;
        });

      defaultPackage = forAllSystems (system: self.packages.${system}.reowolf);

      devShell = forAllSystems (system: self.packages.${system}.reowolf);

      checks = forAllSystems (system: {
        inherit (self.packages.${system}) reowolf;
      });
    };
}
