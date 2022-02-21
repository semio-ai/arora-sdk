# This CMake file is to be used by the parent project.
# Use `cargo` normally to build this project separately.
# See readme.md for more information.
set(MODULE_NAME behavior-tree-nodes)
set(TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/wasm32-wasi/${RUST_TARGET_SUBDIR}")
set(ARORA_GENERATED_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/arora_generated")
add_custom_target(generate-${MODULE_NAME}-bindings ALL
  COMMAND ${ARORA_MODULE_CLI}
    --registry-uri ${REGISTRY_URI}
    generate -l rust
      -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
      -o ${ARORA_GENERATED_SOURCE_DIR}
  BYPRODUCTS
    ${ARORA_GENERATED_SOURCE_DIR}/module.yaml
  DEPENDS
    ${ARORA_MODULE_CLI}
    ${ARORA_MODULE_RUST}
)

# Reformat the generated code.
add_custom_target(reformat-generated-${MODULE_NAME}-bindings ALL
  COMMAND rustfmt ${ARORA_GENERATED_SOURCE_DIR}/*.rs
  DEPENDS generate-${MODULE_NAME}-bindings
)

# Build the WASM module.
set(MODULE_EXECUTABLE "${TARGET_DIR}/test_rust_wasm.wasm")
add_custom_target(${MODULE_NAME} ALL
  COMMAND cargo wasi build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${MODULE_EXECUTABLE}
  DEPENDS
    generate-${MODULE_NAME}-bindings
    reformat-generated-${MODULE_NAME}-bindings
)
