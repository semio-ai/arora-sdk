/**
 * Tests for WASM object type checking - verifying that Value instances
 * created in WASM maintain their type when passed through JavaScript.
 *
 * These tests demonstrate "true casting" - checking that a JavaScript value
 * is actually an instance of the Value class, not a primitive that needs
 * conversion.
 */

import assert from 'node:assert';
import { Value, ValueType } from '../../pkg/arora_types.js';

console.log('\n=== Value instanceof Tests ===\n');

// Test 1: Value instanceof check
console.log('Test 1: instanceof operator');
const unitVal = Value.unit();
assert.ok(unitVal instanceof Value, 'Value.unit() should return a Value instance');

const boolVal = new Value(ValueType.Boolean, true);
assert.ok(boolVal instanceof Value, 'new Value() should return a Value instance');

const numVal = new Value(ValueType.F64, 42.5);
assert.ok(numVal instanceof Value, 'Value with number should be Value instance');

const strVal = new Value(ValueType.String, 'test');
assert.ok(strVal instanceof Value, 'Value with string should be Value instance');

console.log('✓ All Value constructors return Value instances\n');

// Test 2: Primitives are NOT Value instances
console.log('Test 2: Primitives are not Value instances');
assert.ok(!(true instanceof Value), 'Plain boolean is not a Value instance');
assert.ok(!(42 instanceof Value), 'Plain number is not a Value instance');
assert.ok(!('test' instanceof Value), 'Plain string is not a Value instance');
assert.ok(!(undefined instanceof Value), 'undefined is not a Value instance');
assert.ok(!(null instanceof Value), 'null is not a Value instance');

console.log('✓ Primitives correctly identified as non-Value\n');

// Test 3: Value passed through array maintains type
console.log('Test 3: Value through array container');
const array = [Value.unit(), new Value(ValueType.F64, 123.456)];

// Extract from array - should still be Value instances
assert.ok(array[0] instanceof Value, 'Value extracted from array should be Value instance');
assert.ok(array[1] instanceof Value, 'Value extracted from array should be Value instance');

assert.strictEqual(array[0].type, ValueType.Unit);
assert.strictEqual(array[1].type, ValueType.F64);
assert.strictEqual(array[1].get(), 123.456);

console.log('✓ Values maintain type through array storage\n');

// Test 4: Value passed through object property
console.log('Test 4: Value through object property');
const obj = {
  unit: Value.unit(),
  bool: new Value(ValueType.Boolean, false),
  number: new Value(ValueType.F64, 99.9),
};

assert.ok(obj.unit instanceof Value, 'Value as object property should be Value instance');
assert.ok(obj.bool instanceof Value, 'Value as object property should be Value instance');
assert.ok(obj.number instanceof Value, 'Value as object property should be Value instance');

assert.strictEqual(obj.unit.type, ValueType.Unit);
assert.strictEqual(obj.bool.type, ValueType.Boolean);
assert.strictEqual(obj.bool.get(), false);
assert.strictEqual(obj.number.get(), 99.9);

console.log('✓ Values maintain type through object storage\n');

// Test 5: Value passed as function parameter
console.log('Test 5: Value through function parameter');

function checkValue(val) {
  assert.ok(val instanceof Value, 'Parameter should be Value instance');
  return val.type;
}

const testVal = new Value(ValueType.String, 'param');
const type = checkValue(testVal);
assert.strictEqual(type, ValueType.String);

console.log('✓ Values maintain type when passed as function parameters\n');

// Test 6: Value returned from function
console.log('Test 6: Value returned from function');

function createValue() {
  return new Value(ValueType.Boolean, true);
}

const returned = createValue();
assert.ok(returned instanceof Value, 'Returned value should be Value instance');
assert.strictEqual(returned.type, ValueType.Boolean);
assert.strictEqual(returned.get(), true);

console.log('✓ Values maintain type when returned from functions\n');

// Test 7: Mixed array with Values and primitives
console.log('Test 7: Mixed array - Value vs primitive');

const mixed = [
  new Value(ValueType.F64, 10),
  20,  // plain number
  new Value(ValueType.F64, 30),
];

assert.ok(mixed[0] instanceof Value, 'First element should be Value');
assert.ok(!(mixed[1] instanceof Value), 'Second element should be primitive');
assert.ok(mixed[2] instanceof Value, 'Third element should be Value');

// Can distinguish wrapped vs unwrapped
assert.strictEqual(mixed[0].get(), 10);
assert.strictEqual(mixed[1], 20);  // Direct access for primitive
assert.strictEqual(mixed[2].get(), 30);

console.log('✓ Can distinguish Value instances from primitives in mixed context\n');

// Test 8: Type guard function
console.log('Test 8: Type guard function');

function isValue(val) {
  return val instanceof Value;
}

assert.ok(isValue(Value.unit()), 'Type guard should identify Value');
assert.ok(isValue(new Value(ValueType.F64, 42)), 'Type guard should identify Value');
assert.ok(!isValue(42), 'Type guard should reject primitive number');
assert.ok(!isValue('test'), 'Type guard should reject primitive string');
assert.ok(!isValue(undefined), 'Type guard should reject undefined');

console.log('✓ instanceof can be used as type guard\n');

console.log('✅ All instanceof tests passed!');
