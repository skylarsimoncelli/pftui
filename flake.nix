{
  description = "A beautiful terminal portfolio tracker with real-time prices, charts, and market data";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "pftui";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta = with pkgs.lib; {
            description = "A beautiful terminal portfolio tracker";
            homepage = "https://github.com/skylarsimoncelli/pftui";
            license = licenses.mit;
            mainProgram = "pftui";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ cargo rustc rust-analyzer clippy ];
        };
      });
}
