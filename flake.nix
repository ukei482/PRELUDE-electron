# PRELUDE の開発環境と、NixOS の設定(docs/redesign-rust.md の判断 9)。
#   nix develop                               開発環境に入る(cargo・npm・GPUI のビルド)
#   sudo nixos-rebuild switch --flake .#prelude   OS に反映する(/etc/nixos ではなく、いつもこのリポジトリから)
{
  description = "PRELUDE";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; config.allowUnfree = true; };
      # GPUI(Zed)の rust-toolchain.toml に合わせる。既定の rustc は古い(1.95)。
      # nixpkgs の rust_1_98 は clippy がキャッシュに無く、手元でのコンパイルに 3GB 以上要るので、配布済みのものを使う
      rust = pkgs.rust-bin.stable."1.98.1".default.override { extensions = [ "clippy" "rustfmt" "rust-src" "rust-analyzer" ]; };
      rustPlatform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };
      prelude = pkgs.callPackage ./nix/prelude.nix { inherit rustPlatform; };
      prelude-ui = pkgs.callPackage ./nix/prelude-ui.nix { inherit rustPlatform; };
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
      packages.${system} = {
        inherit prelude prelude-ui;
        default = prelude;
        claude-desktop = pkgs.callPackage ./nix/claude-desktop.nix { };
        open-pdf-studio = pkgs.callPackage ./nix/open-pdf-studio.nix { };
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          # Rust 版(crates・ui)
          clang
          cmake
          pkg-config
          rust
          # Electron 版。npm の electron は NixOS では動かないので、nixpkgs のものを使う(`electron . --no-sandbox`)。
          # 版は package.json に合わせる。既定の electron は古い(43)
          electron_44
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
      # PRELUDE の NixOS モジュール(services.prelude)。ほかの設定からも `inputs.prelude.nixosModules.default` で使える
      nixosModules.default = { pkgs, lib, ... }: {
        imports = [ ./nix/module.nix ];
        services.prelude.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.prelude;
        services.prelude.ui.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.prelude-ui;
      };

      nixosConfigurations.prelude = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [ self.nixosModules.default ./nix/configuration.nix ./nix/hardware-configuration.nix ];
      };
    };
}
