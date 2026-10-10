//! The device's HAL as a module ([`arora_hal::hal_module`]): a host module
//! whose functions answer the HAL's components' models, described so a remote
//! finds them in the device's method list.
//!
//! Only a model the HAL states servable leaves the device: [`model_glb`] and
//! [`model_glbs`] check [`ComponentModel::servable`] before they ask the HAL
//! for its bytes.

use std::collections::HashMap;
use std::sync::Arc;

use arora_engine::module::{HostModule, ModuleBuilder};
use arora_hal::{hal_module, ComponentGlb, ComponentModel, Hal};
use arora_types::call::{Call, CallError, CallResult};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenArray, FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::record::FrozenReference;
use arora_types::value::Value;
use arora_types::AroraType;

/// The HAL module over `hal`, under [`hal_module::ID`].
pub(crate) fn module(hal: Arc<dyn Hal>) -> HostModule {
    ModuleBuilder::new(hal_module::ID)
        .described_function(hal_module::MODELS, "models", models_signature(), {
            let hal = hal.clone();
            move |_call| {
                answer(Value::array_of_type::<ComponentModel>(
                    models(&*hal).into_iter().map(Value::from).collect(),
                ))
            }
        })
        .described_function(hal_module::MODEL_GLB, "model_glb", model_glb_signature(), {
            let hal = hal.clone();
            move |call| {
                let component = component_of(&call)?;
                answer(Value::Option(
                    model_glb(&*hal, &component)?.map(|glb| Box::new(Value::ArrayU8(glb))),
                ))
            }
        })
        .described_function(
            hal_module::MODEL_GLBS,
            "model_glbs",
            model_glbs_signature(),
            move |_call| {
                answer(Value::array_of_type::<ComponentGlb>(
                    model_glbs(&*hal)?.into_iter().map(Value::from).collect(),
                ))
            },
        )
        .build()
}

fn answer(ret: Value) -> Result<CallResult, CallError> {
    Ok(CallResult {
        ret,
        mutated: Vec::new(),
    })
}

fn guest(message: impl Into<String>) -> CallError {
    CallError::Guest {
        message: message.into(),
    }
}

/// The HAL's components' models; none for a HAL without assets.
fn models(hal: &dyn Hal) -> Vec<ComponentModel> {
    hal.assets()
        .map(|assets| assets.models())
        .unwrap_or_default()
}

/// `component`'s model, when the HAL states it servable.
fn model_glb(hal: &dyn Hal, component: &str) -> Result<Option<Vec<u8>>, CallError> {
    let Some(assets) = hal.assets() else {
        return Ok(None);
    };
    let servable = assets
        .models()
        .iter()
        .any(|model| model.component == component && model.servable);
    if !servable {
        return Ok(None);
    }
    assets
        .servable_glb(component)
        .map_err(|e| guest(format!("the HAL could not read {component}'s model: {e}")))
}

/// Every model the HAL states servable, with its bytes.
fn model_glbs(hal: &dyn Hal) -> Result<Vec<ComponentGlb>, CallError> {
    let Some(assets) = hal.assets() else {
        return Ok(Vec::new());
    };
    let mut glbs = Vec::new();
    for model in assets.models().into_iter().filter(|model| model.servable) {
        let glb = assets.servable_glb(&model.component).map_err(|e| {
            guest(format!(
                "the HAL could not read {}'s model: {e}",
                model.component
            ))
        })?;
        if let Some(glb) = glb {
            glbs.push(ComponentGlb {
                component: model.component,
                glb,
            });
        }
    }
    Ok(glbs)
}

/// [`hal_module::MODEL_GLB`]'s one argument, the component's name.
fn component_of(call: &Call) -> Result<String, CallError> {
    match call
        .args
        .iter()
        .find(|field| field.id == hal_module::MODEL_GLB_COMPONENT)
        .map(|field| field.value.as_ref())
    {
        Some(Value::String(component)) => Ok(component.clone()),
        Some(other) => Err(guest(format!(
            "model_glb takes the component's name as a string, not {}",
            other.kind()
        ))),
        None => Err(guest("model_glb needs the component's name")),
    }
}

/// An array of the record `T`, at the version it is pinned at.
fn array_of<T: AroraType>() -> FrozenTy {
    FrozenTy::FrozenArray(FrozenArray {
        reference: FrozenReference {
            id: T::arora_type_id(),
            version: T::arora_type_version(),
        },
    })
}

/// `models() -> ComponentModel[]`.
fn models_signature() -> Function {
    Function {
        parameters: HashMap::new(),
        parameter_ordering: Vec::new(),
        return_ty: array_of::<ComponentModel>(),
    }
}

/// `model_glb(component: string) -> Option<bytes>`.
fn model_glb_signature() -> Function {
    Function {
        parameters: HashMap::from([(
            hal_module::MODEL_GLB_COMPONENT,
            Parameter {
                name: "component".to_string(),
                ty: FrozenTy::from(PrimitiveKind::String),
                mutable: false,
            },
        )]),
        parameter_ordering: vec![hal_module::MODEL_GLB_COMPONENT],
        return_ty: FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::ArrayU8)),
        }),
    }
}

/// `model_glbs() -> ComponentGlb[]`.
fn model_glbs_signature() -> Function {
    Function {
        parameters: HashMap::new(),
        parameter_ordering: Vec::new(),
        return_ty: array_of::<ComponentGlb>(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arora_bridge::Caller;
    use arora_hal::{
        content_hash, hal_module, ComponentGlb, ComponentModel, FakeHal, Hal, HalAssets,
        HalDescription, HalResult, UpdatesStream, DEVICE,
    };
    use arora_types::call::Call;
    use arora_types::data::{Key, State, StateChange};
    use arora_types::value::{StructureField, Value};

    use crate::caller_tests::settle;
    use crate::Arora;

    /// A HAL of two components: a servable `face` and a `robot` whose model is
    /// not servable.
    struct TwoModels(FakeHal);

    fn model(component: &str, glb: &[u8], servable: bool) -> ComponentModel {
        ComponentModel {
            component: component.to_string(),
            description: None,
            reference: None,
            content_hash: Some(content_hash(glb)),
            servable,
            mount: None,
        }
    }

    #[async_trait::async_trait]
    impl Hal for TwoModels {
        async fn describe(&self) -> HalDescription {
            self.0.describe().await
        }
        async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
            self.0.read(keys).await
        }
        async fn read_all(&self) -> HalResult<State> {
            self.0.read_all().await
        }
        async fn write(&self, changes: StateChange) -> HalResult<()> {
            self.0.write(changes).await
        }
        fn try_send(&self, changes: &StateChange) {
            self.0.try_send(changes)
        }
        fn updates(&self) -> UpdatesStream {
            self.0.updates()
        }
        fn assets(&self) -> Option<&dyn HalAssets> {
            Some(self)
        }
    }

    impl HalAssets for TwoModels {
        fn models(&self) -> Vec<ComponentModel> {
            vec![
                model("face", b"face", true),
                model("robot", b"robot", false),
            ]
        }
        fn servable_glb(&self, component: &str) -> HalResult<Option<Vec<u8>>> {
            Ok(Some(component.as_bytes().to_vec()))
        }
    }

    fn call(id: arora_types::Uuid, component: Option<&str>) -> Call {
        Call {
            module_id: Some(hal_module::ID),
            id,
            args: component
                .map(|component| StructureField {
                    id: hal_module::MODEL_GLB_COMPONENT,
                    value: Box::new(Value::String(component.to_string())),
                })
                .into_iter()
                .collect(),
        }
    }

    fn glb(bytes: &[u8]) -> Value {
        Value::Option(Some(Box::new(Value::ArrayU8(bytes.to_vec()))))
    }

    /// A HAL that is not composed states its one model under `device`; a
    /// remote lists it and fetches its bytes.
    #[test]
    fn the_device_component_s_model_is_listed_and_served() {
        let hal = FakeHal::new();
        let mut arora = Arora::builder()
            .with_hal(Box::new(hal.clone()))
            .build()
            .expect("build the device");
        let caller = arora.caller();

        let none = settle(&mut arora, caller.call(call(hal_module::MODELS, None)))
            .expect("the device answers");
        assert!(none.ret.into_elements().unwrap().is_empty());
        let none = settle(
            &mut arora,
            caller.call(call(hal_module::MODEL_GLB, Some(DEVICE))),
        )
        .expect("the device answers");
        assert_eq!(none.ret, Value::Option(None));

        hal.set_model_glb(b"glTF".to_vec());
        let models: Vec<ComponentModel> =
            settle(&mut arora, caller.call(call(hal_module::MODELS, None)))
                .expect("the device answers")
                .ret
                .into_elements()
                .unwrap()
                .into_iter()
                .map(|model| ComponentModel::try_from(model).unwrap())
                .collect();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].component, DEVICE);
        assert_eq!(models[0].content_hash, Some(content_hash(b"glTF")));
        assert!(models[0].servable);

        let model = settle(
            &mut arora,
            caller.call(call(hal_module::MODEL_GLB, Some(DEVICE))),
        )
        .expect("the device answers");
        assert_eq!(model.ret, glb(b"glTF"));
    }

    /// A model the HAL does not state servable never leaves the device, though
    /// the HAL would hand its bytes over.
    #[test]
    fn only_a_servable_model_is_served() {
        let mut arora = Arora::builder()
            .with_hal(Box::new(TwoModels(FakeHal::new())))
            .build()
            .expect("build the device");

        let face = arora
            .call(call(hal_module::MODEL_GLB, Some("face")))
            .expect("the device answers");
        assert_eq!(face.ret, glb(b"face"));
        let robot = arora
            .call(call(hal_module::MODEL_GLB, Some("robot")))
            .expect("the device answers");
        assert_eq!(robot.ret, Value::Option(None));
        let unknown = arora
            .call(call(hal_module::MODEL_GLB, Some("arm")))
            .expect("the device answers");
        assert_eq!(unknown.ret, Value::Option(None));

        let glbs: Vec<ComponentGlb> = arora
            .call(call(hal_module::MODEL_GLBS, None))
            .expect("the device answers")
            .ret
            .into_elements()
            .unwrap()
            .into_iter()
            .map(|glb| ComponentGlb::try_from(glb).unwrap())
            .collect();
        assert_eq!(
            glbs,
            vec![ComponentGlb {
                component: "face".to_string(),
                glb: b"face".to_vec(),
            }]
        );

        let models = arora
            .call(call(hal_module::MODELS, None))
            .expect("the device answers")
            .ret
            .into_elements()
            .unwrap();
        assert_eq!(models.len(), 2, "both models are listed");
    }

    /// `model_glb` without the component's name is refused.
    #[test]
    fn model_glb_needs_a_component() {
        let mut arora = Arora::builder().build().expect("build the device");
        let error = arora
            .call(call(hal_module::MODEL_GLB, None))
            .expect_err("no component");
        assert!(error.to_string().contains("component"), "{error}");
    }

    /// A device lists the HAL module's three functions.
    #[test]
    fn the_device_describes_the_hal_module() {
        let mut arora = Arora::builder().build().expect("build the device");
        let caller = arora.caller();
        let methods = settle(
            &mut arora,
            caller.describe_methods(Some("model".to_string())),
        )
        .expect("the device describes its methods");
        let described: HashMap<_, _> = methods
            .iter()
            .filter(|method| method.module_id == hal_module::ID)
            .map(|method| (method.name.as_str(), method.id))
            .collect();
        assert_eq!(
            described,
            HashMap::from([
                ("models", hal_module::MODELS),
                ("model_glb", hal_module::MODEL_GLB),
                ("model_glbs", hal_module::MODEL_GLBS),
            ])
        );
        let model_glb = methods
            .iter()
            .find(|method| method.id == hal_module::MODEL_GLB)
            .unwrap();
        assert_eq!(
            model_glb.function.parameter_ordering,
            vec![hal_module::MODEL_GLB_COMPONENT]
        );
    }
}
