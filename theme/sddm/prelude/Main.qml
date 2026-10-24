// PRELUDE のログイン画面(SDDM のテーマ)。見た目は Prompt.qml(theme/common と共通、ロック画面と同じ)。
// ユーザーは1人の前提で名前は聞かない。前回のユーザー(無ければ一覧の先頭)・前回のセッションで入る。
// sddm・userModel・sessionModel は SDDM がコンテキストに入れるもの。
import QtQuick

Prompt {
    id: ui
    property string firstUser: ""

    Instantiator { // 一覧の先頭のユーザー名を拾う(初回起動で lastUser が空のとき用)
        model: userModel
        delegate: QtObject {
            Component.onCompleted: if (index === 0) ui.firstUser = model.name
        }
    }

    Connections {
        target: sddm
        function onLoginFailed() {
            ui.say("access denied");
            ui.clear();
            ui.busy = false;
            ui.focusInput();
        }
        function onInformationMessage(message) { ui.say(message); }
    }

    onSubmitted: password => sddm.login(userModel.lastUser || ui.firstUser, password, sessionModel.lastIndex)

    Component.onCompleted: ui.focusInput()
}
