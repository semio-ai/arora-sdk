project(arora)
cmake_minimum_required(VERSION 3.0)

include(CTest)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(USE_RUST_DEBUG ON CACHE BOOL "Use Rust debug build")
set(NAO OFF CACHE BOOL "Cross-compile for NAO and build its specific module")

set(RUST_TARGET_DIR "${CMAKE_SOURCE_DIR}/target")

set(CARGO_BUILD_FLAGS --lib --bins)
if(USE_RUST_DEBUG)
  message(STATUS "Will build Rust crates in debug")
  set(RUST_TARGET_SUBDIR "debug")
else()
message(STATUS "Will build Rust crates in release")
  set(RUST_TARGET_SUBDIR "release")
  list(APPEND CARGO_BUILD_FLAGS --release)
endif()
if(BUILD_TESTING)
  list(APPEND CARGO_BUILD_FLAGS --tests)
endif()

set(RUST_BINARY_DIR "${RUST_TARGET_DIR}/${RUST_TARGET_SUBDIR}")
set(WASM_LIB_DIR "${RUST_TARGET_DIR}/wasm32-unknown-unknown/${RUST_TARGET_SUBDIR}")

set(ARORA_CLI "${RUST_BINARY_DIR}/arora-cli${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_CLI "${RUST_BINARY_DIR}/arora-module-cli${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_CPP "${RUST_BINARY_DIR}/arora-module-cpp${WASI_HOST_EXE_SUFFIX}")
set(ARORA_MODULE_RUST "${RUST_BINARY_DIR}/arora-module-rust${WASI_HOST_EXE_SUFFIX}")
set(ARORA_BEHAVIOR_TREE_INCLUDE "${CMAKE_CURRENT_SOURCE_DIR}/crates/arora-behavior-tree-types-yaml/records")

if(NAO)
  set(CROSSCOMPILING_TARGET_SYSTEM linux)
  set(CROSSCOMPILING_TARGET_PLATFORM i686-unknown-${CROSSCOMPILING_TARGET_SYSTEM}-musl)

  # Only works on Mac for now.
  execute_process(
    COMMAND brew list --versions ${CROSSCOMPILING_TARGET_PLATFORM}
    OUTPUT_VARIABLE HOMEBREW_FORMULA_VERSION
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  string(REGEX REPLACE "[A-Za-z0-9_-]* ([A-Za-z0-9_-]*)" "\\1" TOOLCHAIN_VERSION "${HOMEBREW_FORMULA_VERSION}")

  execute_process(
    COMMAND brew --cellar ${CROSSCOMPILING_TARGET_PLATFORM}
    OUTPUT_VARIABLE HOMEBREW_FORMULA_DIR
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  set(TOOLCHAIN_DIR ${HOMEBREW_FORMULA_DIR}/${TOOLCHAIN_VERSION})

  set(
    NAO_ENV
    ENABLE_NAO=1
    CC_i686_unknown_linux_musl=${TOOLCHAIN_DIR}/bin/${CROSSCOMPILING_TARGET_PLATFORM}-gcc
    CXX_i686_unknown_linux_musl=${TOOLCHAIN_DIR}/bin/${CROSSCOMPILING_TARGET_PLATFORM}-g++
    AR_i686_unknown_linux_musl=${TOOLCHAIN_DIR}/bin/${CROSSCOMPILING_TARGET_PLATFORM}-ar
    CARGO_TARGET_I686_UNKNOWN_LINUX_MUSL_LINKER=${TOOLCHAIN_DIR}/bin/${CROSSCOMPILING_TARGET_PLATFORM}-gcc
  )

  set(NATIVE_TARGET_FLAG --target ${CROSSCOMPILING_TARGET_PLATFORM})
endif()

set(CARGO_COMMAND ${CMAKE_COMMAND} -E env ${NAO_ENV} cargo)

add_custom_target(arora-host-tools
  COMMAND cargo build --bins # no target flag, building for the host
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS
    ${ARORA_MODULE_CLI}
    ${ARORA_MODULE_CPP}
    ${ARORA_MODULE_RUST}
)

add_custom_target(arora ALL
  COMMAND ${CARGO_COMMAND} build ${CARGO_BUILD_FLAGS} ${NATIVE_TARGET_FLAG}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)
add_dependencies(arora arora-host-tools)

add_custom_target(arora-buffers-build ALL
  COMMAND ${CARGO_COMMAND} build --package arora-buffers --target wasm32-unknown-unknown ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)
link_directories(${WASM_LIB_DIR})

set (CMAKE_SKIP_RPATH ON)

add_library(arora-buffers INTERFACE)
target_link_libraries(arora-buffers INTERFACE arora_buffers)
add_dependencies(arora-buffers arora-buffers-build)

add_custom_target(arora-util-build ALL
  COMMAND ${CARGO_COMMAND} build --package arora-util --target wasm32-unknown-unknown ${CARGO_BUILD_FLAGS}
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
    COMMAND ${CARGO_COMMAND} test ${CARGO_BUILD_FLAGS}
  )
endif()

add_subdirectory(libs)
add_subdirectory(modules)