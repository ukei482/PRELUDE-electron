# Claude のデスクトップアプリ。nixpkgs に無いので、Anthropic の配布する .deb を NixOS 用に包む。
# 中身はビルド済みの Electron なので、コンパイルはしない。版を上げるときは version と hash を、
# https://downloads.claude.ai/claude-desktop/apt/stable/ の Packages に合わせる
{
  lib, stdenv, fetchurl, dpkg, autoPatchelfHook, makeWrapper, alsa-lib, at-spi2-atk, at-spi2-core, cairo, cups, dbus,
  expat, gdk-pixbuf, glib, gtk3, libcap_ng, libdrm, libgbm, libGL, libnotify, libsecret, libx11, libxcb, libxcomposite, libxdamage,
  libseccomp, libxext, libxfixes, libxkbcommon, libxrandr, nspr, nss, pango, pipewire, systemd, vulkan-loader, wayland, xdg-utils,
}:
let
  # アプリが起動のときに開くライブラリ(リンクされていないもの)
  runtimeLibs = [ libGL libsecret libnotify vulkan-loader wayland ];
in
stdenv.mkDerivation rec {
  pname = "claude-desktop";
  version = "4.16336.0";

  src = fetchurl {
    url = "https://downloads.claude.ai/claude-desktop/apt/stable/pool/main/c/claude-desktop/claude-desktop_${version}_amd64.deb";
    sha256 = "3810733e93792f70eae921175d13e53d1cc6bdce9dc23117f0588087521a64bc";
  };

  nativeBuildInputs = [ dpkg autoPatchelfHook makeWrapper ];
  buildInputs = [
    alsa-lib at-spi2-atk at-spi2-core cairo cups dbus expat gdk-pixbuf glib gtk3 libcap_ng libdrm libgbm libseccomp libx11 libxcb libxcomposite
    libxdamage libxext libxfixes libxkbcommon libxrandr nspr nss pango pipewire systemd stdenv.cc.cc
  ];

  # chrome-sandbox の setuid は Nix のストアでは付けられないので、権限を落として展開する
  unpackPhase = "dpkg-deb --fsys-tarfile $src | tar -x --no-same-permissions --no-same-owner";

  installPhase = ''
    mkdir -p $out/bin $out/lib $out/share
    cp -r usr/lib/claude-desktop $out/lib/
    cp -r usr/share/icons $out/share/
    mkdir -p $out/share/applications
    substitute usr/share/applications/com.anthropic.Claude.desktop $out/share/applications/com.anthropic.Claude.desktop \
      --replace-quiet /usr/bin/claude-desktop $out/bin/claude-desktop
    makeWrapper $out/lib/claude-desktop/claude-desktop $out/bin/claude-desktop \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
      --prefix PATH : ${lib.makeBinPath [ xdg-utils ]}
  '';

  # コピーで壊れないように、付属のバイナリは strip しない
  dontStrip = true;

  meta = {
    description = "Claude desktop app";
    homepage = "https://claude.ai";
    license = lib.licenses.unfree;
    platforms = [ "x86_64-linux" ];
    mainProgram = "claude-desktop";
  };
}
