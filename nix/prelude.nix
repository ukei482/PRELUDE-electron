# PRELUDE の Rust 版のコア(prelude-core)とコマンド(prelude)。crates/ のワークスペース。GPUI の UI(ui/)は別に包む
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "prelude";
  version = (lib.importTOML ../crates/prelude-core/Cargo.toml).package.version;
  src = lib.fileset.toSource {
    root = ../.;
    # skills/ は prelude コマンドに埋め込む(prelude skill)
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../crates ../portal ../skills ];
  };
  cargoLock.lockFile = ../Cargo.lock;
  # テストは CI と手元の cargo test で流す(ビルドの砂場では、子のプロセスの終わりを確かめるテストが通らない)
  doCheck = false;
  # アプリのダイアログのポータルの宣言(xdg.portal.extraPortals が読む)
  postInstall = ''
    install -Dm644 portal/prelude.portal $out/share/xdg-desktop-portal/portals/prelude.portal
  '';
  meta = {
    description = "PRELUDE のコアとコマンド(スペース・ブックマーク・タブの持ち主)";
    mainProgram = "prelude";
    platforms = lib.platforms.linux;
  };
}
