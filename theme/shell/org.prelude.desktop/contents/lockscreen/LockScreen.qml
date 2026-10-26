// PRELUDE のロック画面。kscreenlocker_greet が読み込む(シェルのパッケージの lockscreen/LockScreen.qml)。
// 白い画面の中央に小さな表示領域を置き、左上に PRELUDE、真ん中に password: を出す。SDDM のテーマと同じ見た目。
// authenticator・kscreenlocker_userName は greeter がコンテキストに入れるもの。
import QtQuick

Item {
    id: root
    property bool debug: false
    property string notification
    signal clearPassword()
    signal notificationRepeated()

    // kscreenlocker が探すプロパティとシグナル(名前を変えない)
    property bool viewVisible: false
    property bool suspendToRamSupported: false
    property bool suspendToDiskSupported: false
    signal suspendToDisk()
    signal suspendToRam()

    implicitWidth: 800
    implicitHeight: 600

    LockScreenUi {
        anchors.fill: parent
    }
}
