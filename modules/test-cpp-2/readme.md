# test-cpp-2

A C++ module the tests load, which calls another module: it imports
`test_2_args` from [`test-cpp`](../test-cpp/readme.md) and takes a structure
argument whose record it carries. It builds as `test-cpp` does.

It exports `test_structured(a: bool, b: TestStructure1) -> i32`.
[`module.yaml`](module.yaml) declares the export and the import;
`TestStructure1`'s record is in [`records/`](records/structure).

Its build reads test-cpp's records from `target/<profile>/modules/test-cpp/records/`
and its own from `records/` (`arora-module-cli --include`), so cargo builds
`test-cpp` first. The generated bindings hold `test-cpp`'s function too,
calling it through the engine. The module directory it writes,
`target/<profile>/modules/test-cpp-2/`, holds `header.json`,
`test-cpp-2.wasm`, and in `records/` its own record and `TestStructure1`'s.
