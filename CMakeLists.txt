project(arora)
cmake_minimum_required(VERSION 3.0)

set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)

option (DEBUG "Build debug version" OFF)
option (REGISTRY_URI "Registry URI" "https://raw.githubusercontent.com/semio-ai/arora-registry/master/")

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


add_custom_target(arora ALL
  COMMAND cargo build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)

add_custom_target(arora-buffers-build ALL
  COMMAND cargo build --package arora-buffers --target wasm32-unknown-unknown ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)
link_directories(${WASM_LIB_DIR})

set (CMAKE_SKIP_RPATH ON)


add_library(arora-buffers INTERFACE)
target_link_libraries(arora-buffers INTERFACE arora_buffers)
add_dependencies(arora-buffers arora-buffers-build)

add_custom_target(arora-util-build ALL
  COMMAND cargo build --package arora-util --target wasm32-unknown-unknown ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)

add_library(arora-util INTERFACE)
target_link_libraries(arora-util INTERFACE arora_util)
add_dependencies(arora-util arora-util-build)


set (ARORA_MODULE_CLI "${NATIVE_LIB_DIR}/arora-module-cli${WASI_HOST_EXE_SUFFIX}")

install(
  FILES
    ${NATIVE_LIB_DIR}/arora-cli${WASI_HOST_EXE_SUFFIX}
    ${NATIVE_LIB_DIR}/arora-module-cli${WASI_HOST_EXE_SUFFIX}
    ${NATIVE_LIB_DIR}/arora-module-cpp${WASI_HOST_EXE_SUFFIX}
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
add_subdirectory(modules)