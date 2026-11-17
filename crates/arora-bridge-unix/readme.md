# arora-bridge-unix

A device's bridge over a Unix socket file, in Arora Buffers: an
[`arora_bridge::Bridge`](../arora-bridge/readme.md) on the device side
(`UnixBridge`) and a client (`UnixClient`). For clients on the device's
machine, in another container sharing the file, or at the end of an `ssh`
forward of the file.

- **No framing of its own.** Each message is one Arora Buffer, which starts
  with its own length: the stream is buffers back to back. Values travel in
  the binary form modules already exchange, with no text encoding.
- **Every operation.** `get`, `update`, `call`, `list_keys` and
  `describe_methods` (every `BridgeOp` but the deprecated `ListMethods`), with
  requests in flight together, each answered under its id.
- **Pushes by prefix.** A connection subscribes to key prefixes, which cover a
  subtree on a `/` boundary as a store's prefixes do. It starts subscribed to
  nothing, so a client that only commands is sent nothing, and the device
  copies no change while no connection subscribes.
- **Access is the file's.** The socket is created `0600`: only its owner
  connects, locally or through `ssh`. A device's writes are then checked like
  every bridge's: a key is closed to remote writers unless its meta opens it.
- **Nothing on the step's thread but a copy.** The step hands each change to
  the connections' tasks, which encode and write it.

The crate documentation describes the wire format message by message, for a
client in another language.

## Serve a device

```rust,ignore
let bridge = arora_bridge_unix::UnixBridge::bind("/run/user/1000/arora/robot.sock")?;
arora::Arora::builder()
    .with_hal(hal)
    .with_bridge(Box::new(bridge))
    .run()
    .await?;
```

`bind` creates the file's directory, replaces a file a crashed device left
(one nothing answers on), and fails with `AddrInUse` when a device answers
there. Dropping the bridge stops its connections and removes the file.

## Drive it

```rust,ignore
use arora_bridge::BridgeOp;
use arora_types::{data::StateChange, value::Value};
use futures::StreamExt;

let (client, mut changes) = arora_bridge_unix::UnixClient::connect("/tmp/robot.sock").await?;
client.subscribe(vec!["HeadYaw".into()]).await?;
client
    .ask(BridgeOp::Update(StateChange::set("HeadYaw/target.position", Value::F64(0.3))))
    .await?;
while let Some(change) = changes.next().await {
    println!("{change:?}");
}
```

`UnixClient` implements `arora_bridge::Caller`, so code written against an
in-process `arora::LocalCaller` drives a remote device unchanged.

## Through ssh

OpenSSH forwards a socket file to a socket file. On the client's machine:

```sh
ssh -N -o StreamLocalBindUnlink=yes \
    -L /tmp/robot.sock:/run/user/1000/arora/robot.sock robot
```

`/tmp/robot.sock` is then the device's bridge, reached as the user `ssh` logs
in as; `-J relay` goes through other hosts on the way. The device's sshd needs
OpenSSH 6.7 or later and `AllowStreamLocalForwarding` (on by default).

An older sshd, such as a NAO's OpenSSH 5.9, forwards no socket file but runs
a command. [`relay.py`](relay.py) (Python 2.6+ or 3, standard library only,
blocked in `select` while idle) joins its standard streams to the socket file,
and the client speaks over the command's:

```rust,ignore
let mut ssh = tokio::process::Command::new("ssh")
    .args(["robot", "/usr/bin/python", "relay.py", "/run/user/1000/arora/robot.sock"])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .kill_on_drop(true)
    .spawn()?;
let (client, changes) = arora_bridge_unix::UnixClient::over(
    ssh.stdout.take().unwrap(),
    ssh.stdin.take().unwrap(),
);
```

A client in another language does the same with its own process API, and
`socat UNIX-LISTEN:/tmp/robot.sock,fork EXEC:'ssh robot python relay.py …'`
gives such a robot a local socket file again.
