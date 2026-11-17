"""Join this process's standard input and output to a Unix bridge's socket file.

An ssh server too old to forward a socket file (OpenSSH before 6.7, as on a
NAO) still runs a command, so a client reaches the device over the command's
standard streams instead:

    ssh robot python relay.py /path/to/device.sock

Bytes pass both ways unchanged until either side closes. Python 2.6+ and 3,
standard library only.
"""

import errno
import os
import select
import socket
import sys


def main():
    if len(sys.argv) != 2:
        sys.stderr.write("usage: relay.py SOCKET\n")
        return 2
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.connect(sys.argv[1])
    except socket.error as e:
        sys.stderr.write("relay.py: cannot connect to %s: %s\n" % (sys.argv[1], e))
        return 1
    stdin, stdout = sys.stdin.fileno(), sys.stdout.fileno()
    reading = [stdin, sock]
    while sock in reading:
        try:
            ready = select.select(reading, [], [])[0]
        except (select.error, OSError) as e:
            if e.args[0] == errno.EINTR:
                continue
            raise
        if stdin in ready:
            data = os.read(stdin, 65536)
            if data:
                sock.sendall(data)
            else:
                # The client is done sending; keep relaying the device's answers.
                sock.shutdown(socket.SHUT_WR)
                reading.remove(stdin)
        if sock in ready:
            data = sock.recv(65536)
            if not data:
                break
            while data:
                data = data[os.write(stdout, data):]
    return 0


if __name__ == "__main__":
    sys.exit(main())
