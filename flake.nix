{
  description = "purl - a curl-like CLI tool for HTTP requests with automatic payment support";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        
        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];
        
        buildInputs = with pkgs; [
          openssl
        ] ++ lib.optionals stdenv.isDarwin [
          apple-sdk_15
          (darwinMinVersionHook "10.15")
        ];
        
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "purl";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          
          inherit nativeBuildInputs buildInputs;
          
          cargoBuildFlags = [ "-p" "purl-cli" ];
          
          meta = with pkgs.lib; {
            description = "A curl-like CLI tool for HTTP requests with automatic payment support";
            homepage = "https://github.com/tempoxyz/purl";
            license = with licenses; [ mit asl20 ];
            mainProgram = "purl";
          };
        };

        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs;
          
          packages = with pkgs; [
            nodejs_22
          ];
          
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}
