cmake_minimum_required(VERSION 3.14)

# Getting and using the WASI SDK
set(WASI_VERSION 14)
set(WASI_VERSION_FULL "${WASI_VERSION}.0")
if (WIN32)
  set(WASI_PLATFORM "mingw")
  set(CMAKE_GENERATOR "Ninja" CACHE STRING "Generator")
  set(CMAKE_MAKE_PROGRAM "ninja" CACHE STRING "Make program")
elseif (APPLE)
  set(WASI_PLATFORM "macos")
elseif (UNIX)
  set(WASI_PLATFORM "linux")
else ()
  message(FATAL "Unsupported platform")
endif()

include(FetchContent)
FetchContent_Declare(wasi_sdk
  URL https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
)
FetchContent_MakeAvailable(wasi_sdk)
set(WASI_SDK_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/_deps/wasi_sdk-src")
set(CMAKE_TOOLCHAIN_FILE "${WASI_SDK_PREFIX}/share/cmake/wasi-sdk.cmake" CACHE STRING "Toolchain file")

project(arora)
cmake_minimum_required(VERSION 3.0)

include(CTest)

set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)

option (USE_RUST_DEBUG "Use Rust debug build" OFF)
set(REGISTRY_URI "https://raw.githubusercontent.com/semio-ai/arora-registry/master/" CACHE STRING "Registry URI")

set(RUST_TARGET_DIR "${CMAKE_SOURCE_DIR}/target")

set(CARGO_BUILD_FLAGS --lib --bins)
if (USE_RUST_DEBUG)
  set(RUST_TARGET_SUBDIR "debug")
else()
  set(RUST_TARGET_SUBDIR "release")
  list(APPEND CARGO_BUILD_FLAGS --release)
endif()
if (BUILD_TESTING)
  list(APPEND CARGO_BUILD_FLAGS --tests)
endif()

set(RUST_BINARY_DIR "${RUST_TARGET_DIR}/${RUST_TARGET_SUBDIR}")
set(WASM_LIB_DIR "${RUST_TARGET_DIR}/wasm32-unknown-unknown/${RUST_TARGET_SUBDIR}")

set(ARORA_CLI "${RUST_BINARY_DIR}/arora-cli${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_CLI "${RUST_BINARY_DIR}/arora-module-cli${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_CPP "${RUST_BINARY_DIR}/arora-module-cpp${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_RUST "${RUST_BINARY_DIR}/arora-module-rust${WASI_HOST_EXE_SUFFIX}")

add_custom_target(arora ALL
  COMMAND cargo build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS
    ${ARORA_MODULE_CLI}
    ${ARORA_MODULE_CPP}
    ${ARORA_MODULE_RUST}
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

install(
  FILES
    ${ARORA_CLI}
    ${ARORA_MODULE_CLI}
    ${ARORA_MODULE_CPP}
    ${ARORA_MODULE_RUST}
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

if(BUILD_TESTING)
  add_test(
    NAME cargo_test
    COMMAND cargo test
  )
endif()

add_subdirectory(libs)
add_subdirectory(modules)