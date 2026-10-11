{
  description = "Universal OpenAI-to-Antigravity Chat & FIM completions bridge";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      packages.${system} = {
        default = pkgs.stdenv.mkDerivation {
          pname = "antigravity-bridge";
          version = "0.1.0";
          src = ./.;

          dontBuild = true;

          installPhase = ''
            mkdir -p $out/bin
            cp dist/antigravity-bridge $out/bin/antigravity-bridge
            chmod +x $out/bin/antigravity-bridge
          '';

          meta = with pkgs.lib; {
            description = "Universal OpenAI-to-Antigravity completions bridge";
            homepage = "https://github.com/RikaKit2/antigravity-bridge";
            license = licenses.agpl3Only;
            mainProgram = "antigravity-bridge";
            platforms = [ "x86_64-linux" ];
          };
        };

        from-source = pkgs.rustPlatform.buildRustPackage {
          pname = "antigravity-bridge";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          postPatch = ''
            rm -rf .cargo/config.toml
          '';
        };
      };
    };
}
