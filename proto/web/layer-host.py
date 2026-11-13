#!/usr/bin/env python3
"""HTML のサイドバーを、画面の左端に貼り付く面(layer-shell)で出すホスト(試作)。

本番の形(docs/redesign-rust.md の「サイドバー」: Rust の外側・WebKitGTK・gtk-layer-shell)を、Python で仮に作ったもの。
中身は橋(bridge.js)が出す sidebar.html で、コアとのやり取りは橋が受け持つ。ここは窓の置き場所だけを決める:

- 左端の上から下まで貼り付き、その幅だけ窓が避ける(exclusive zone)
- 幅はサイドバーの中の状態に合わせる。中身が題名を `prelude:compact`・`prelude:wide` にするので、それを見て変える
- 画面の端との間・窓の角丸は、設定(settings.toml の [windows] の gaps_outer・corner_radius)に合わせる
- キーボードは、名前を変える欄などで押したときだけ受ける(on-demand)

使い方: prelude-sidebar-web [--url URL]   (既定は http://127.0.0.1:8370/sidebar.html)
"""
import os
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, Gtk, GtkLayerShell, WebKit2  # noqa: E402

WIDE = 272
THIN = 60


def settings():
    """settings.toml の [windows] の gaps_outer・corner_radius(px。auto・無しは 0)"""
    try:
        import tomllib
    except ImportError:  # Python 3.10 まで
        return 0, 0
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    try:
        with open(os.path.join(base, "prelude", "settings.toml"), "rb") as f:
            w = tomllib.load(f).get("windows", {})
    except (OSError, ValueError):
        return 0, 0
    px = lambda v: v if isinstance(v, int) and 0 <= v <= 64 else 0  # noqa: E731
    return px(w.get("gaps_outer")), px(w.get("corner_radius"))


def main():
    url = "http://127.0.0.1:8370/sidebar.html"
    args = sys.argv[1:]
    if "--url" in args and args.index("--url") + 1 < len(args):
        url = args[args.index("--url") + 1]
    gap, radius = settings()
    if gap or radius:
        sep = "&" if "?" in url else "?"
        url = f"{url}{sep}gap={gap}&radius={radius}"

    win = Gtk.Window(title="PRELUDE サイドバー")
    if not GtkLayerShell.is_supported():
        # layer-shell の無い環境(試す用): 普通の窓で出す
        print("prelude-sidebar-web: layer-shell が無いので、普通の窓で出す", file=sys.stderr)
    else:
        GtkLayerShell.init_for_window(win)
        GtkLayerShell.set_namespace(win, "prelude-sidebar")
        GtkLayerShell.set_layer(win, GtkLayerShell.Layer.TOP)
        for edge in (GtkLayerShell.Edge.TOP, GtkLayerShell.Edge.BOTTOM, GtkLayerShell.Edge.LEFT):
            GtkLayerShell.set_anchor(win, edge, True)
            GtkLayerShell.set_margin(win, edge, gap)
        GtkLayerShell.set_keyboard_mode(win, GtkLayerShell.KeyboardMode.ON_DEMAND)

    # 角丸・透明な所は後ろが見えるように、窓を透明にする(色はサイドバーの中身が塗る)
    screen = win.get_screen()
    visual = screen.get_rgba_visual()
    if visual is not None:
        win.set_visual(visual)
    win.set_app_paintable(True)

    view = WebKit2.WebView()
    view.set_background_color(Gdk.RGBA(0, 0, 0, 0))
    view.get_settings().set_enable_developer_extras(bool(os.environ.get("PRELUDE_DEBUG")))
    win.add(view)

    def set_width(w):
        win.set_size_request(w, -1)
        win.resize(w, 1)
        if GtkLayerShell.is_supported():
            GtkLayerShell.set_exclusive_zone(win, w)

    def on_title(v, _p):
        t = v.get_title() or ""
        if t == "prelude:compact":
            set_width(THIN)
        elif t == "prelude:wide":
            set_width(WIDE)

    view.connect("notify::title", on_title)
    set_width(WIDE)
    view.load_uri(url)
    win.connect("destroy", Gtk.main_quit)
    win.show_all()
    Gtk.main()


if __name__ == "__main__":
    main()
