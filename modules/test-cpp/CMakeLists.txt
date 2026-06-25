cmake_minimum_required(VERSION 3.20)
project(test-cpp CXX)

# Required cache vars (normally passed by the wrapping build.rs):
#   ARORA_MODULE_CLI            host bin that codegens C++ from module.yaml
#   records dir for arora-behavior-tree-types-yaml
#   ARORA_BUFFERS_LIB           wasm32-wasip1 static lib (.a) of arora-buffers
#   ARORA_UTIL_LIB              wasm32-wasip1 static lib (.a) of arora-util
#   ARORA_CPP_SOURCE_DIR        source dir of libs/cpp (built inline below)
# CMAKE_TOOLCHAIN_FILE must be the WASI SDK toolchain file.
foreach(VAR ARORA_MODULE_CLI ARORA_BUFFERS_LIB
            ARORA_UTIL_LIB ARORA_CPP_SOURCE_DIR ARORA_INCLUDE_DIR)
  if(NOT DEFINED ${VAR})
    message(FATAL_ERROR "Required cache variable ${VAR} not set. "
            "Either invoke through modules/test-cpp/build.rs or pass -D${VAR}=... explicitly.")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# The arora include headers are written by the arora-buffers / arora-util
# crates to <workspace>/target/include/arora/*.h. The wrapping build.rs passes
# that directory via ARORA_INCLUDE_DIR.
if(NOT DEFINED ARORA_INCLUDE_DIR)
  message(FATAL_ERROR "ARORA_INCLUDE_DIR not set")
endif()

# Build libs/cpp inline. Same source dir as the original libs/cpp, dropped
# in our build tree so we get an arora-cpp target locally.
add_subdirectory(${ARORA_CPP_SOURCE_DIR} arora-cpp-build)

# Imported targets pointing at the bindep static libraries.
add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BUFFERS_LIB})
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_UTIL_LIB})
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

# Code generation: module.yaml -> {include,source}/test-cpp.{hpp,cpp}
set(GENERATED_DIR ${CMAKE_CURRENT_BINARY_DIR}/arora)
add_custom_target(generate-test-cpp-wrapper ALL
  COMMAND ${ARORA_MODULE_CLI}
    generate
    -l cpp
    -m ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
    -o ${GENERATED_DIR}
  BYPRODUCTS
    ${GENERATED_DIR}/include/test-cpp.hpp
    ${GENERATED_DIR}/source/test-cpp.cpp
    ${GENERATED_DIR}/module.yaml
)

# The wasm module is built as an executable (so wasi-sdk wasm-ld produces a
# proper .wasm), but with --no-entry so the engine can load it as a library
# and call the exported functions.
add_executable(test-cpp
  src/main.cpp
  ${GENERATED_DIR}/source/test-cpp.cpp
)
add_dependencies(test-cpp generate-test-cpp-wrapper)
target_include_directories(test-cpp PUBLIC ${ARORA_INCLUDE_DIR} ${GENERATED_DIR}/include)
target_link_libraries(test-cpp PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(test-cpp PRIVATE
  -Wl,--no-entry
  -Wl,--export=arora_buffer_alloc
  -Wl,--export=arora_buffer_free
  -Wl,--allow-multiple-definition
)
