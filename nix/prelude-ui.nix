# PRELUDE の GPUI の UI(prelude-sidebar・prelude-launcher・prelude-filer)。ui/ のワークスペース。
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
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../crates ../src/fonts ../ui/Cargo.toml ../ui/Cargo.lock ../ui/prelude-ui ../ui/prelude-sidebar ../ui/prelude-launcher ../ui/prelude-filer ];
  };
  cargoRoot = "ui";
  buildAndTestSubdir = "ui";
  cargoHash = "sha256-2Psj0jCs0q23ihmC2xV/2/NlmZ0pG8YUPZt3Qy7DGSQ=";
  nativeBuildInputs = [ clang cmake patchelf pkg-config rustPlatform.bindgenHook ];
  buildInputs = runtimeLibs;
  doCheck = false;
  postFixup = ''
    for f in $out/bin/*; do
      patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} "$f"
    done
  '';
  meta = {
    description = "PRELUDE のサイドバー・ランチャー・ファイラ(GPUI)";
    platforms = lib.platforms.linux;
  };
}
