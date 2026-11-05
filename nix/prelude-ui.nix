# PRELUDE の GPUI の UI(prelude-sidebar・prelude-launcher・prelude-filer・prelude-settings)。ui/ のワークスペース。
# ビルドは重い(GPUI ごと)。`--cores 2 --max-jobs 1` を付けて、メモリの空きを見てから動かす
{
  lib,
  rustPlatform,
  clang,
  cmake,
  pkg-config,
  fontconfig,
  freetype,
  libGL,
  libx11,
  libxcb,
  libxkbcommon,
  patchelf,
  vulkan-loader,
  wayland,
}:
let
  # 起動のときに dlopen するものも含む。LD_LIBRARY_PATH はファイラから開くアプリに漏れるので、実行ファイルの rpath に入れる
  runtimeLibs = [ fontconfig freetype libGL libx11 libxcb libxkbcommon vulkan-loader wayland ];
in
rustPlatform.buildRustPackage {
  pname = "prelude-ui";
  version = (lib.importTOML ../crates/prelude-proto/Cargo.toml).package.version;
  # prelude-proto はパスで使う。ルートの Cargo.toml はワークスペースの継承に、src/fonts は Electron 版と同じ字に要る
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../crates ../src/fonts ../ui/Cargo.toml ../ui/Cargo.lock ../ui/prelude-ui ../ui/prelude-sidebar ../ui/prelude-launcher ../ui/prelude-filer ../ui/prelude-settings ];
  };
  cargoRoot = "ui";
  buildAndTestSubdir = "ui";
  cargoHash = "sha256-WD0HxWrp4mP0BZg7gT4ZGV6Rkir/HK6lSuuAeBm7w9k=";
  nativeBuildInputs = [ clang cmake patchelf pkg-config rustPlatform.bindgenHook ];
  buildInputs = runtimeLibs;
  doCheck = false;
  # sway は席のキーボードが決まる前の窓に、keymap を送らずに modifiers を送ることがある。GPUI はそこで unwrap して落ちるので、
  # keymap が届くまで読み飛ばす(GPUI のコミットを上げるときに、直っていれば外す)
  postPatch = ''
    client=$cargoDepsCopy/source-git-0/gpui_linux-0.1.0/src/linux/wayland/client.rs
    sed -i \
      -e '/wl_keyboard::Event::Modifiers {/,/let old_layout =/ s/let keymap_state = state.keymap_state.as_mut().unwrap();/let Some(keymap_state) = state.keymap_state.as_mut() else { return };/' \
      -e 's/let keymap_state = state.keymap_state.as_ref().unwrap();/let Some(keymap_state) = state.keymap_state.as_ref() else { return };/' \
      "$client"
    test "$(grep -c 'else { return };' "$client")" -ge 3
    ! grep -q 'keymap_state.as_ref().unwrap()' "$client"
  '';
  postFixup = ''
    for f in $out/bin/*; do
      patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} "$f"
    done
  '';
  meta = {
    description = "PRELUDE のサイドバー・ランチャー・ファイラ・設定(GPUI)";
    platforms = lib.platforms.linux;
  };
}
