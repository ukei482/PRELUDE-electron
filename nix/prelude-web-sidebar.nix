# HTML のサイドバーの試作(proto/web)を sway で使うためのパッケージ。
#   prelude-sidebar-bridge  橋(bridge.js)。コアのソケットと HTML の中身をつなぎ、http://127.0.0.1:8370/ で出す
#   prelude-sidebar-web     窓のホスト(layer-host.py)。WebKitGTK の面を layer-shell で画面の左端に貼る
# 本番の形(Rust の外側・WebKitGTK・gtk-layer-shell)ができるまでの仮のもの。
{
  lib,
  stdenvNoCC,
  makeWrapper,
  wrapGAppsHook3,
  gobject-introspection,
  nodejs,
  python3,
  gtk3,
  webkitgtk_4_1,
  gtk-layer-shell,
  glib-networking,
}:
let
  py = python3.withPackages (p: [ p.pygobject3 ]);
in
stdenvNoCC.mkDerivation {
  pname = "prelude-web-sidebar";
  version = (lib.importTOML ../crates/prelude-core/Cargo.toml).package.version;
  src = lib.fileset.toSource {
    root = ../proto/web;
    fileset = lib.fileset.unions (
      map (f: ../proto/web + "/${f}") [
        "bridge.js"
        "parts.js"
        "glue.js"
        "sidebar.js"
        "sidebar.css"
        "sidebar.html"
        "fake.js"
        "host.js"
        "layer-host.py"
      ]
    );
  };
  nativeBuildInputs = [
    makeWrapper
    wrapGAppsHook3
    gobject-introspection
  ];
  buildInputs = [
    py
    gtk3
    webkitgtk_4_1
    gtk-layer-shell
    glib-networking
  ];
  dontWrapGApps = true;
  installPhase = ''
    runHook preInstall
    d=$out/share/prelude-web-sidebar
    mkdir -p $d $out/bin
    cp *.js *.css *.html $d/
    install -Dm755 layer-host.py $d/layer-host.py
    makeWrapper ${nodejs}/bin/node $out/bin/prelude-sidebar-bridge --add-flags $d/bridge.js
    runHook postInstall
  '';
  # WebKitGTK・gtk-layer-shell の型の情報(GI_TYPELIB_PATH)などを付けて、ホストを包む
  postFixup = ''
    makeWrapper ${py}/bin/python3 $out/bin/prelude-sidebar-web \
      "''${gappsWrapperArgs[@]}" \
      --add-flags $out/share/prelude-web-sidebar/layer-host.py
  '';
  meta = {
    description = "PRELUDE の HTML のサイドバー(試作)と、それを画面の左端に出すホスト";
    platforms = lib.platforms.linux;
  };
}
