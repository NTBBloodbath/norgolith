{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable&shallow=1";
    flake-utils.url = "github:numtide/flake-utils?shallow=1";
    norgolith.url = "github:NTBBloodbath/norgolith?shallow=1";
  };

  outputs = {
    nixpkgs,
    flake-utils,
    norgolith,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
        pkgs = import nixpkgs {inherit system;};
      in
      {
        # nix develop
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustywind # Organize Tailwind CSS classes
            watchman # required by tailwindcss CLI for watch functionality
            tailwindcss_4
            tailwindcss-language-server
            mprocs # Run multiple commands in parallel
            norgolith.packages.${system}.default
          ];
        };
      });
}
