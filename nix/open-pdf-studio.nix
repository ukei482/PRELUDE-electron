# Open PDF Studio。nixpkgs に無く、snap は NixOS で使えないので、公式の .deb を NixOS 用に包む。コンパイルはしない。
# 版を上げるときは version と hash を https://github.com/OpenAEC-Foundation/open-pdf-studio/releases に合わせる
{
  lib, stdenv, fetchurl, dpkg, autoPatchelfHook, wrapGAppsHook3, cairo, dbus, gdk-pixbuf, glib, glib-networking, gtk3,
  libsoup_3, webkitgtk_4_1, xz,
}:
stdenv.mkDerivation rec {
  pname = "open-pdf-studio";
  version = "2026.44.8";

  src = fetchurl {
    url = "https://github.com/OpenAEC-Foundation/open-pdf-studio/releases/download/v${version}/Open.PDF.Studio_${version}_amd64.deb";
    hash = "sha256-Sx1QYkUrd5QXReT/dX0UazSTgQ3Ppl7wy34wBZ7iJl8=";
  };

  nativeBuildInputs = [ dpkg autoPatchelfHook wrapGAppsHook3 ];
  buildInputs = [ cairo dbus gdk-pixbuf glib glib-networking gtk3 libsoup_3 stdenv.cc.cc webkitgtk_4_1 xz ];

  unpackPhase = "dpkg-deb --fsys-tarfile $src | tar -x --no-same-permissions --no-same-owner";

  installPhase = ''
    mkdir -p $out
    cp -r usr/bin usr/lib usr/share $out/
    substituteInPlace "$out/share/applications/Open PDF Studio.desktop" --replace-quiet /usr/bin/open-pdf-studio $out/bin/open-pdf-studio
  '';

  dontStrip = true;

  meta = {
    description = "Open source PDF viewer/editor";
    homepage = "https://github.com/OpenAEC-Foundation/open-pdf-studio";
    platforms = [ "x86_64-linux" ];
    mainProgram = "open-pdf-studio";
  };
}
