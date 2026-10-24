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

    Rectangle { // CLI 領域(青地に白)
        anchors.centerIn: parent
        color: prompt.blue
        width: 26 * prompt.fs
        height: 9 * prompt.fs

        Rectangle { // 白地に青の PRELUDE(CLI 領域の色を反転)。角にくっつけると外側の白と続いて欠けて見えるので内側に寄せる
            x: prompt.fs * 0.6
            y: prompt.fs * 0.6
            color: "white"
            width: logo.implicitWidth + prompt.fs
            height: logo.implicitHeight + prompt.fs * 0.3
            Text {
                id: logo
                anchors.centerIn: parent
                text: "PRELUDE"
                color: prompt.blue
                font.family: vcr.name
                font.pixelSize: prompt.fs
            }
        }

        Row {
            anchors.centerIn: parent
            Text {
                text: "password: "
                color: "white"
                font.family: vcr.name
                font.pixelSize: prompt.fs
            }
            TextInput {
                id: password
                objectName: "password"
                width: 8 * prompt.fs // 「password: 」とこの幅を合わせた行を中央に置く。長いパスワードは中で横に流れる
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
                    width: prompt.fs * 0.55
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

        Text {
            id: message
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: prompt.fs * 0.6
            color: "white"
            font.family: vcr.name
            font.pixelSize: prompt.fs
        }
    }
}
