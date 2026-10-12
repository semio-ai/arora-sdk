use arora_types::value::Value as AroraValue;
use qi::value::Value as QiValue;

/// The number held by an Arora value, if numeric.
pub(crate) fn arora_f64(value: &AroraValue) -> Option<f64> {
    Some(match value {
        AroraValue::F64(v) => *v,
        AroraValue::F32(v) => f64::from(*v),
        AroraValue::I8(v) => f64::from(*v),
        AroraValue::I16(v) => f64::from(*v),
        AroraValue::I32(v) => f64::from(*v),
        AroraValue::I64(v) => *v as f64,
        AroraValue::U8(v) => f64::from(*v),
        AroraValue::U16(v) => f64::from(*v),
        AroraValue::U32(v) => f64::from(*v),
        AroraValue::U64(v) => *v as f64,
        AroraValue::Boolean(v) => f64::from(u8::from(*v)),
        _ => return None,
    })
}

/// The number held by a `qi` value (an `ALValue`), if numeric; dynamics are unwrapped.
pub(crate) fn qi_f64(value: &QiValue<'_>) -> Option<f64> {
    Some(match value {
        QiValue::Float32(v) => f64::from(v.0),
        QiValue::Float64(v) => v.0,
        QiValue::Int8(v) => f64::from(*v),
        QiValue::Int16(v) => f64::from(*v),
        QiValue::Int32(v) => f64::from(*v),
        QiValue::Int64(v) => *v as f64,
        QiValue::UInt8(v) => f64::from(*v),
        QiValue::UInt16(v) => f64::from(*v),
        QiValue::UInt32(v) => f64::from(*v),
        QiValue::UInt64(v) => *v as f64,
        QiValue::Bool(v) => f64::from(u8::from(*v)),
        QiValue::Dynamic(inner) => return qi_f64(inner),
        _ => return None,
    })
}

/// The elements of a `qi` list value (an `ALValue` array); dynamics are unwrapped.
pub(crate) fn qi_list(value: QiValue<'static>) -> Option<Vec<QiValue<'static>>> {
    match value {
        QiValue::List(items) | QiValue::Tuple(items) => Some(items),
        QiValue::Dynamic(inner) => qi_list(*inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_convert_from_any_width() {
        assert_eq!(arora_f64(&AroraValue::F32(1.5)), Some(1.5));
        assert_eq!(arora_f64(&AroraValue::I32(-2)), Some(-2.0));
        assert_eq!(arora_f64(&AroraValue::String("x".into())), None);
        assert_eq!(qi_f64(&QiValue::Int32(3)), Some(3.0));
        assert_eq!(
            qi_f64(&QiValue::Dynamic(Box::new(QiValue::Float32(0.5.into())))),
            Some(0.5)
        );
        assert_eq!(qi_f64(&QiValue::Unit), None);
    }

    #[test]
    fn lists_unwrap_dynamics() {
        let list = QiValue::Dynamic(Box::new(QiValue::List(vec![QiValue::Int32(1)])));
        assert_eq!(qi_list(list), Some(vec![QiValue::Int32(1)]));
        assert_eq!(qi_list(QiValue::Int32(1)), None);
    }
}
