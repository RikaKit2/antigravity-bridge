{
  description = "Universal OpenAI-to-Antigravity Chat & FIM completions bridge";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "antigravity-bridge";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            postPatch = ''
              rm -rf .cargo/config.toml
            '';
            meta = with pkgs.lib; {
              description = "Universal OpenAI-to-Antigravity completions bridge";
              homepage = "https://github.com/RikaKit2/antigravity-bridge";
              license = licenses.agpl3Only;
              mainProgram = "antigravity-bridge";
            };
          };
        }
      );
    };
}
