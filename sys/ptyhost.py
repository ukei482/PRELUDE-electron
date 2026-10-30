#!/usr/bin/env python3
# ホームの端末(sys/term.js)の中継(標準ライブラリの pty と名前がぶつからないよう ptyhost)。疑似端末(pty)を作ってコマンドを動かし、出力をそのまま標準出力へ流す。
# Node には pty が無く、node-pty はネイティブのモジュールでビルドが要るので、Python の pty で代わりにする。
#
#   ptyhost.py <cols> <rows> <コマンド> [引数...]
#
# 標準入力は枠に入れて受ける: 種類 1 バイト + 長さ 4 バイト(ビッグエンディアン) + 中身。
#   d = 端末への入力(キー・貼り付け)  r = 大きさの変更(中身は "cols rows")
# コマンドが終わったら、その終了コードで終わる。こちらが SIGTERM / SIGHUP を受けたら、コマンドに SIGHUP を送って終わる。
import errno, fcntl, os, pty, select, signal, struct, sys, termios


def set_size(fd, cols, rows):
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', max(1, rows), max(1, cols), 0, 0))
    except OSError:
        pass


def main():
    cols, rows, argv = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
    pid, master = pty.fork()
    if pid == 0:
        try:
            os.execvp(argv[0], argv)
        finally:
            os._exit(127)
    set_size(master, cols, rows)

    def hangup(*_):
        try:
            os.killpg(os.getpgid(pid), signal.SIGHUP)
        except OSError:
            pass
        os._exit(129)
    signal.signal(signal.SIGTERM, hangup)
    signal.signal(signal.SIGHUP, hangup)

    out = sys.stdout.buffer
    buf = b''
    stdin_open = True
    while True:
        fds = [master] + ([0] if stdin_open else [])
        try:
            ready, _, _ = select.select(fds, [], [])
        except InterruptedError:
            continue
        if master in ready:
            try:
                data = os.read(master, 65536)
            except OSError as e:
                if e.errno != errno.EIO:
                    raise
                data = b''
            if not data:  # コマンドが終わって pty が閉じた
                break
            out.write(data)
            out.flush()
        if 0 in ready:
            chunk = os.read(0, 65536)
            if not chunk:  # 親(PRELUDE)が居なくなった
                hangup()
            buf += chunk
            while len(buf) >= 5:
                kind, n = buf[0:1], struct.unpack('>I', buf[1:5])[0]
                if len(buf) < 5 + n:
                    break
                body, buf = buf[5:5 + n], buf[5 + n:]
                if kind == b'd':
                    while body:
                        body = body[os.write(master, body):]
                elif kind == b'r':
                    c, r = body.decode().split()
                    set_size(master, int(c), int(r))
    _, status = os.waitpid(pid, 0)
    sys.exit(os.waitstatus_to_exitcode(status) & 0xff)


main()
