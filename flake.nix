# PRELUDE の開発環境と、NixOS の設定(docs/redesign-rust.md の判断 9)。
#   nix develop                               開発環境に入る(cargo・npm・GPUI のビルド)
#   sudo nixos-rebuild switch --flake .#prelude   OS に反映する
{
  description = "PRELUDE";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      # GPUI が起動のときに読むライブラリ(リンクせずに開くものも含む)
      runtimeLibs = with pkgs; [
        fontconfig
        freetype
        libGL
        libx11
        libxcb
        libxkbcommon
        vulkan-loader
        wayland
      ];
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          # Rust 版(crates・ui)
          cargo
          clang
          clippy
          cmake
          pkg-config
          rust-analyzer
          rustc
          rustfmt
          # Electron 版。npm の electron は NixOS では動かないので、nixpkgs のものを使う(`electron . --no-sandbox`)
          electron
          nodejs
          python3 # ホームの端末(sys/ptyhost.py)
          git
        ];
        buildInputs = runtimeLibs;
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
        # npm install で Electron の本体を取りに行かない
        ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
      };

      # hardware-configuration.nix は入れる PC で `nixos-generate-config` が作る
      nixosConfigurations.prelude = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [ ./nix/configuration.nix ./nix/hardware-configuration.nix ];
      };
    };
}
