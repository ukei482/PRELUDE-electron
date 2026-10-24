// ロック画面と SDDM で共通の入力画面。白い画面の中央に青い CLI 領域を置き、左上に PRELUDE(白地に青)、真ん中に password: を出す。
// 認証はしない。入力が確定したら submitted を出すので、使う側(LockScreenUi.qml / SDDM の Main.qml)が認証に渡す。
// 右上のメニュー(三本線)には、画面のキーボード(Keyboard.qml)と、使う側が actions で渡す操作(スリープなど)を出す。
// キーボードが効かなくなってもマウスだけで入れるようにするためのもの。
// 書体は同じフォルダの VCR_OSD_MONO_1.001.ttf(theme/common から各パッケージへリンク・コピーする)。
import QtQuick

Rectangle {
    id: prompt
    readonly property color blue: "#1926a3"
    readonly property int fs: 20 // PRELUDE と password: は同じ大きさ
    property bool busy: false // 認証の結果を待っている間・失敗の直後は入力させない
    property var actions: [] // メニューに足す操作: [{ label, run, visible }]
    property bool menuOpen: false
    property bool keyboardVisible: false
    property bool pointerShown: false // マウスを動かすまでポインタは隠す
    signal submitted(string password)

    readonly property var menuItems: [{ label: keyboardVisible ? "hide keyboard" : "keyboard", run: () => keyboardVisible = !keyboardVisible }]
        .concat(actions.filter(a => a.visible !== false))

    function say(msg) { message.text = msg; messageTimer.restart(); }
    function clear() { password.text = ""; }
    function focusInput() { password.forceActiveFocus(); }
    function submit() {
        if (!password.text || busy) return;
        busy = true;
        submitted(password.text);
    }

    objectName: "prompt"
    color: "white"

    FontLoader { id: vcr; source: Qt.resolvedUrl("VCR_OSD_MONO_1.001.ttf") }
    Timer { id: messageTimer; interval: 3000; onTriggered: message.text = "" }

    MouseArea { // 外をクリックしたらメニューを閉じる。入力欄からフォーカスは外さない
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: prompt.pointerShown ? Qt.ArrowCursor : Qt.BlankCursor
        onPositionChanged: prompt.pointerShown = true
        onPressed: { prompt.menuOpen = false; password.forceActiveFocus(); }
    }

    Rectangle { // CLI 領域(青地に白)。正方形で固定
        id: cli
        anchors.centerIn: parent
        anchors.verticalCenterOffset: prompt.keyboardVisible ? -(keyboard.height + prompt.fs) / 2 : 0 // キーボードを出したら、合わせて画面の中央に来るよう上へ
        color: prompt.blue
        width: 18 * prompt.fs
        height: width
        readonly property real pad: prompt.fs * 1.2 // 縁からの余白

        Rectangle { // 白地に青の PRELUDE(CLI 領域の色を反転)。角にくっつけると外側の白と続いて欠けて見えるので内側に寄せる
            id: logoBox
            x: parent.pad
            y: parent.pad
            color: "white"
            width: logo.implicitWidth + prompt.fs * 1.2
            height: logo.implicitHeight + prompt.fs * 0.6
            Text {
                id: logo
                anchors.centerIn: parent
                text: "PRELUDE"
                color: prompt.blue
                font.family: vcr.name
                font.pixelSize: prompt.fs
            }
        }

        Column { // password: の次の行に * を出す。どの行も CLI 領域の中で中央にそろえる
            id: lines
            anchors.centerIn: parent
            width: parent.width - 2 * parent.pad
            spacing: prompt.fs * 0.5

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "password:"
                color: "white"
                font.family: vcr.name
                font.pixelSize: prompt.fs
            }
            Item {
                width: parent.width
                height: password.height
                TextInput {
                    id: password
                    objectName: "password"
                    readonly property real cursorWidth: prompt.fs * 0.55
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(contentWidth + cursorWidth, parent.width) // 打った分だけ広げて中央に保つ。領域より長いと中で横に流れる
                    color: "white"
                    font.family: vcr.name
                    font.pixelSize: prompt.fs
                    echoMode: TextInput.Password
                    passwordCharacter: "*"
                    passwordMaskDelay: 0
                    maximumLength: 256
                    clip: true
                    focus: true
                    enabled: !prompt.busy
                    cursorDelegate: Rectangle { // 点滅する四角のカーソル
                        width: password.cursorWidth
                        color: "white"
                        visible: password.activeFocus && !prompt.busy
                        SequentialAnimation on opacity {
                            loops: Animation.Infinite
                            NumberAnimation { to: 1; duration: 0 }
                            PauseAnimation { duration: 500 }
                            NumberAnimation { to: 0; duration: 0 }
                            PauseAnimation { duration: 500 }
                        }
                    }
                    Keys.onEscapePressed: event => { text = ""; event.accepted = false; } // Esc はロック画面では画面を消すのに使われる
                    onAccepted: prompt.submit()
                }
            }
        }

        Text { // 出ても password: の位置が動かないよう、Column の外に置く
            id: message
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: lines.bottom
            anchors.topMargin: prompt.fs
            color: "white"
            font.family: vcr.name
            font.pixelSize: prompt.fs
        }

        Rectangle { // メニューのボタン(三本線)。PRELUDE と同じ行の右端。開いている間は反転
            id: menuButton
            anchors.right: parent.right
            anchors.rightMargin: parent.pad
            y: logoBox.y
            width: logoBox.height
            height: logoBox.height
            color: prompt.menuOpen || menuArea.containsMouse ? "white" : prompt.blue
            Column {
                anchors.centerIn: parent
                spacing: prompt.fs * 0.2
                Repeater {
                    model: 3
                    Rectangle { width: prompt.fs * 0.9; height: Math.max(2, Math.round(prompt.fs * 0.12)); color: menuButton.color === prompt.blue ? "white" : prompt.blue }
                }
            }
            MouseArea {
                id: menuArea
                anchors.fill: parent
                hoverEnabled: true
                onClicked: prompt.menuOpen = !prompt.menuOpen
            }
        }

        Rectangle { // メニュー。白地に青、指している項目は反転
            visible: prompt.menuOpen
            z: 1
            anchors.right: menuButton.right
            anchors.top: menuButton.bottom
            width: items.width
            height: items.height
            color: "white"
            Column {
                id: items
                Repeater {
                    model: prompt.menuItems
                    Rectangle {
                        width: Math.max(items.widest, label.implicitWidth + prompt.fs * 1.2)
                        height: label.implicitHeight + prompt.fs * 0.6
                        color: itemArea.containsMouse ? prompt.blue : "white"
                        Component.onCompleted: items.widest = Math.max(items.widest, label.implicitWidth + prompt.fs * 1.2)
                        Text {
                            id: label
                            x: prompt.fs * 0.6
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label
                            color: itemArea.containsMouse ? "white" : prompt.blue
                            font.family: vcr.name
                            font.pixelSize: prompt.fs
                        }
                        MouseArea {
                            id: itemArea
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: {
                                const run = modelData.run; // run で menuItems が変わるとこの項目が作り直されるので、先に取り出して最後に呼ぶ
                                prompt.menuOpen = false;
                                password.forceActiveFocus();
                                run();
                            }
                        }
                    }
                }
                property real widest: 0
            }
        }
    }

    Keyboard {
        id: keyboard
        visible: prompt.keyboardVisible
        anchors.horizontalCenter: cli.horizontalCenter
        anchors.top: cli.bottom
        anchors.topMargin: prompt.fs
        fs: prompt.fs
        fontFamily: vcr.name
        onTyped: ch => { if (!prompt.busy) password.insert(password.cursorPosition, ch); }
        onBackspace: if (!prompt.busy && password.cursorPosition > 0) password.remove(password.cursorPosition - 1, password.cursorPosition)
        onEnter: prompt.submit()
    }
}
