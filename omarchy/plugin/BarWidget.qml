// Omarchy のバーの PRELUDE の部品(docs/redesign-rust.md の「Omarchy の上で PRELUDE が使うところ」)。
// `prelude watch --bar`(waybar の形の JSON を 1 行ずつ)を読んで、スペースと今のタブを出す。
//   左クリック  PRELUDE のランチャー(prelude-omarchy-launcher。Omarchy のメニューで候補を選ぶ)
//   中クリック  AI のペイン(今のブックマークのフォルダで既定の AI)
//   右クリック  Omarchy のメニューの PRELUDE の項目
// 設定(shell.json のこの部品の項目): maxWidth(既定 320)、launcher(左クリックのコマンド)
import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "prelude.bar"

  property string label: ""
  property string tip: ""
  property bool online: false
  readonly property int maxLabelWidth: Number(setting("maxWidth", 320))
  readonly property string launcher: String(setting("launcher", "prelude-omarchy-launcher"))

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

  Row {
    id: row
    anchors.centerIn: parent
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
      else root.bar.run(root.launcher)
    }
    onEntered: if (root.bar) root.bar.showTooltip(root, root.tip !== "" ? root.tip : "PRELUDE")
    onExited: if (root.bar) root.bar.hideTooltip(root)
  }
}
