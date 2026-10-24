// ロック画面と SDDM で共通の入力画面。白い画面の中央に青い CLI 領域を置き、左上に PRELUDE(白地に青)、真ん中に password: を出す。
// 認証はしない。入力が確定したら submitted を出すので、使う側(LockScreenUi.qml / SDDM の Main.qml)が認証に渡す。
// 書体は同じフォルダの VCR_OSD_MONO_1.001.ttf(theme/common から各パッケージへリンク・コピーする)。
import QtQuick

Rectangle {
    id: prompt
    readonly property color blue: "#1926a3"
    readonly property int fs: 20 // PRELUDE と password: は同じ大きさ
    property bool busy: false // 認証の結果を待っている間・失敗の直後は入力させない
    signal submitted(string password)

    function say(msg) { message.text = msg; messageTimer.restart(); }
    function clear() { password.text = ""; }
    function focusInput() { password.forceActiveFocus(); }

    color: "white"

    FontLoader { id: vcr; source: Qt.resolvedUrl("VCR_OSD_MONO_1.001.ttf") }
    Timer { id: messageTimer; interval: 3000; onTriggered: message.text = "" }

    MouseArea { // ポインタは隠す。クリックしても入力欄からフォーカスを外さない
        anchors.fill: parent
        cursorShape: Qt.BlankCursor
        onPressed: password.forceActiveFocus()
    }

    Rectangle { // CLI 領域(青地に白)。正方形で固定
        anchors.centerIn: parent
        color: prompt.blue
        width: 18 * prompt.fs
        height: width
        readonly property real pad: prompt.fs * 1.2 // 縁からの余白

        Rectangle { // 白地に青の PRELUDE(CLI 領域の色を反転)。角にくっつけると外側の白と続いて欠けて見えるので内側に寄せる
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
                    onAccepted: {
                        if (!text || prompt.busy) return;
                        prompt.busy = true;
                        prompt.submitted(text);
                    }
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
    }
}
