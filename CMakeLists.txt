project(arora)
cmake_minimum_required(VERSION 3.0)

option (DEBUG "Build debug version" OFF)

set(TARGET_DIR "${CMAKE_SOURCE_DIR}/target")

if (DEBUG)
  set(NATIVE_LIB_DIR "${TARGET_DIR}/debug")
  set(WASM_LIB_DIR "${TARGET_DIR}/wasm32-unknown-unknown/debug")
  set(CARGO_BUILD_FLAGS)
else()
  set(NATIVE_LIB_DIR "${TARGET_DIR}/release")
  set(WASM_LIB_DIR "${TARGET_DIR}/wasm32-unknown-unknown/release")
  set(CARGO_BUILD_FLAGS --release)
endif()

set (ARORA_MODULE_CLI "${NATIVE_LIB_DIR}/arora-module-cli")

add_custom_target(arora ALL
  COMMAND cargo build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)

add_custom_target(arora-wasm ALL
  COMMAND cargo build --package arora-buffers --target wasm32-unknown-unknown ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)

install(
  FILES
    ${NATIVE_LIB_DIR}/arora-cli${CMAKE_EXECUTABLE_SUFFIX}
    ${NATIVE_LIB_DIR}/arora-module-cli${CMAKE_EXECUTABLE_SUFFIX}
    ${NATIVE_LIB_DIR}/arora-module-cpp${CMAKE_EXECUTABLE_SUFFIX}
  PERMISSIONS
    OWNER_EXECUTE
    OWNER_READ
    OWNER_WRITE
    GROUP_READ
    GROUP_EXECUTE
    WORLD_READ
    WORLD_EXECUTE
  DESTINATION bin
)

add_subdirectory(libs)
add_subdirectory(deps)
add_subdirectory(modules)