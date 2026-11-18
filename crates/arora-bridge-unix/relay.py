"""Join this process's standard input and output to a Unix bridge's socket file.

An ssh server too old to forward a socket file (OpenSSH before 6.7, as on a
NAO) still runs a command, so a client reaches the device over the command's
standard streams instead:

    ssh robot python relay.py /path/to/device.sock

Bytes pass both ways unchanged until either side closes. Each way has its own
buffer, written as its destination takes it, so a side that is slow to read
holds back only what is sent to it: the relay keeps reading the other way.
When the client's input ends, the device is told it will be sent nothing more
and is still relayed until it closes. Python 2.6+ and 3, standard library
only.
"""

import errno
import fcntl
import os
import select
import socket
import sys

# A buffer stops being read into past this many bytes, until it drains.
LIMIT = 1 << 20
AGAIN = (errno.EAGAIN, errno.EWOULDBLOCK, errno.EINTR)
# The device's end closed: nothing more goes to it or comes from it.
GONE = (errno.ECONNRESET, errno.EPIPE, errno.ENOTCONN)


def nonblocking(fd):
    fcntl.fcntl(fd, fcntl.F_SETFL, fcntl.fcntl(fd, fcntl.F_GETFL) | os.O_NONBLOCK)


class Way(object):
    """Bytes read from one side and not yet written to the other."""

    def __init__(self):
        self.chunks = []
        self.size = 0
        self.open = True

    def add(self, data):
        self.chunks.append(data)
        self.size += len(data)

    def drop(self):
        """Discard what waits, and read no more into it."""
        self.chunks = []
        self.size = 0
        self.open = False

    def written(self, n):
        self.size -= n
        first = self.chunks[0][n:]
        if first:
            self.chunks[0] = first
        else:
            self.chunks.pop(0)


def attempt(call, *args):
    """Call, or None when it would block or was interrupted."""
    try:
        return call(*args)
    except (socket.error, OSError, IOError) as e:
        if e.args[0] in AGAIN:
            return None
        raise


def device(call, *args):
    """attempt() on the device's socket, or False when its end is gone."""
    try:
        return attempt(call, *args)
    except (socket.error, OSError, IOError) as e:
        if e.args[0] in GONE:
            return False
        raise


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
    sock.setblocking(0)
    nonblocking(stdin)
    nonblocking(stdout)
    # What the client sent, for the device; what the device sent, for the client.
    up, down = Way(), Way()
    shut = False
    while down.open or down.size:
        if not up.open and not up.size and not shut:
            # The client is done sending; keep relaying the device's answers.
            device(sock.shutdown, socket.SHUT_WR)
            shut = True
        reading, writing = [], []
        if up.open and up.size < LIMIT:
            reading.append(stdin)
        if down.open and down.size < LIMIT:
            reading.append(sock)
        if up.size:
            writing.append(sock)
        if down.size:
            writing.append(stdout)
        try:
            readable, writable = select.select(reading, writing, [])[:2]
        except (select.error, OSError) as e:
            if e.args[0] == errno.EINTR:
                continue
            raise
        if stdin in readable:
            data = attempt(os.read, stdin, 65536)
            if data:
                up.add(data)
            elif data is not None:
                up.open = False
        if sock in readable:
            data = device(sock.recv, 65536)
            if data:
                down.add(data)
            elif data is not None:
                # The device closed (or reset): relay what it sent, send it
                # nothing more.
                down.open = False
                up.drop()
        if sock in writable and up.size:
            n = device(sock.send, up.chunks[0])
            if n is False:
                # The device stopped reading: send it nothing more, and read
                # what it sent until its end says so.
                up.drop()
            elif n:
                up.written(n)
        if stdout in writable:
            n = attempt(os.write, stdout, down.chunks[0])
            if n:
                down.written(n)
    return 0


if __name__ == "__main__":
    sys.exit(main())
