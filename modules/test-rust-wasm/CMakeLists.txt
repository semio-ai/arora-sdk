# This CMake file is to be used by the parent project.
# Use `cargo` normally to build this project separately.
# See readme.md for more information.
set(MODULE_NAME test-rust-wasm)
set(TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/wasm32-wasip1/${RUST_TARGET_SUBDIR}")
set(ARORA_GENERATED_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/arora_generated")

# Build the WASM module.
set(MODULE_EXECUTABLE "${TARGET_DIR}/test_rust_wasm.wasm")
add_custom_target(${MODULE_NAME} ALL
  COMMAND cargo component build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${MODULE_EXECUTABLE}
)

if(BUILD_TESTING)
  # Build also for the host at build time, if tests are enabled.
  add_custom_target(${MODULE_NAME}-host ALL
    COMMAND cargo build ${CARGO_BUILD_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    DEPENDS ${MODULE_NAME}
  )

  # Unit tests for the host.
  add_test(
    NAME test-${MODULE_NAME}-host
    COMMAND cargo test ${CARGO_BUILD_FLAGS}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )
  set_tests_properties(test-${MODULE_NAME}-host PROPERTIES DEPENDS "${MODULE_NAME}")

  # Integration test with the engine.
  add_test(
    NAME test-call-${MODULE_NAME}-from-engine
    COMMAND ${ARORA_CLI}
      --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
      --header ${ARORA_GENERATED_SOURCE_DIR}/module.yaml
      --exe ${MODULE_EXECUTABLE}
      --call "id: 00cd31a8-2cf4-48e6-a957-69a55de90424"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
  set_tests_properties(test-call-${MODULE_NAME}-from-engine PROPERTIES DEPENDS "${MODULE_NAME}")
endif()
