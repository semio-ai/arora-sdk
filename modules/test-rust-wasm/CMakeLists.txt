# This CMake file is to be used by the parent project.
# Use `cargo` normally to build this project separately.
# See readme.md for more information.
set(MODULE_NAME test-rust-wasm)
set(TEST_RUST_WASM_MODULE_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/wasm32-wasi/${RUST_TARGET_SUBDIR}")
set(ARORA_GENERATED_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/arora_generated")
add_custom_target(generate-${MODULE_NAME}-bindings ALL
  COMMAND ${ARORA_MODULE_CLI}
    --registry-uri ${REGISTRY_URI}
    generate -l rust
      -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
      -o ${ARORA_GENERATED_SOURCE_DIR}
  BYPRODUCTS
    ${ARORA_GENERATED_SOURCE_DIR}/module.yaml
)

# Reformat the generated code.
add_custom_target(reformat-generated-${MODULE_NAME}-bindings ALL
  COMMAND rustfmt ${ARORA_GENERATED_SOURCE_DIR}/*.rs
  DEPENDS generate-${MODULE_NAME}-bindings
)

# Build the WASM module.
set(TEST_RUST_WASM_MODULE_EXECUTABLE "${TEST_RUST_WASM_MODULE_TARGET_DIR}/test_rust_wasm.wasm")
add_custom_target(${MODULE_NAME} ALL
  COMMAND cargo wasi build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${TEST_RUST_WASM_MODULE_EXECUTABLE}
  DEPENDS
    generate-${MODULE_NAME}-bindings
    reformat-generated-${MODULE_NAME}-bindings
)

if(BUILD_TESTING)
  # Build also for the host at build time, if tests are enabled.
  add_custom_target(${MODULE_NAME}-host ALL
    COMMAND cargo build ${CARGO_BUILD_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Unit tests for the host.
  add_test(
    NAME test-${MODULE_NAME}-host
    COMMAND cargo test
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Integration test with the engine.
  add_test(
    NAME test-call-${MODULE_NAME}-from-engine
    COMMAND ${ARORA_CLI}
      --registry-uri ${REGISTRY_URI}
      --header ${ARORA_GENERATED_SOURCE_DIR}/module.yaml
      --exe ${TEST_RUST_WASM_MODULE_EXECUTABLE}
      --call "id: 00cd31a8-2cf4-48e6-a957-69a55de90424"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
endif()
