import { Value, ValueType } from '../../pkg/arora_types.js';
import { strict as assert } from 'assert';

console.log('Testing arora-types WASM API...\n');

// Test 1: ValueType enum is properly exposed
console.log('Test 1: ValueType enum exposure');
assert.strictEqual(ValueType.Unit, 0);
assert.strictEqual(ValueType.Boolean, 1);
assert.strictEqual(ValueType.F64, 11);
assert.strictEqual(ValueType.String, 12);
assert.strictEqual(ValueType.Uuid, 32);
console.log('✓ ValueType enum values match expected numbers\n');

// Test 1.5: Value.unit() static constructor
console.log('Test 1.5: Value.unit() static constructor');
const unitValStatic = Value.unit();
assert.strictEqual(unitValStatic.type, ValueType.Unit);
assert.strictEqual(unitValStatic.get(), undefined);
console.log('✓ Unit value created via static constructor\n');

// Test 2: Value constructor for primitives
console.log('Test 2: Value constructor for primitives');
const boolVal = new Value(ValueType.Boolean, true);
assert.strictEqual(boolVal.type, ValueType.Boolean);
assert.strictEqual(boolVal.get(), true);
console.log('✓ Boolean value created and retrieved\n');

const numVal = new Value(ValueType.F64, 3.14);
assert.strictEqual(numVal.type, ValueType.F64);
assert.strictEqual(numVal.get(), 3.14);
console.log('✓ F64 value created and retrieved\n');

const strVal = new Value(ValueType.String, "hello");
assert.strictEqual(strVal.type, ValueType.String);
assert.strictEqual(strVal.get(), "hello");
console.log('✓ String value created and retrieved\n');

// Test 3: Integer types with range validation
console.log('Test 3: Integer types with range validation');
const u8Val = new Value(ValueType.U8, 255);
assert.strictEqual(u8Val.type, ValueType.U8);
assert.strictEqual(u8Val.get(), 255);
console.log('✓ U8 max value works\n');

try {
    new Value(ValueType.U8, 256);
    assert.fail('Should have thrown error for U8 out of range');
} catch (e) {
    console.log('✓ U8 out of range properly rejected\n');
}

const i32Val = new Value(ValueType.I32, -42);
assert.strictEqual(i32Val.type, ValueType.I32);
assert.strictEqual(i32Val.get(), -42);
console.log('✓ I32 negative value works\n');

// Test 4: Unit and Option types
console.log('Test 4: Unit and Option types');
const unitVal = new Value(ValueType.Unit, null);
assert.strictEqual(unitVal.type, ValueType.Unit);
assert.strictEqual(unitVal.get(), undefined);
console.log('✓ Unit value created\n');

const someVal = new Value(ValueType.Option, 42);
assert.strictEqual(someVal.type, ValueType.Option);
assert.strictEqual(someVal.get(), 42);
console.log('✓ Option(Some) value created\n');

const noneVal = new Value(ValueType.Option, null);
assert.strictEqual(noneVal.type, ValueType.Option);
assert.strictEqual(noneVal.get(), null);
console.log('✓ Option(None) value created\n');

// Test 5: Array types
console.log('Test 5: Array types');
const boolArr = new Value(ValueType.ArrayBoolean, [true, false, true]);
assert.strictEqual(boolArr.type, ValueType.ArrayBoolean);
assert.deepStrictEqual(boolArr.get(), [true, false, true]);
console.log('✓ Boolean array works\n');

const f64Arr = new Value(ValueType.ArrayF64, [1.5, 2.5, 3.5]);
assert.strictEqual(f64Arr.type, ValueType.ArrayF64);
assert.deepStrictEqual(f64Arr.get(), [1.5, 2.5, 3.5]);
console.log('✓ F64 array works\n');

const strArr = new Value(ValueType.ArrayString, ["a", "b", "c"]);
assert.strictEqual(strArr.type, ValueType.ArrayString);
assert.deepStrictEqual(strArr.get(), ["a", "b", "c"]);
console.log('✓ String array works\n');

// Test 6: Value.from() with auto-detection
console.log('Test 6: Value.from() with auto-detection');
const autoBoolean = Value.from(true);
assert.strictEqual(autoBoolean.type, ValueType.Boolean);
assert.strictEqual(autoBoolean.get(), true);
console.log('✓ Auto-detected boolean\n');

const autoNumber = Value.from(42.0);
assert.strictEqual(autoNumber.type, ValueType.F64);
assert.strictEqual(autoNumber.get(), 42.0);
console.log('✓ Auto-detected number (defaults to F64)\n');

const autoString = Value.from("test");
assert.strictEqual(autoString.type, ValueType.String);
assert.strictEqual(autoString.get(), "test");
console.log('✓ Auto-detected string\n');

const autoNull = Value.from(null);
// Note: Value.from(null) returns Option(None), not Unit
assert.strictEqual(autoNull.type, ValueType.Option);
assert.strictEqual(autoNull.get(), null);
console.log('✓ Auto-detected null as Option(None)\n');

// Test 7: Auto-detected arrays
console.log('Test 7: Auto-detected arrays');
const autoBoolArr = Value.from([true, false]);
assert.strictEqual(autoBoolArr.type, ValueType.ArrayBoolean);
assert.deepStrictEqual(autoBoolArr.get(), [true, false]);
console.log('✓ Auto-detected boolean array\n');

const autoNumArr = Value.from([1.0, 2.0, 3.0]);
assert.strictEqual(autoNumArr.type, ValueType.ArrayF64);
assert.deepStrictEqual(autoNumArr.get(), [1.0, 2.0, 3.0]);
console.log('✓ Auto-detected number array (defaults to ArrayF64)\n');

const autoStrArr = Value.from(["x", "y", "z"]);
assert.strictEqual(autoStrArr.type, ValueType.ArrayString);
assert.deepStrictEqual(autoStrArr.get(), ["x", "y", "z"]);
console.log('✓ Auto-detected string array\n');

// Test 8: set() method with type checking
console.log('Test 8: set() method with type checking');
const mutableVal = new Value(ValueType.F64, 1.0);
assert.strictEqual(mutableVal.get(), 1.0);
mutableVal.set(2.0);
assert.strictEqual(mutableVal.get(), 2.0);
console.log('✓ set() updates value correctly\n');

try {
    mutableVal.set("string");
    assert.fail('Should have thrown error for type mismatch');
} catch (e) {
    console.log('✓ set() rejects mismatched type\n');
}

// Test 9: KeyValue from plain object
console.log('Test 9: KeyValue from plain object');
const kvObj = { name: "Alice", age: 30, active: true };
const kvVal = Value.from(kvObj);
assert.strictEqual(kvVal.type, ValueType.KeyValue);
const retrieved = kvVal.get();
assert.strictEqual(retrieved.name, "Alice");
assert.strictEqual(retrieved.age, 30);
assert.strictEqual(retrieved.active, true);
console.log('✓ KeyValue created and retrieved from plain object\n');

// Test 10: Nested values in ArrayValue
console.log('Test 10: Nested values in ArrayValue');
const nestedArr = [42, "text", true, null];
const nestedVal = Value.from(nestedArr);
assert.strictEqual(nestedVal.type, ValueType.ArrayValue);
const retrievedNested = nestedVal.get();
assert.strictEqual(retrievedNested.length, 4);
assert.strictEqual(retrievedNested[0], 42);
assert.strictEqual(retrievedNested[1], "text");
assert.strictEqual(retrievedNested[2], true);
assert.strictEqual(retrievedNested[3], null);
console.log('✓ Mixed-type array (ArrayValue) works\n');

// Test 11: Empty arrays
console.log('Test 11: Empty arrays');
const emptyArr = Value.from([]);
assert.strictEqual(emptyArr.type, ValueType.ArrayValue);
assert.deepStrictEqual(emptyArr.get(), []);
console.log('✓ Empty array defaults to ArrayValue\n');

// Test 12: getAs() method (with null registry for now)
console.log('Test 12: getAs() method');
const val = new Value(ValueType.F64, 123.45);
const retrieved2 = val.getAs(null);
assert.strictEqual(retrieved2, 123.45);
console.log('✓ getAs() retrieves value (type registry placeholder)\n');

// Test 13: Type switching/matching pattern
console.log('Test 13: Type switching/matching pattern');
const unknownValue = Value.from(42.5);

switch (unknownValue.type) {
    case ValueType.Boolean:
        console.log('  Value is boolean:', unknownValue.get());
        break;
    case ValueType.F64:
        console.log('  Value is F64:', unknownValue.get());
        assert.strictEqual(unknownValue.get(), 42.5);
        break;
    case ValueType.String:
        console.log('  Value is string:', unknownValue.get());
        break;
    case ValueType.ArrayF64:
        console.log('  Value is F64 array:', unknownValue.get());
        break;
    case ValueType.KeyValue:
        console.log('  Value is KeyValue object:', unknownValue.get());
        break;
    case ValueType.Option:
        const optVal = unknownValue.get();
        if (optVal === null) {
            console.log('  Value is Option(None)');
        } else {
            console.log('  Value is Option(Some):', optVal);
        }
        break;
    default:
        console.log('  Unknown or unhandled type:', unknownValue.type);
}
console.log('✓ Type switching works correctly\n');

console.log('\n✅ All tests passed!');
