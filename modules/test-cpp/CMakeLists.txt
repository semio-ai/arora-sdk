cmake_minimum_required(VERSION 3.20)
project(test-cpp CXX)

# Required cache vars (normally passed by the wrapping build.rs):
#   ARORA_MODULE_CLI            host bin that codegens C++ from module.yaml
#   ARORA_BUFFERS_LIB           wasm32-wasip2 static lib (.a) of arora-buffers
#   ARORA_UTIL_LIB              wasm32-wasip2 static lib (.a) of arora-util
#   ARORA_BUFFERS_INCLUDE_DIR   directory holding arora/buffers.h
#   ARORA_UTIL_INCLUDE_DIR      directory holding arora/util.h
#   ARORA_CPP_SOURCE_DIR        source dir of libs/cpp (built inline below)
#   ARORA_INCLUDE_DIR           target/include (generated arora C headers)
#   ARORA_MODULE_DIR            the module directory to write (see below)
# CMAKE_TOOLCHAIN_FILE must be the WASI SDK's wasm32-wasip2 toolchain file.
foreach(VAR ARORA_MODULE_CLI ARORA_BUFFERS_LIB ARORA_UTIL_LIB
            ARORA_BUFFERS_INCLUDE_DIR ARORA_UTIL_INCLUDE_DIR
            ARORA_CPP_SOURCE_DIR ARORA_MODULE_DIR)
  if(NOT DEFINED ${VAR})
    message(FATAL_ERROR "Required cache variable ${VAR} not set. "
            "Either invoke through modules/test-cpp/build.rs or pass -D${VAR}=... explicitly.")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Build libs/cpp inline. Same source dir as the original libs/cpp, dropped
# in our build tree so we get an arora-cpp target locally.
add_subdirectory(${ARORA_CPP_SOURCE_DIR} arora-cpp-build)

# Imported targets pointing at the bindep static libraries, each with the
# directory of its C header, which the crate's build script generates under
# its OUT_DIR.
add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BUFFERS_LIB})
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_BUFFERS_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_UTIL_LIB})
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_UTIL_INCLUDE_DIR})

# Code generation: module.yaml -> {include,source}/test-cpp.{hpp,cpp}
set(GENERATED_DIR ${CMAKE_CURRENT_BINARY_DIR}/arora)
# The records are written afresh: the generator adds files and removes none,
# so a version module.yaml no longer declares would stay among them.
add_custom_target(generate-test-cpp-wrapper ALL
  COMMAND ${CMAKE_COMMAND} -E rm -rf ${GENERATED_DIR}/records
  COMMAND ${ARORA_MODULE_CLI}
    generate
    -l cpp
    -m ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
    -o ${GENERATED_DIR}
  BYPRODUCTS
    ${GENERATED_DIR}/include/test-cpp.hpp
    ${GENERATED_DIR}/source/test-cpp.cpp
    ${GENERATED_DIR}/module.yaml
    ${GENERATED_DIR}/header.json
)

# The guest is a component of the `arora:module` world, as a Rust guest is:
# arora-cpp-component serves the module's functions through the world's
# `functions` and `dispatch` exports. It links as a WASI reactor: no `main`,
# no `_start`, and an `_initialize` the component calls once, when it is
# instantiated, to run the C++ static constructors. Its state then lives
# across calls.
add_executable(test-cpp
  src/main.cpp
  ${GENERATED_DIR}/source/test-cpp.cpp
)
add_dependencies(test-cpp generate-test-cpp-wrapper)
target_include_directories(test-cpp PUBLIC ${GENERATED_DIR}/include)
target_link_libraries(test-cpp PRIVATE arora-buffers arora-util arora-cpp arora-cpp-component)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(test-cpp PRIVATE
  -mexec-model=reactor
  -Wl,--allow-multiple-definition
)

# The module directory a device loads the module from, written by the target
# test-cpp-module-directory: its header, the guest,
# and its record.
arora_module_directory(test-cpp ${ARORA_MODULE_DIR}
  HEADER ${GENERATED_DIR}/header.json
  RECORDS ${GENERATED_DIR}/records)
