#!/usr/bin/env python3
"""ロック画面の QML を画面に出さずに描画して PNG にする(実機をロックせずに見た目を確かめる)。

  python3 theme/preview.py out.png [幅 高さ] [--fail]

--fail を付けると、パスワードを間違えたあとの画面を描く。
authenticator は偽物で、どのパスワードでも失敗する。
"""
import os
import sys

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

from PyQt6.QtCore import QMetaObject, QObject, QTimer, QUrl, pyqtProperty, pyqtSignal, pyqtSlot
from PyQt6.QtGui import QGuiApplication
from PyQt6.QtQuick import QQuickView

HERE = os.path.dirname(os.path.abspath(__file__))
MAIN = os.path.join(HERE, 'shell/org.prelude.desktop/contents/lockscreen/LockScreen.qml')


class FakeAuthenticator(QObject):
    failed = pyqtSignal(int)
    succeeded = pyqtSignal()
    infoMessageChanged = pyqtSignal()
    errorMessageChanged = pyqtSignal()
    promptChanged = pyqtSignal()
    promptForSecretChanged = pyqtSignal()

    @pyqtProperty(bool, constant=True)
    def hadPrompt(self): return True

    @pyqtProperty(str, notify=infoMessageChanged)
    def infoMessage(self): return ''

    @pyqtProperty(str, notify=errorMessageChanged)
    def errorMessage(self): return ''

    @pyqtProperty(str, notify=promptChanged)
    def prompt(self): return ''

    @pyqtSlot()
    def startAuthenticating(self): QTimer.singleShot(0, self.promptForSecretChanged.emit)

    @pyqtSlot(str)
    def respond(self, _password): QTimer.singleShot(0, lambda: self.failed.emit(0))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    out = args[0] if args else 'lockscreen.png'
    w, h = (int(args[1]), int(args[2])) if len(args) >= 3 else (1280, 720)
    app = QGuiApplication(sys.argv)
    auth = FakeAuthenticator()
    view = QQuickView()
    ctx = view.rootContext()
    ctx.setContextProperty('authenticator', auth)
    ctx.setContextProperty('kscreenlocker_userName', os.environ.get('USER', 'user'))
    view.setResizeMode(QQuickView.ResizeMode.SizeRootObjectToView)
    view.resize(w, h)
    view.setSource(QUrl.fromLocalFile(MAIN))
    if view.status() != QQuickView.Status.Ready:
        for e in view.errors(): print(e.toString(), file=sys.stderr)
        sys.exit(1)
    view.show()

    def typed():
        pw = view.rootObject().findChild(QObject, 'password')
        if pw: pw.setProperty('text', 'hunter2')
        if '--fail' in sys.argv and pw: QTimer.singleShot(0, lambda: QMetaObject.invokeMethod(pw, 'accepted'))

    def grab():
        view.grabWindow().save(out)
        print(out)
        app.quit()

    QTimer.singleShot(300, typed)
    QTimer.singleShot(900, grab)
    app.exec()


if __name__ == '__main__':
    main()
