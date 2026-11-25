// Omarchy の上の PRELUDE のサイドバー(docs/omarchy.md の「サイドバー」、docs/redesign-rust.md の「サイドバー」)。
// 画面の端に layer-shell で貼り付き、幅の分だけ場所を予約する(Hyprland はほかの窓をその横に並べる)。
// 窓と同じ決まりで置く: 画面の端との間は Hyprland の gaps_out、隣の窓との間は窓と窓の間(gaps_in の 2 倍)、
// 枠の太さ・色(選ばれていない窓の色)・角丸も Hyprland の設定から読む。
// コアとは `prelude` コマンドだけでやり取りする: `prelude watch` の知らせで `prelude raw …` を読み直し、
// 押されたら `prelude raw <命令> <引数>` を送る(サイドバーにだけ許された操作は作らない)。
// 開け閉め・幅・置く側は BarWidget.qml が決める(バーの P を押すと開け閉め)。
import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import "SidebarModel.js" as Model

PanelWindow {
  id: win

  // BarWidget から
  property bool compact: false
  property int wideWidth: 260
  property int compactWidth: 48
  property string side: "left"
  property string newTab: "prelude-omarchy-newtab"
  property string fontFamily: Style.font.family
  signal closeRequested()
  signal compactToggled()

  // コアの様子
  property bool online: false
  property var core: ({})
  property var bookmarks: []
  property var folders: []
  property var spaces: []
  property var expanded: ({})
  // たたんだ分割のタブ(タブの id → true)
  property var collapsed: ({})
  property bool spacesOpen: false

  readonly property int barWidth: compact ? compactWidth : wideWidth
  readonly property color fg: Commons.Color.foreground
  readonly property color bg: Commons.Color.background
  readonly property color accent: Commons.Color.accent
  readonly property color dim: Commons.Color.muted
  readonly property int rowH: Style.space(26)
  readonly property int pad: Style.space(8)

  // Hyprland の窓の決まり(読めるまでは間も枠も無し)
  property var hypr: Model.hyprStyle("")
  readonly property int edge: win.side === "right" ? hypr.gapsOut.right : hypr.gapsOut.left
  // 隣の窓との間を窓と窓の間に合わせる。Hyprland は予約に端の余白(margins)を足し、その端から gaps_out だけ離して窓を置くので、
  // 予約は「幅 + 窓と窓の間 − gaps_out」にする
  readonly property int zone: Math.max(0, barWidth + 2 * hypr.gapsIn - edge)

  readonly property var tabRows: Model.tabRows(win.core, win.collapsed)
  readonly property var bookmarkRows: Model.bookmarkRows(win.core, win.bookmarks, win.folders, win.expanded)

  WlrLayershell.namespace: "prelude-sidebar"
  WlrLayershell.layer: WlrLayer.Top
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
  anchors {
    left: win.side !== "right"
    right: win.side === "right"
    top: true
    bottom: true
  }
  margins {
    top: win.hypr.gapsOut.top
    bottom: win.hypr.gapsOut.bottom
    left: win.side !== "right" ? win.hypr.gapsOut.left : 0
    right: win.side === "right" ? win.hypr.gapsOut.right : 0
  }
  implicitWidth: barWidth
  exclusiveZone: zone
  color: "transparent"

  // ---------------------------------------------------------------- コアとのやり取り

  // 命令を送る。引数は argv で渡すので、題名やパスがシェルに読まれることはない
  function send(cmd, args) {
    Util.execArgv(["prelude", "raw", cmd, JSON.stringify(args || {})])
    // 知らせの来ない命令(フォルダの開閉など)もあるので、少し後に読み直す
    refreshSoon.restart()
  }

  function run(command) {
    Util.execDetached(command)
  }

  function refresh() {
    if (fetch.running) {
      refreshSoon.restart()
      return
    }
    fetch.running = true
  }

  function readAll(text) {
    var parts = String(text || "").split("\u001e\n")
    var st = Model.parse(parts[0], null)
    win.online = !!st && typeof st === "object"
    win.core = win.online ? st : ({})
    win.bookmarks = Model.parse(parts[1], [])
    win.folders = Model.parse(parts[2], [])
    win.spaces = Model.parse(parts[3], [])
  }

  // 命令の名前は決まった文字だけ。使用者のデータはここに入らない
  Process {
    id: fetch
    command: ["bash", "-lc", "for c in state.get bookmark.list folder.list space.list; do prelude raw \"$c\" 2>/dev/null || echo null; printf '\\036\\n'; done"]
    stdout: StdioCollector { onStreamFinished: win.readAll(text) }
  }

  // 状態が変わるたびに 1 行来る。コアが止まっていれば終わるので、少し待ってつなぎ直す
  Process {
    id: watch
    command: ["bash", "-lc", "exec prelude watch"]
    running: true
    stdout: SplitParser { onRead: function(line) { refreshSoon.restart() } }
    onExited: {
      win.online = false
      reconnect.start()
    }
  }

  Timer {
    id: reconnect
    interval: 3000
    onTriggered: {
      watch.running = true
      win.refresh()
    }
  }

  Timer {
    id: refreshSoon
    interval: 120
    onTriggered: win.refresh()
  }

  // Hyprland の間・枠・角丸を読む(決まった命令だけ。使用者のデータは入らない)
  Process {
    id: hyprOpts
    command: ["bash", "-lc", "for o in general:gaps_out general:gaps_in general:border_size decoration:rounding general:col.inactive_border; do hyprctl getoption \"$o\" -j 2>/dev/null | tr -d '\\n'; echo; done"]
    stdout: StdioCollector { onStreamFinished: win.hypr = Model.hyprStyle(text) }
  }

  Component.onCompleted: {
    hyprOpts.running = true
    refresh()
  }

  // ---------------------------------------------------------------- 見た目の部品

  component Label: Text {
    textFormat: Text.PlainText
    color: win.fg
    font.family: win.fontFamily
    font.pixelSize: Style.font.body
    elide: Text.ElideRight
    verticalAlignment: Text.AlignVCenter
  }

  function toggleFold(tab) {
    var next = {}
    for (var k in win.collapsed) next[k] = win.collapsed[k]
    next[tab] = !next[tab]
    win.collapsed = next
  }

  // 1 行。左に印、右に題名。hover で薄く塗る。foldable なら印(▾・▸)を押すとたたむ/広げる
  component Row1: Rectangle {
    id: r
    property string mark: ""
    property bool foldable: false
    property string text: ""
    property string note: ""
    property bool active: false
    property bool lit: false
    property int depth: 0
    property bool closable: false
    signal clicked(var mouse)
    signal closeClicked()
    signal foldClicked()

    width: parent ? parent.width : 0
    height: win.rowH
    radius: Style.cornerRadius
    color: active ? Style.selectedFill : (hover.hovered ? Style.hoverFill : "transparent")

    // 行の上にある間ずっと真(× や印の上に移っても切れない。MouseArea の containsMouse は上の部品に移ると切れ、× が消えて押せなかった)
    HoverHandler { id: hover }

    MouseArea {
      id: area
      anchors.fill: parent
      hoverEnabled: true
      acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
      cursorShape: Qt.PointingHandCursor
      onClicked: function(mouse) { r.clicked(mouse) }
    }

    Label {
      id: markText
      x: win.compact ? 0 : win.pad + r.depth * win.pad * 1.5
      width: win.compact ? r.width : Style.space(22)
      height: r.height
      horizontalAlignment: win.compact ? Text.AlignHCenter : Text.AlignLeft
      text: r.mark
      font.bold: true
      font.pixelSize: Style.font.bodySmall
      color: r.active || r.lit ? win.accent : win.dim
      elide: Text.ElideNone
      MouseArea {
        visible: r.foldable && !win.compact
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: r.foldClicked()
      }
    }

    Label {
      visible: !win.compact
      anchors.left: markText.right
      anchors.right: noteText.left
      anchors.rightMargin: Style.space(4)
      height: r.height
      text: r.text
      opacity: r.active || r.lit ? 1 : 0.8
    }

    Label {
      id: noteText
      visible: !win.compact && text !== ""
      anchors.right: closeBox.visible ? closeBox.left : parent.right
      anchors.rightMargin: win.pad
      height: r.height
      text: r.note
      color: win.dim
      font.pixelSize: Style.font.caption
      width: visible ? implicitWidth : 0
    }

    Label {
      id: closeBox
      visible: !win.compact && r.closable && hover.hovered
      anchors.right: parent.right
      anchors.rightMargin: Style.space(4)
      width: visible ? Style.space(18) : 0
      height: r.height
      horizontalAlignment: Text.AlignHCenter
      text: "×"
      color: closeArea.containsMouse ? win.fg : win.dim
      MouseArea {
        id: closeArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: r.closeClicked()
      }
    }
  }

  component Heading: Label {
    visible: !win.compact
    width: parent ? parent.width : 0
    leftPadding: win.pad
    height: Style.space(22)
    font.pixelSize: Style.font.caption
    font.bold: true
    color: win.dim
  }

  // ---------------------------------------------------------------- 中身

  Rectangle {
    anchors.fill: parent
    color: win.bg
    radius: win.hypr.rounding
    border.width: win.hypr.borderSize
    border.color: win.hypr.borderColor || Style.normalBorderColor

    // 枠が無く、画面の端に付けているときは、隣の窓の側にだけ線を引く
    Rectangle {
      visible: win.hypr.borderSize === 0
      width: Style.spacing.hairline
      anchors.top: parent.top
      anchors.bottom: parent.bottom
      anchors.right: win.side === "right" ? undefined : parent.right
      anchors.left: win.side === "right" ? parent.left : undefined
      color: Style.normalBorderColor
    }

    // 上: スペースの名前(押すとスペースの一覧)、細い列の切り替え、閉じる
    Item {
      id: header
      anchors.top: parent.top
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.topMargin: Style.space(4)
      height: win.rowH + Style.space(4)

      Row1 {
        anchors.left: parent.left
        anchors.right: win.compact ? parent.right : buttons.left
        anchors.verticalCenter: parent.verticalCenter
        mark: win.compact ? (win.online ? "P" : "!") : (win.spacesOpen ? "▾" : "▸")
        text: win.online ? (win.core.space || "") : "コアにつながっていません"
        lit: true
        onClicked: function(mouse) {
          if (win.compact) win.compactToggled()
          else win.spacesOpen = !win.spacesOpen
        }
      }

      Row {
        id: buttons
        visible: !win.compact
        anchors.right: parent.right
        anchors.rightMargin: Style.space(4)
        anchors.verticalCenter: parent.verticalCenter
        Repeater {
          model: [{ t: "‹", f: "compact" }, { t: "×", f: "close" }]
          delegate: Label {
            required property var modelData
            width: Style.space(22)
            height: win.rowH
            horizontalAlignment: Text.AlignHCenter
            text: modelData.t
            color: btnArea.containsMouse ? win.fg : win.dim
            MouseArea {
              id: btnArea
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: modelData.f === "close" ? win.closeRequested() : win.compactToggled()
            }
          }
        }
      }
    }

    Flickable {
      id: scroller
      anchors.top: header.bottom
      anchors.bottom: footer.top
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.leftMargin: Style.space(4)
      anchors.rightMargin: Style.space(4)
      clip: true
      contentHeight: list.implicitHeight
      boundsBehavior: Flickable.StopAtBounds

      Column {
        id: list
        width: scroller.width
        spacing: Style.space(1)

        // スペースの一覧(上の名前を押したとき)
        Repeater {
          model: win.spacesOpen && !win.compact ? win.spaces : []
          delegate: Row1 {
            required property var modelData
            depth: 1
            mark: modelData === win.core.space ? "●" : "○"
            text: String(modelData)
            active: modelData === win.core.space
            // 一覧を閉じるとこの行は消える(その後は win も modelData も読めない)ので、送ってから閉じる
            onClicked: function(mouse) {
              var name = String(modelData)
              if (name !== win.core.space) win.send("space.switch", { name: name })
              win.spacesOpen = false
            }
          }
        }

        Heading { text: "タブ" }
        Label {
          visible: !win.compact && win.online && win.tabRows.length === 0
          leftPadding: win.pad
          height: win.rowH
          text: "タブはありません"
          color: win.dim
          font.pixelSize: Style.font.bodySmall
        }

        // タブ: 押すと切り替え、中クリック・× で閉じる。分割したタブはペインの行が続く
        Repeater {
          model: win.tabRows
          delegate: Row1 {
            required property var modelData
            readonly property bool isPane: modelData.row === "pane"
            visible: !(isPane && win.compact)
            height: visible ? win.rowH : 0
            readonly property bool isSplit: !isPane && modelData.panes > 1
            depth: isPane ? 1 : 0
            // 分割したタブは、種類の印の代わりにたたむ印(押すとペインの行をたたむ/広げる)
            foldable: isSplit
            mark: isPane ? modelData.mark : (win.compact ? String(modelData.num) : (isSplit ? (modelData.collapsed ? "▸" : "▾") : modelData.mark))
            text: modelData.title
            note: isPane ? "" : (isSplit && modelData.collapsed ? modelData.panes + " · " : "") + modelData.num
            active: modelData.active
            closable: true
            onClicked: function(mouse) {
              if (isPane) {
                if (mouse.button === Qt.MiddleButton) win.send("pane.close", { pane: modelData.id })
                else win.send("pane.focus", { pane: modelData.id })
              } else if (mouse.button === Qt.MiddleButton) win.send("tab.close", { tab: { id: modelData.id } })
              else if (mouse.button === Qt.RightButton) win.send("tab.duplicate", { tab: { id: modelData.id } })
              else win.send("tab.activate", { tab: { id: modelData.id } })
            }
            onFoldClicked: win.toggleFold(modelData.id)
            onCloseClicked: isPane ? win.send("pane.close", { pane: modelData.id }) : win.send("tab.close", { tab: { id: modelData.id } })
          }
        }

        Row1 {
          visible: !win.compact && (win.core.closed || 0) > 0
          height: visible ? win.rowH : 0
          mark: "↺"
          text: "閉じたタブを開き直す"
          onClicked: win.send("tab.reopen", {})
        }

        Item { width: 1; height: Style.space(8) }
        Heading { text: "ブックマーク" }
        Label {
          visible: !win.compact && win.online && win.bookmarkRows.length === 0
          leftPadding: win.pad
          height: win.rowH
          text: "ブックマークはありません"
          color: win.dim
          font.pixelSize: Style.font.bodySmall
        }

        // ブックマーク: 押すと開く(開いていればそのタブへ)。右クリックでフォルダを開く。フォルダは押すと開閉
        Repeater {
          model: win.bookmarkRows
          delegate: Row1 {
            required property var modelData
            readonly property bool isFolder: modelData.row === "folder"
            visible: !(win.compact && isFolder)
            height: visible ? win.rowH : 0
            depth: modelData.depth || 0
            mark: isFolder ? (modelData.expanded ? "▾" : "▸") : (win.compact && modelData.num ? "b" + modelData.num : modelData.mark)
            text: modelData.title
            note: isFolder ? String(modelData.count) : ""
            lit: isFolder ? modelData.hasOpen : modelData.open
            onClicked: function(mouse) {
              if (isFolder) {
                var next = {}
                for (var k in win.expanded) next[k] = win.expanded[k]
                next[modelData.path] = !modelData.expanded
                win.expanded = next
              } else if (mouse.button === Qt.RightButton) {
                win.send("tab.open", { pane: { type: "files", path: String(modelData.path).replace(/\/[^/]*$/, "") } })
              } else win.send("bookmark.open", { path: modelData.path })
            }
          }
        }
      }
    }

    // 下: 新しいタブ。種類は選ばせず、Omarchy のアプリの一覧に任せる(空いた作業領域で開き、出た窓をコアが引き取る。Super+Alt+Space と同じ)
    Column {
      id: footer
      anchors.bottom: parent.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.margins: Style.space(4)

      Row1 {
        mark: "+"
        text: "新しいタブ"
        onClicked: function(mouse) { win.run(win.newTab) }
      }
    }
  }
}
