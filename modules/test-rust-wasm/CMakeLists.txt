# This CMake file is to be used by the parent project.
# Use `cargo` normally to build this project separately.
# See readme.md for more information.

# add_custom_target(generate-test-rust-wasm ALL
#   COMMAND ${ARORA_MODULE_CLI} --registry-uri ${REGISTRY_URI} generate -l rust -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml -o ${CMAKE_CURRENT_SOURCE_DIR}/target/arora
#   BYPRODUCTS
#     ${CMAKE_CURRENT_SOURCE_DIR}/ai/mod.rs
#     ${CMAKE_CURRENT_SOURCE_DIR}/ai/semio/mod.rs
#     ${CMAKE_CURRENT_SOURCE_DIR}/ai/semio/core/mod.rs
#     ${CMAKE_CURRENT_SOURCE_DIR}/ai/semio/core/error.rs
#     ${CMAKE_CURRENT_SOURCE_DIR}/ai/semio/core/status.rs
#     ${CMAKE_CURRENT_BINARY_DIR}/arora.rs
#     ${TEST_RUST_WASM_MODULE_TARGET_DIR}/arora/module.yaml
# )

# Build the WASM module.
set(TEST_RUST_WASM_MODULE_EXECUTABLE "${CMAKE_CURRENT_SOURCE_DIR}/target/wasm32-wasi/${RUST_TARGET_SUBDIR}/test_rust_wasm.wasm")
add_custom_target(test-rust-wasm-module-wasm ALL
  COMMAND cargo wasi build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${TEST_RUST_WASM_MODULE_EXECUTABLE}
)

if(BUILD_TESTING)
  # Build also for the host at build time, if tests are enabled.
  add_custom_target(test-rust-wasm-module-host ALL
    COMMAND cargo build ${CARGO_BUILD_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Unit tests for the host.
  add_test(
    NAME test-test-rust-wasm-module-host
    COMMAND cargo test
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Sanity check of the WASM library.
  add_test(
    NAME test-call-rust-wasm-from-wasmtime
    COMMAND wasmtime ${TEST_RUST_WASM_MODULE_EXECUTABLE}
      --invoke arora_function_5f423ba9_d5f9_46d7_a9b5_fb7d28f99ea6 0
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Integration test with the engine.
  add_test(
    NAME test-call-rust-wasm-from-engine
    COMMAND ${ARORA_CLI}
      --registry-uri ${REGISTRY_URI}
      --header ${CMAKE_CURRENT_SOURCE_DIR}/header.yaml
      --exe ${TEST_RUST_WASM_MODULE_EXECUTABLE}
      --call "id: 00cd31a8-2cf4-48e6-a957-69a55de90424"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
endif()
