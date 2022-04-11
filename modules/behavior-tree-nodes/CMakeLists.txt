# This CMake file is to be used by the parent project.
# Use `cargo` normally to build this project separately.
# See readme.md for more information.
set(MODULE_NAME behavior-tree-nodes)
set(TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/wasm32-wasi/${RUST_TARGET_SUBDIR}")
set(ARORA_GENERATED_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/arora_generated")

# Build the WASM module.
set(MODULE_EXECUTABLE "${TARGET_DIR}/${MODULE_NAME}.wasm")
add_custom_target(${MODULE_NAME} ALL
  COMMAND cargo wasi build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${MODULE_EXECUTABLE}
)
