// Omarchy のバーの PRELUDE の部品(docs/redesign-rust.md の「Omarchy の上で PRELUDE が使うところ」)。
// `prelude watch --bar`(waybar の形の JSON を 1 行ずつ)を読んで、スペースと今のタブを出す。
//   左クリック  サイドバー(Sidebar.qml)を開け閉めする。設定の click を "launcher" にするとランチャー
//   ボタン      題名の右に 新しいタブ・分割して開く・ブックマーク
//   中クリック  AI のペイン(今のブックマークのフォルダで既定の AI)
//   右クリック  Omarchy のメニューの PRELUDE の項目
// 設定(shell.json のこの部品の項目。`omarchy bar set prelude.bar <名前> <値>`):
//   maxWidth          バーの題名の最大の幅(既定 320)
//   launcher          ランチャーのコマンド(既定 prelude-omarchy-launcher)
//   newTab            新しいタブのコマンド(既定 prelude-omarchy-newtab。Omarchy のアプリの一覧で選んだ窓を新しいタブに)
//   split             分割して開くコマンド(既定 prelude-omarchy-newtab --split。選んだ窓を今のタブのペインに)
//   bookmark          ブックマークのボタンのコマンド(既定 prelude-omarchy-bookmark)
//   buttons           題名の右の 新しいタブ・分割・ブックマーク のボタンを出すか(既定 true)
//   click             左クリック: "sidebar"(既定)・"launcher"
//   sidebarWidth      サイドバーの幅(既定 260、160〜800)
//   sidebarCompactWidth  細い列の幅(既定はバーの P のボタンと同じ幅。数を書けばその幅、16〜120)
//   sidebarSide       置く側: "left"(既定)・"right"
//   sidebarOpen       シェルの起動のときに開いておくか(既定 false)
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "SidebarModel.js" as SidebarModel

BarWidget {
  id: root
  moduleName: "prelude.bar"

  property string label: ""
  property string tip: ""
  property bool online: false
  readonly property int maxLabelWidth: Number(setting("maxWidth", 320))
  readonly property string launcher: String(setting("launcher", "prelude-omarchy-launcher"))
  readonly property string newTab: String(setting("newTab", "prelude-omarchy-newtab"))
  readonly property string splitCmd: String(setting("split", "prelude-omarchy-newtab --split"))
  readonly property string bookmarkCmd: String(setting("bookmark", "prelude-omarchy-bookmark"))
  readonly property bool showButtons: String(setting("buttons", true)) !== "false"
  readonly property string click: String(setting("click", "sidebar"))
  readonly property int sidebarWidth: SidebarModel.width(setting("sidebarWidth", 260), 260, 160, 800)
  // バーの P のボタン(印と左右の余白)の幅
  readonly property int buttonWidth: Math.round(mark.implicitWidth + Style.spacing.controlPaddingX * 2)
  readonly property int sidebarCompactWidth: SidebarModel.width(setting("sidebarCompactWidth", 0), buttonWidth, 16, 120)
  readonly property string sidebarSide: String(setting("sidebarSide", "left")) === "right" ? "right" : "left"

  // サイドバーは部品の窓と同じ画面に出す。開いているか・細い列かは、シェルが動いている間だけ覚える
  property bool sidebarShown: String(setting("sidebarOpen", false)) === "true"
  property bool sidebarCompact: false
  readonly property var hostScreen: root.QsWindow.window ? root.QsWindow.window.screen : null

  // waybar の形は pango の文字なので、戻す
  function plain(s) {
    return String(s || "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&")
  }

  function read(line) {
    try {
      var v = JSON.parse(line)
      root.online = v["class"] === "online"
      root.label = plain(v.text)
      root.tip = plain(v.tooltip)
    } catch (e) {}
  }

  // コアが動いていなくても印だけは出して、押せばランチャーが開く(コアが無いことはランチャーが知らせる)
  visible: !vertical
  implicitWidth: visible ? row.implicitWidth + Style.spacing.controlPaddingX * 2 : 0
  implicitHeight: barSize

  Process {
    id: watch
    command: ["prelude", "watch", "--bar"]
    running: true
    stdout: SplitParser { onRead: function(line) { root.read(line) } }
    // prelude が無い・落ちたときは、少し待って読み直す
    onExited: restart.start()
  }

  Timer {
    id: restart
    interval: 3000
    onTriggered: watch.running = true
  }

  LazyLoader {
    active: root.sidebarShown && !!root.hostScreen
    Sidebar {
      screen: root.hostScreen
      compact: root.sidebarCompact
      wideWidth: root.sidebarWidth
      compactWidth: root.sidebarCompactWidth
      side: root.sidebarSide
      newTab: root.newTab
      fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
      onCloseRequested: root.sidebarShown = false
      onCompactToggled: root.sidebarCompact = !root.sidebarCompact
    }
  }

  // P と題名(押すとサイドバー)、その右に 新しいタブ・分割して開く・ブックマーク のボタン
  readonly property var buttons: [
    { glyph: "\u{F0415}", tip: "新しいタブ(Super+Alt+Space)", cmd: root.newTab },
    { glyph: "\u{F0DB}", tip: "分割して開く(今のタブにアプリを足す)", cmd: root.splitCmd },
    { glyph: "\u{F00C0}", tip: "今のタブをブックマークにする", cmd: root.bookmarkCmd }
  ]

  Row {
    id: row
    anchors.centerIn: parent
    spacing: Style.space(8)

    Item {
      id: main
      anchors.verticalCenter: parent.verticalCenter
      width: mainRow.implicitWidth
      height: root.barSize

      Row {
        id: mainRow
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(6)

        Text {
          id: mark
          textFormat: Text.PlainText
          anchors.verticalCenter: parent.verticalCenter
          text: "P"
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.body
          font.bold: true
          color: root.bar ? root.bar.barForeground : Commons.Color.foreground
          opacity: root.online ? 1 : 0.45
          // サイドバーが開いているあいだは下線
          Rectangle {
            visible: root.sidebarShown
            anchors.top: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            width: parent.width + Style.space(2)
            height: Style.space(2)
            color: parent.color
          }
        }

        Text {
          id: labelText
          textFormat: Text.PlainText
          anchors.verticalCenter: parent.verticalCenter
          visible: text !== ""
          width: Math.min(root.maxLabelWidth, implicitWidth)
          text: root.label
          color: root.bar ? root.bar.barForeground : Commons.Color.foreground
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
          opacity: 0.85
        }
      }

      MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
        cursorShape: Qt.PointingHandCursor

        onClicked: function(mouse) {
          if (!root.bar) return
          if (mouse.button === Qt.RightButton) root.bar.run("omarchy menu summon prelude")
          else if (mouse.button === Qt.MiddleButton) root.bar.run("prelude new agent")
          else if (root.click === "launcher") root.bar.run(root.launcher)
          else root.sidebarShown = !root.sidebarShown
        }
        onEntered: if (root.bar) root.bar.showTooltip(main, root.tip !== "" ? root.tip : "PRELUDE")
        onExited: if (root.bar) root.bar.hideTooltip(main)
      }
    }

    Repeater {
      model: root.showButtons ? root.buttons : []
      delegate: Item {
        id: btn
        required property var modelData
        anchors.verticalCenter: parent.verticalCenter
        width: glyph.implicitWidth + Style.space(4)
        height: root.barSize

        Text {
          id: glyph
          anchors.centerIn: parent
          textFormat: Text.PlainText
          text: btn.modelData.glyph
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.body
          color: root.bar ? root.bar.barForeground : Commons.Color.foreground
          opacity: btnArea.containsMouse ? 1 : (root.online ? 0.7 : 0.35)
        }

        MouseArea {
          id: btnArea
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: if (root.bar && btn.modelData.cmd) root.bar.run(btn.modelData.cmd)
          onEntered: if (root.bar) root.bar.showTooltip(btn, btn.modelData.tip)
          onExited: if (root.bar) root.bar.hideTooltip(btn)
        }
      }
    }
  }
}
