//! WebSocket message types bridging the Arora API.
//!
//! Messages are serialized as JSON with a `type` field discriminator, and
//! speak the Arora data-layer vocabulary: values are written to and read from
//! **keys** (hierarchical paths into the store).

use arora_bridge::client::KeyInfo;
use arora_bridge::client::MethodInfo;
use arora_bridge::MethodSignature;
use arora_types::call::{Call, CallResult};
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Messages received from WebSocket clients.
///
/// All incoming messages use a `type` field to discriminate the message kind.
/// Example JSON: `{"type": "write_values", "values": {...}}`
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Incoming {
    /// Write values to keys.
    ///
    /// Example: `{"type": "write_values", "values": {"face/mouth": {"f64": 0.5}}}`
    WriteValues {
        /// Map of key paths to their new values
        values: HashMap<String, Value>,
    },

    /// Read the current values of keys.
    ///
    /// Example: `{"type": "read_values", "keys": ["face/mouth", "face/eyes"]}`
    ReadValues {
        /// List of key paths to retrieve
        keys: Vec<String>,
    },

    /// Choose the keys this connection is pushed
    /// [`ValuesChanged`](Outgoing::ValuesChanged) for. Absent `keys` means every
    /// key, which is where a connection starts — so a client that never
    /// subscribes sees the whole feed, and one that does sees only what it asked
    /// for. Nothing about a key makes it special: a client that wants the clock
    /// subscribes to it.
    ///
    /// Example: `{"type": "subscribe", "keys": ["face/mouth"]}`
    Subscribe {
        /// The keys to receive pushes for; absent or null means all of them.
        #[serde(default)]
        keys: Option<Vec<String>>,
    },

    /// Request the list of available keys.
    ///
    /// Example: `{"type": "list_keys"}` or `{"type": "list_keys", "path": "face"}`
    ListKeys {
        /// Optional path prefix to filter keys
        #[serde(default)]
        path: Option<String>,
    },

    /// Request the list of available RPC methods.
    ///
    /// Example: `{"type": "list_methods"}` or `{"type": "list_methods", "path": "audio"}`
    ListMethods {
        /// Optional path prefix to filter methods
        #[serde(default)]
        path: Option<String>,
    },

    /// Describe the device's callable functions with their full signatures:
    /// the module and function ids a [`Call`](Incoming::Call) names, the
    /// parameters by id, their order, and the return type.
    ///
    /// Example: `{"type": "describe_methods"}` or `{"type": "describe_methods", "path": "say"}`
    DescribeMethods {
        /// Optional name prefix to filter methods
        #[serde(default)]
        path: Option<String>,
    },

    /// Invoke an RPC method.
    ///
    /// Example: `{"type": "invoke", "method": "reset", "request_id": "req-1"}`
    Invoke {
        /// Method path/name to invoke
        method: String,

        /// Arguments as key-value pairs
        #[serde(default)]
        args: HashMap<String, Value>,

        /// Optional request ID for correlating responses
        #[serde(default)]
        request_id: Option<String>,
    },

    /// Call a function by ids: an [`arora_types::call::Call`], its fields
    /// beside the message's own. The device makes it as it makes an in-process
    /// caller's — whatever function it names, described or not — and answers
    /// with the [`CallResult`].
    ///
    /// Example: `{"type": "call", "module_id": "…", "id": "…", "args": [{"id": "…", "value": {"f64": 2.0}}], "request_id": "req-3"}`
    Call {
        /// The call: `module_id`, `id`, and `args` by parameter id.
        #[serde(flatten)]
        call: Call,

        /// Optional request ID for correlating responses
        #[serde(default)]
        request_id: Option<String>,
    },

    /// Stop a run started by invoking a task-shaped method, named by the run id
    /// its handle carried.
    ///
    /// Example: `{"type": "halt", "run": "0195...", "request_id": "req-2"}`
    Halt {
        /// The run's id, as the invoke's task handle gave it.
        run: String,

        /// Optional request ID for correlating responses
        #[serde(default)]
        request_id: Option<String>,
    },
}

/// Messages sent to WebSocket clients.
///
/// All outgoing messages use a `type` field to discriminate the message kind.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outgoing {
    /// Response to WriteValues message.
    ///
    /// Example: `{"type": "write_values_resp", "success": true}`
    WriteValuesResp {
        /// Whether the write was successful
        success: bool,

        /// Error message if success is false
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Response to ReadValues message.
    ///
    /// Example: `{"type": "read_values_resp", "values": {"face/mouth": {"f64": 0.5}}}`
    ReadValuesResp {
        /// Map of key paths to their current values
        values: HashMap<String, Value>,
    },

    /// Response to Subscribe: the keys now pushed to this connection, or null
    /// for all of them.
    ///
    /// Example: `{"type": "subscribe_resp", "keys": ["face/mouth"]}`
    SubscribeResp {
        /// The effective subscription; null means every key.
        keys: Option<Vec<String>>,
    },

    /// Response to ListKeys message.
    ///
    /// Example: `{"type": "list_keys_resp", "keys": [...]}`
    ListKeysResp {
        /// List of matching keys
        keys: Vec<KeyInfo>,
    },

    /// Response to ListMethods message.
    ///
    /// Example: `{"type": "list_methods_resp", "methods": [...]}`
    ListMethodsResp {
        /// List of matching methods
        methods: Vec<MethodInfo>,
    },

    /// Response to DescribeMethods message.
    ///
    /// Example: `{"type": "describe_methods_resp", "methods": [{"module_id": "…", "id": "…", "name": "say", "function": {…}}]}`
    DescribeMethodsResp {
        /// The matching methods, each with its full signature
        methods: Vec<MethodSignature>,
    },

    /// Response to Invoke message.
    ///
    /// Example: `{"type": "invoke_resp", "success": true, "request_id": "req-1"}`
    InvokeResp {
        /// Whether the invocation was successful
        success: bool,

        /// Echo back request_id if provided in the request
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,

        /// Return value from the method (if any)
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,

        /// Error message if success is false
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Response to Call: the [`CallResult`]'s fields beside the message's own
    /// when the call was made, a message when it was not.
    ///
    /// Example: `{"type": "call_resp", "success": true, "request_id": "req-3", "ret": {"f64": 4.0}, "mutated": []}`
    CallResp {
        /// Whether the call was made
        success: bool,

        /// Echo back request_id if provided in the request
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,

        /// The call's result: `ret` and `mutated`, present when success is true
        #[serde(flatten)]
        result: Option<CallResult>,

        /// Error message if success is false
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Response to Halt: whether the run was asked to stop.
    ///
    /// Example: `{"type": "halt_resp", "success": true, "request_id": "req-2"}`
    HaltResp {
        /// Whether the halt reached the run.
        success: bool,

        /// Echo back request_id if provided in the request
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,

        /// Error message if success is false
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// Generic error response for parse failures or unknown message types.
    ///
    /// Example: `{"type": "error", "message": "Invalid JSON"}`
    Error {
        /// Echo back request_id if available
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,

        /// Error description
        message: String,
    },

    /// Server-initiated push: values changed (e.g. the runtime wrote new
    /// state). Sent unsolicited to subscribed clients — the live-edit feed.
    ///
    /// Example: `{"type": "values_changed", "values": {"face/mouth": {"f64": 0.5}}}`
    ValuesChanged {
        /// Map of key paths to their new values.
        values: HashMap<String, Value>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incoming_write_values_deserialize() {
        let json = r#"{"type": "write_values", "values": {"test/path": {"f64": 0.5}}}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::WriteValues { values } => {
                assert!(values.contains_key("test/path"));
            }
            _ => panic!("Expected WriteValues message"),
        }
    }

    #[test]
    fn test_incoming_read_values_deserialize() {
        let json = r#"{"type": "read_values", "keys": ["face/mouth", "face/eyes"]}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::ReadValues { keys } => {
                assert_eq!(keys, vec!["face/mouth", "face/eyes"]);
            }
            _ => panic!("Expected ReadValues message"),
        }
    }

    #[test]
    fn test_incoming_list_keys_deserialize() {
        let json = r#"{"type": "list_keys", "path": "face"}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::ListKeys { path } => {
                assert_eq!(path, Some("face".to_string()));
            }
            _ => panic!("Expected ListKeys message"),
        }
    }

    #[test]
    fn test_incoming_invoke_deserialize() {
        let json = r#"{"type": "invoke", "method": "reset", "request_id": "req-1"}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::Invoke {
                method,
                args,
                request_id,
            } => {
                assert_eq!(method, "reset");
                assert!(args.is_empty());
                assert_eq!(request_id, Some("req-1".to_string()));
            }
            _ => panic!("Expected Invoke message"),
        }
    }

    #[test]
    fn test_incoming_describe_methods_deserialize() {
        let json = r#"{"type": "describe_methods", "path": "say"}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            Incoming::DescribeMethods { path: Some(path) } if path == "say"
        ));

        let json = r#"{"type": "describe_methods"}"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, Incoming::DescribeMethods { path: None }));
    }

    /// A call's fields sit beside the message's own, in `Call`'s serde shape.
    #[test]
    fn test_incoming_call_deserialize() {
        let json = r#"{
            "type": "call",
            "module_id": "00000000-0000-0000-0000-00000000a11c",
            "id": "00000000-0000-0000-0000-0000000000d0",
            "args": [{"id": "00000000-0000-0000-0000-0000000000a0", "value": {"f64": 2.0}}],
            "request_id": "req-3"
        }"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::Call { call, request_id } => {
                assert_eq!(
                    call,
                    Call {
                        module_id: Some(arora_types::Uuid::from_u128(0xa11c)),
                        id: arora_types::Uuid::from_u128(0xd0),
                        args: vec![arora_types::value::StructureField {
                            id: arora_types::Uuid::from_u128(0xa0),
                            value: Box::new(Value::F64(2.0)),
                        }],
                    }
                );
                assert_eq!(request_id, Some("req-3".to_string()));
            }
            other => panic!("Expected Call message, got {other:?}"),
        }
    }

    /// A call with no arguments and no request id needs neither field.
    #[test]
    fn test_incoming_call_without_args_deserialize() {
        let json = r#"{
            "type": "call",
            "module_id": "00000000-0000-0000-0000-00000000a11c",
            "id": "00000000-0000-0000-0000-0000000000d0"
        }"#;
        let msg: Incoming = serde_json::from_str(json).unwrap();
        match msg {
            Incoming::Call { call, request_id } => {
                assert!(call.args.is_empty());
                assert_eq!(request_id, None);
            }
            other => panic!("Expected Call message, got {other:?}"),
        }
    }

    /// A call naming no function is not a call.
    #[test]
    fn test_incoming_call_without_id_is_refused() {
        let json = r#"{"type": "call", "module_id": "00000000-0000-0000-0000-00000000a11c"}"#;
        assert!(serde_json::from_str::<Incoming>(json).is_err());
    }

    /// A described method carries `MethodSignature`'s serde shape: the ids
    /// snake_case, the frozen function record camelCase.
    #[test]
    fn test_outgoing_describe_methods_resp_serialize() {
        use arora_types::record::module::frozen::{Function, Parameter};
        use arora_types::record::ty::{FrozenTy, PrimitiveKind};

        let x = arora_types::Uuid::from_u128(0xa0);
        let msg = Outgoing::DescribeMethodsResp {
            methods: vec![MethodSignature {
                module_id: arora_types::Uuid::from_u128(0xa11c),
                id: arora_types::Uuid::from_u128(0xd0),
                name: "double".to_string(),
                function: Function {
                    parameters: HashMap::from([(
                        x,
                        Parameter {
                            name: "x".to_string(),
                            ty: FrozenTy::from(PrimitiveKind::F64),
                            mutable: false,
                        },
                    )]),
                    parameter_ordering: vec![x],
                    return_ty: FrozenTy::from(PrimitiveKind::F64),
                },
            }],
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "describe_methods_resp");
        let method = &json["methods"][0];
        assert_eq!(method["module_id"], "00000000-0000-0000-0000-00000000a11c");
        assert_eq!(method["id"], "00000000-0000-0000-0000-0000000000d0");
        assert_eq!(method["name"], "double");
        let function = &method["function"];
        assert_eq!(
            function["parameterOrdering"],
            serde_json::json!(["00000000-0000-0000-0000-0000000000a0"])
        );
        assert_eq!(
            function["parameters"]["00000000-0000-0000-0000-0000000000a0"]["name"],
            "x"
        );
        assert!(function.get("returnType").is_some(), "{function}");

        // What a client reads back is the signature it was sent.
        let read: Vec<MethodSignature> = serde_json::from_value(json["methods"].clone()).unwrap();
        assert_eq!(
            read,
            match msg {
                Outgoing::DescribeMethodsResp { methods } => methods,
                _ => unreachable!(),
            }
        );
    }

    /// A made call answers with the `CallResult`'s fields beside the message's
    /// own; a refused one with a message and no result.
    #[test]
    fn test_outgoing_call_resp_serialize() {
        let made = Outgoing::CallResp {
            success: true,
            request_id: Some("req-3".to_string()),
            result: Some(CallResult {
                ret: Value::F64(4.0),
                mutated: Vec::new(),
            }),
            message: None,
        };
        assert_eq!(
            serde_json::to_value(&made).unwrap(),
            serde_json::json!({
                "type": "call_resp",
                "success": true,
                "request_id": "req-3",
                "ret": {"f64": 4.0},
                "mutated": []
            })
        );

        let refused = Outgoing::CallResp {
            success: false,
            request_id: None,
            result: None,
            message: Some("call failed: FunctionNotFound".to_string()),
        };
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            serde_json::json!({
                "type": "call_resp",
                "success": false,
                "message": "call failed: FunctionNotFound"
            })
        );
    }

    #[test]
    fn test_outgoing_write_values_resp_serialize() {
        let msg = Outgoing::WriteValuesResp {
            success: true,
            message: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"write_values_resp""#));
        assert!(json.contains(r#""success":true"#));
        assert!(!json.contains("message"));
    }

    #[test]
    fn test_outgoing_invoke_resp_serialize() {
        let msg = Outgoing::InvokeResp {
            success: false,
            request_id: Some("req-1".to_string()),
            value: None,
            message: Some("Method not found".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"invoke_resp""#));
        assert!(json.contains(r#""request_id":"req-1""#));
        assert!(json.contains(r#""message":"Method not found""#));
    }
}
