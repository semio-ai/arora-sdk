//! A minimal fake NAOqi for the tests: the services and members the HAL uses, hosted
//! in-process with `qi`, with hooks to inspect the commands received and to drive sensors.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use qi::dynamic::ObjectBuilder;
use qi::node::Node;
use qi::service_directory::LocalServiceDirectory;
use qi::value::{Dynamic, Value};
use qi::{AnyObject, Error, Signal};

type Memory = Arc<Mutex<HashMap<String, Value<'static>>>>;
type EventSignals = Arc<Mutex<HashMap<String, Signal<Dynamic<Value<'static>>>>>>;
/// The `setAngles` commands received: names, angles and speed fraction.
pub type AngleCommands = Arc<Mutex<Vec<(Vec<String>, Vec<f32>, f32)>>>;
/// The `setStiffnesses` commands received: names and stiffnesses.
pub type StiffnessCommands = Arc<Mutex<Vec<(Vec<String>, Vec<f32>)>>>;

pub const JOINTS: [&str; 3] = ["HeadYaw", "HeadPitch", "LShoulderPitch"];

pub struct FakeNaoqi {
    /// The node hosting the fake robot; the services live as long as it does.
    _node: Node<LocalServiceDirectory>,
    pub url: String,
    memory: Memory,
    events: EventSignals,
    pub said: Arc<Mutex<Vec<String>>>,
    pub set_angles: AngleCommands,
    pub set_stiffnesses: StiffnessCommands,
    pub leds: Arc<Mutex<Vec<(String, i32, f32)>>>,
    pub move_toward: Arc<Mutex<Vec<(f32, f32, f32)>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn initial_memory() -> HashMap<String, Value<'static>> {
    let mut memory = HashMap::new();
    let mut set = |key: &str, value: Value<'static>| {
        memory.insert(key.to_string(), value);
    };
    set("RobotConfig/Body/Type", Value::String("Nao".into()));
    set("RobotConfig/Body/BaseVersion", Value::String("V6".into()));
    for (index, joint) in JOINTS.iter().enumerate() {
        set(
            &format!("Device/SubDeviceList/{joint}/Position/Sensor/Value"),
            Value::Float32((0.1 * index as f32).into()),
        );
        set(
            &format!("Device/SubDeviceList/{joint}/Hardness/Actuator/Value"),
            Value::Float32(0.8.into()),
        );
        set(
            &format!("Device/SubDeviceList/{joint}/Temperature/Sensor/Value"),
            Value::Float32(30.0.into()),
        );
        set(
            &format!("Device/SubDeviceList/{joint}/ElectricCurrent/Sensor/Value"),
            Value::Float32(0.05.into()),
        );
    }
    set(
        "Device/SubDeviceList/Battery/Charge/Sensor/Value",
        Value::Float32(0.75.into()),
    );
    set(
        "Device/SubDeviceList/Battery/Current/Sensor/Value",
        Value::Float32((-0.5).into()),
    );
    set(
        "Device/SubDeviceList/InertialSensor/AngleX/Sensor/Value",
        Value::Float32(0.01.into()),
    );
    set(
        "Device/SubDeviceList/US/Left/Sensor/Value",
        Value::Float32(1.5.into()),
    );
    // Touch events are memory keys too.
    set("FrontTactilTouched", Value::Float32(0.0.into()));
    memory
}

impl FakeNaoqi {
    /// Hosts the fake NAOqi on a loopback port.
    pub async fn start() -> Self {
        let memory: Memory = Arc::new(Mutex::new(initial_memory()));
        let events: EventSignals = Arc::default();
        let said = Arc::new(Mutex::new(Vec::new()));
        let set_angles = Arc::new(Mutex::new(Vec::new()));
        let set_stiffnesses = Arc::new(Mutex::new(Vec::new()));
        let leds = Arc::new(Mutex::new(Vec::new()));
        let move_toward = Arc::new(Mutex::new(Vec::new()));

        let mut init = qi::node::init();
        init.add_service_object("ALMemory", almemory(memory.clone(), events.clone()));
        init.add_service_object(
            "ALMotion",
            almotion(
                memory.clone(),
                set_angles.clone(),
                set_stiffnesses.clone(),
                move_toward.clone(),
            ),
        );
        init.add_service_object("ALTextToSpeech", altexttospeech(said.clone()));
        init.add_service_object("ALLeds", alleds(leds.clone()));
        init.add_service_object("ALSystem", alsystem());
        init.add_service_object("ALRobotModel", alrobotmodel());
        init.bind("tcp://127.0.0.1:0".parse().unwrap());
        let node = init
            .host_space()
            .start()
            .await
            .expect("host the fake NAOqi");
        let url = node
            .endpoints()
            .iter()
            .find_map(|endpoint| {
                let endpoint = endpoint.to_string();
                endpoint.starts_with("tcp://").then_some(endpoint)
            })
            .expect("a TCP endpoint");
        Self {
            _node: node,
            url,
            memory,
            events,
            said,
            set_angles,
            set_stiffnesses,
            leds,
            move_toward,
        }
    }

    /// Sets a memory key to a number, as a sensor would.
    pub fn set_memory(&self, key: &str, value: f32) {
        lock(&self.memory).insert(key.to_string(), Value::Float32(value.into()));
    }

    /// Raises an event: sets its memory key and notifies its subscribers.
    pub fn raise_event(&self, key: &str, value: f32) {
        self.set_memory(key, value);
        if let Some(signal) = lock(&self.events).get(key) {
            signal.emit(Dynamic(Value::Float32(value.into())));
        }
    }
}

fn almemory(memory: Memory, events: EventSignals) -> AnyObject {
    let mut builder = ObjectBuilder::new();
    let get_memory = memory.clone();
    builder.add_method("getData", move |key: String| {
        let memory = get_memory.clone();
        async move {
            lock(&memory)
                .get(&key)
                .cloned()
                .map(Dynamic)
                .ok_or_else(|| Error::Other(format!("ALMemory::getData: {key} not found").into()))
        }
    });
    let list_memory = memory.clone();
    builder.add_method("getListData", move |keys: Dynamic<Value<'static>>| {
        let memory = list_memory.clone();
        async move {
            let Value::List(keys) = keys.0 else {
                return Err(Error::Other("getListData expects a list".into()));
            };
            let memory = lock(&memory);
            // An ALValue array: every element is itself an ALValue (a dynamic value).
            let values = keys
                .into_iter()
                .map(|key| match key {
                    Value::String(key) => memory
                        .get(&key.to_string())
                        .cloned()
                        // Unknown keys are invalid values, as in NAOqi.
                        .unwrap_or(Value::Unit),
                    _ => Value::Unit,
                })
                .map(Value::into_dynamic)
                .collect();
            Ok(Dynamic(Value::List(values)))
        }
    });
    let insert_memory = memory.clone();
    let insert_events = events.clone();
    let insert = move |(key, value): (String, Dynamic<Value<'static>>)| {
        let memory = insert_memory.clone();
        let events = insert_events.clone();
        async move {
            lock(&memory).insert(key.clone(), value.0.clone());
            if let Some(signal) = lock(&events).get(&key) {
                signal.emit(value);
            }
            Ok(())
        }
    };
    builder.add_method("insertData", insert.clone());
    builder.add_method("raiseEvent", insert);
    builder.add_method("subscriber", move |key: String| {
        let events = events.clone();
        async move {
            let signal = lock(&events).entry(key).or_default().clone();
            let mut subscriber = ObjectBuilder::new();
            subscriber.add_signal("signal", signal);
            Ok(AnyObject::new(subscriber.build()))
        }
    });
    AnyObject::new(builder.build())
}

fn almotion(
    memory: Memory,
    set_angles: AngleCommands,
    set_stiffnesses: StiffnessCommands,
    move_toward: Arc<Mutex<Vec<(f32, f32, f32)>>>,
) -> AnyObject {
    let mut builder = ObjectBuilder::new();
    builder.add_method("getBodyNames", |_chain: String| async move {
        Ok(JOINTS
            .iter()
            .map(|joint| joint.to_string())
            .collect::<Vec<_>>())
    });
    let angles_memory = memory.clone();
    builder.add_method(
        "getAngles",
        move |(names, _sensors): (Dynamic<Value<'static>>, bool)| {
            let memory = angles_memory.clone();
            async move {
                let names = match names.0 {
                    Value::List(names) => names,
                    single => vec![single],
                };
                let memory = lock(&memory);
                Ok(names
                    .iter()
                    .map(|name| {
                        match memory.get(&format!(
                            "Device/SubDeviceList/{name}/Position/Sensor/Value"
                        )) {
                            Some(Value::Float32(value)) => value.0,
                            _ => 0.0,
                        }
                    })
                    .collect::<Vec<f32>>())
            }
        },
    );
    builder.add_method(
        "setAngles",
        move |(names, angles, speed): (Dynamic<Vec<String>>, Dynamic<Vec<f32>>, f32)| {
            let memory = memory.clone();
            let set_angles = set_angles.clone();
            async move {
                // The fake reaches its targets instantly.
                let mut memory = lock(&memory);
                for (name, angle) in names.0.iter().zip(&angles.0) {
                    memory.insert(
                        format!("Device/SubDeviceList/{name}/Position/Sensor/Value"),
                        Value::Float32((*angle).into()),
                    );
                }
                lock(&set_angles).push((names.0, angles.0, speed));
                Ok(())
            }
        },
    );
    builder.add_method(
        "setStiffnesses",
        move |(names, values): (Dynamic<Vec<String>>, Dynamic<Vec<f32>>)| {
            let set_stiffnesses = set_stiffnesses.clone();
            async move {
                lock(&set_stiffnesses).push((names.0, values.0));
                Ok(())
            }
        },
    );
    builder.add_method("moveToward", move |(x, y, theta): (f32, f32, f32)| {
        let move_toward = move_toward.clone();
        async move {
            lock(&move_toward).push((x, y, theta));
            Ok(())
        }
    });
    AnyObject::new(builder.build())
}

fn altexttospeech(said: Arc<Mutex<Vec<String>>>) -> AnyObject {
    let mut builder = ObjectBuilder::new();
    builder.add_method("say", move |text: String| {
        let said = said.clone();
        async move {
            // Speaking takes time.
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            lock(&said).push(text);
            Ok(())
        }
    });
    AnyObject::new(builder.build())
}

fn alleds(leds: Arc<Mutex<Vec<(String, i32, f32)>>>) -> AnyObject {
    let mut builder = ObjectBuilder::new();
    builder.add_method(
        "fadeRGB",
        move |(group, rgb, duration): (String, i32, f32)| {
            let leds = leds.clone();
            async move {
                lock(&leds).push((group, rgb, duration));
                Ok(())
            }
        },
    );
    builder.add_method(
        "setIntensity",
        |(_group, _intensity): (String, f32)| async move { Ok(()) },
    );
    AnyObject::new(builder.build())
}

fn alsystem() -> AnyObject {
    let mut builder = ObjectBuilder::new();
    builder.add_method("systemVersion", |(): ()| async move {
        Ok("2.8.7.4".to_string())
    });
    AnyObject::new(builder.build())
}

fn alrobotmodel() -> AnyObject {
    let mut builder = ObjectBuilder::new();
    builder.add_method(
        "getRobotType",
        |(): ()| async move { Ok("Nao".to_string()) },
    );
    AnyObject::new(builder.build())
}
