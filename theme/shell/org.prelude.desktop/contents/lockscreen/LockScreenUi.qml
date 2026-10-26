// ロック画面の中身。見た目は Prompt.qml(theme/common と共通、SDDM と同じ)で、ここは kscreenlocker の認証とつなぐだけ。
import QtQuick

Prompt {
    id: ui

    actions: [
        { label: "sleep", visible: root.suspendToRamSupported, run: () => root.suspendToRam() },
        { label: "hibernate", visible: root.suspendToDiskSupported, run: () => root.suspendToDisk() },
    ]

    Connections {
        target: authenticator
        function onFailed(kind) {
            if (kind !== 0) return; // 指紋などパスワード以外の認証の失敗は出さない
            ui.say("access denied");
            ui.clear();
            graceTimer.restart();
        }
        function onSucceeded() {
            if (authenticator.hadPrompt) Qt.quit();
        }
        function onInfoMessageChanged() { if (authenticator.infoMessage) ui.say(authenticator.infoMessage); }
        function onErrorMessageChanged() { if (authenticator.errorMessage) ui.say(authenticator.errorMessage); }
        function onPromptForSecretChanged() {
            ui.busy = false;
            ui.focusInput();
        }
    }

    Connections {
        target: root
        function onClearPassword() { ui.clear(); }
    }

    Timer {
        id: graceTimer // 失敗したら少し待ってから次の入力を受ける(総当たりを遅くする)
        interval: 1500
        onTriggered: { ui.busy = false; authenticator.startAuthenticating(); }
    }

    onSubmitted: password => authenticator.respond(password)

    Component.onCompleted: {
        authenticator.startAuthenticating();
        ui.focusInput();
    }
}
