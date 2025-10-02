use arora_schema::value::Value;

fn main() {
    // Test Option conversions
    let some_value = Value::from(Some(42u32));
    println!("Some(42u32) -> {:?}", some_value);
    
    let none_value = Value::from(None::<String>);
    println!("None::<String> -> {:?}", none_value);
    
    // Test ArrayValue conversions
    let mixed_array = vec![
        Value::U32(42),
        Value::Boolean(true),
        Value::String("test".to_string()),
    ];
    let array_value = Value::from(mixed_array);
    println!("Vec<Value> -> {:?}", array_value);
    
    // Test nested structures
    let nested = Value::from(Some(vec![
        Value::U32(1),
        Value::U32(2),
        Value::U32(3),
    ]));
    println!("Some(Vec<Value>) -> {:?}", nested);
}
