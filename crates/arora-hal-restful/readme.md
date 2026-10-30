# arora-hal-restful

A HAL ([`arora-hal`](../arora-hal/readme.md)) that drives a robot over its HTTP
API: a write to a joint's target key becomes a request to the robot's
endpoint. The configuration describes the robot's API; there are no built-in
robots.

The crate is not published on crates.io: build it from this workspace.

## Run a robot

The `arora-restful` binary runs a robot as an Arora device from its
configuration file:

```sh
cargo run -p arora-hal-restful --features runner --bin arora-restful -- \
    crates/arora-hal-restful/configs/hackerbot.json
```

[`configs/hackerbot.json`](configs/hackerbot.json) is the sample: a Hackerbot
on a local address — change `base_url` to your robot's. The device serves the
open local bridge on `ws://127.0.0.1:9000`
([`arora-bridge-ws`](../arora-bridge-ws/README.md)).

## How keys become requests

`RestfulHal::new(config)` builds the HAL from a `RESTfulRobotConfig`: the
robot's `base_url` and its `endpoints`, each a `path`, an HTTP `method` and an
`EndpointMapping`.

A `POST` endpoint mapped as `JointPositions` commands joints. Its
`joint_mapping` maps each joint's Arora name to the API's name for it. A write
of a joint's position target — `<joint>/target.position` or
`<joint>.target_position`, the HALs' target keys (see
[arora-hal](../arora-hal/readme.md#joint-targets)) — becomes one request per
such endpoint, at the speed a velocity target (`<joint>/target.velocity` or
`<joint>.target_velocity`) in the same change sets. Positions are in radians.

The API reports no readings, so the HAL senses back what it commanded: a
request the robot accepts mirrors each target as `<joint>.position` and
`<joint>.velocity`. Every key is held as written, both target forms included.

The HAL describes, for each joint of a `POST` `JointPositions` endpoint, the
four target keys as `F64` inputs, open to remote writers, and
`<joint>.position` and `<joint>.velocity` as `F64` outputs. A device over it
opens its inputs with no configuration of its own.

Writes never block the device's step: `try_send` hands them to a task that
merges any backlog (the newer target wins) and sends the requests. A request
failing on some endpoints is logged; a write fails only when every endpoint it
reached failed.

## Limits

- Only `POST` `JointPositions` endpoints send requests, with the Hackerbot's
  payload. The `Twist`, `Navigation` and `Custom` mappings, and `GET`
  endpoints, are accepted by the configuration and send nothing.
- HTTPS trusts the bundled webpki roots only, not the operating system's
  store, so a robot behind a certificate from a private authority is
  refused.

## Reference

```text
arora-restful [--module <DIR>]... [--device <NAME>] [--device-dir <PATH>]
              [--bridge-port <PORT>] [--no-studio] <CONFIG> [OVERRIDES]
```

| Argument | Environment | Meaning |
|---|---|---|
| `<CONFIG>` | `CONFIG` | The robot's `RESTfulRobotConfig` JSON file. |
| `[OVERRIDES]` | — | A JSON file whose set fields override the configuration. |
| `--module <DIR>` | — | A module directory to load, besides the device directory's `modules/`. Repeatable. |
| `--device <NAME>` | `ARORA_DEVICE` | The device's name on this machine (`default` when unset). |
| `--device-dir <PATH>` | `ARORA_DEVICE_DIR` | The device directory, instead of the per-user directory of its name. |
| `--bridge-port <PORT>` | `ARORA_BRIDGE_PORT` | The open local bridge's port (9000 when unset). |
| `--no-studio` | — | Only in a build with the `studio` feature: serve the open local bridge instead of the device's Semio Studio bridge. |

| Feature | Effect |
|---|---|
| `runner` | The `arora-restful` binary. The library needs none of it. |
| `studio` | Opt-in Semio Studio connection for the runner: a run serves the device's Studio bridge unless it passes `--no-studio`. The device runs at once; one that is not enrolled with Studio enrolls while it runs, by the token in `STUDIO_ENROLLMENT_TOKEN` or a code the run shows. |

As a library, `RestfulHal::new(config)` must be called inside a Tokio runtime
that outlives the HAL.
