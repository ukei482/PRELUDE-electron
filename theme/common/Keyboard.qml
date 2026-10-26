// 画面のキーボード(マウスだけで打つ)。キーボードが効かないときのため、入力メソッドなどに頼らず文字を直接渡す。
// 配列は US。押したキーは typed / backspace / enter で知らせ、入力欄へは使う側(Prompt.qml)が入れる。
import QtQuick

Rectangle {
    id: kb
    required property int fs
    required property string fontFamily
    property bool shift: false
    signal typed(string ch)
    signal backspace()
    signal enter()

    readonly property color blue: "#1926a3"
    readonly property real key: fs * 1.5 // キー1つの幅と高さ
    readonly property real gap: fs * 0.3
    readonly property var rows: [ // [ふつう, Shift]
        ["`1234567890-=", "~!@#$%^&*()_+"],
        ["qwertyuiop[]\\", "QWERTYUIOP{}|"],
        ["asdfghjkl;'", "ASDFGHJKL:\""],
        ["zxcvbnm,./", "ZXCVBNM<>?"]
    ]

    color: blue
    width: keys.width + fs * 2.4
    height: keys.height + fs * 2.4

    component Key: Rectangle { // 白い枠のキー。押している間は反転
        id: k
        property string label
        property bool on: false // Shift が効いている間など
        signal clicked()
        width: kb.key
        height: kb.key
        color: area.pressed || on ? "white" : kb.blue
        border.color: "white"
        border.width: 1
        Text {
            anchors.centerIn: parent
            text: k.label
            color: area.pressed || k.on ? kb.blue : "white"
            font.family: kb.fontFamily
            font.pixelSize: kb.fs
        }
        MouseArea { id: area; anchors.fill: parent; onClicked: k.clicked() }
    }

    Column {
        id: keys
        anchors.centerIn: parent
        spacing: kb.gap

        Repeater {
            model: kb.rows.length
            Row {
                id: line
                readonly property string chars: kb.rows[index][kb.shift ? 1 : 0]
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: kb.gap
                Repeater {
                    model: line.chars.length
                    Key {
                        label: line.chars.charAt(index)
                        onClicked: { kb.typed(label); kb.shift = false; }
                    }
                }
            }
        }

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: kb.gap
            Key { label: "shift"; width: kb.key * 3; on: kb.shift; onClicked: kb.shift = !kb.shift }
            Key { label: ""; width: kb.key * 4; onClicked: kb.typed(" ") } // スペース
            Key { label: "del"; width: kb.key * 2; onClicked: kb.backspace() }
            Key { label: "enter"; width: kb.key * 3; onClicked: kb.enter() }
        }
    }
}
