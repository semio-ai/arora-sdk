mod arora_generated;

use serde_json::json;
use std::sync::{Mutex, OnceLock};
use vizij_orchestrator::{VizijModuleFacade, MODULE_FACADE_VERSION};

static FACADE: OnceLock<Mutex<VizijModuleFacade>> = OnceLock::new();

fn facade() -> &'static Mutex<VizijModuleFacade> {
    FACADE.get_or_init(|| Mutex::new(VizijModuleFacade::new()))
}

fn dispatch_json(request_json: Option<String>) -> String {
    let request_json = match request_json {
        Some(request_json) => request_json,
        None => {
            return json!({
              "ok": false,
              "error": "missing request_json",
              "version": MODULE_FACADE_VERSION,
            })
            .to_string()
        }
    };

    match facade().lock() {
        Ok(mut guard) => guard.dispatch_json(&request_json),
        Err(_) => json!({
          "ok": false,
          "error": "vizij orchestrator facade lock is poisoned",
          "version": MODULE_FACADE_VERSION,
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn unwrap_result(response: &str) -> Value {
        let parsed: Value = serde_json::from_str(response).expect("response json");
        assert_eq!(parsed["ok"], true, "{parsed}");
        parsed["result"].clone()
    }

    fn call(name: &str, args: Value) -> Value {
        unwrap_result(&dispatch_json(Some(
            json!({
              "call": name,
              "requestId": format!("req:{name}"),
              "args": args,
            })
            .to_string(),
        )))
    }

    fn fixture_graph() -> Value {
        json!({
          "nodes": [
            {
              "id": "source",
              "type": "constant",
              "params": { "value": { "type": "float", "data": 3.0 } }
            },
            {
              "id": "out",
              "type": "output",
              "params": { "path": "facade/arora.value" }
            }
          ],
          "edges": [
            {
              "from": { "node_id": "source", "output": "out" },
              "to": { "node_id": "out", "input": "in" }
            }
          ]
        })
    }

    #[test]
    fn module_dispatches_stateful_facade_calls() {
        let runtime = call("runtime.create", json!({ "schedule": "SinglePass" }));
        assert_eq!(runtime["runtimeHandle"], "runtime:0");

        let graph = call(
            "graph.register",
            json!({ "id": "graph:arora", "spec": fixture_graph() }),
        );
        assert_eq!(graph["graphId"], "graph:arora");

        let frame = call("orchestrator.step", json!({ "dt": 1.0 / 60.0 }));
        let writes = frame["merged_writes"].as_array().expect("writes array");
        assert!(
            writes
                .iter()
                .any(|write| write["path"] == "facade/arora.value"),
            "facade write missing: {writes:?}"
        );
    }
}
