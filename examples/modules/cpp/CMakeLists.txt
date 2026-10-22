cmake_minimum_required(VERSION 3.20)
project(example-cpp CXX)

# Required cache variables, which build.rs passes:
#   ARORA_MODULE_CLI      host bin that generates C++ bindings from module.yaml
#                         (with arora-module-cpp beside it)
#   ARORA_BUFFERS_LIB     wasm32-wasip1 static library (.a) of arora-buffers
#   ARORA_UTIL_LIB        wasm32-wasip1 static library (.a) of arora-util
#   ARORA_CPP_SOURCE_DIR  source directory of libs/cpp
#   ARORA_INCLUDE_DIR     directory holding the generated arora C headers
#   ARORA_MODULE_DIR      the module directory to write
# CMAKE_TOOLCHAIN_FILE must be the WASI SDK's wasm32-wasip1 toolchain file.
foreach(VAR ARORA_MODULE_CLI ARORA_BUFFERS_LIB ARORA_UTIL_LIB
            ARORA_CPP_SOURCE_DIR ARORA_INCLUDE_DIR ARORA_MODULE_DIR)
  if(NOT DEFINED ${VAR})
    message(FATAL_ERROR "Required cache variable ${VAR} not set. "
            "Either build through cargo (build.rs) or pass -D${VAR}=... explicitly.")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# arora-cpp, the C++ helpers the generated bindings use; it also defines
# arora_module_directory().
add_subdirectory(${ARORA_CPP_SOURCE_DIR} arora-cpp-build)

add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BUFFERS_LIB})
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_UTIL_LIB})
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

# module.yaml -> the bindings (include/example-cpp.hpp, source/example-cpp.cpp),
# the module's record (records/) and its header (header.json).
set(GENERATED_DIR ${CMAKE_CURRENT_BINARY_DIR}/arora)
# The records are written afresh: the generator adds files and removes none,
# so a version module.yaml no longer declares would stay among them.
add_custom_target(generate-example-cpp-bindings ALL
  COMMAND ${CMAKE_COMMAND} -E rm -rf ${GENERATED_DIR}/records
  COMMAND ${ARORA_MODULE_CLI}
    generate
    -l cpp
    -m ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
    -o ${GENERATED_DIR}
  BYPRODUCTS
    ${GENERATED_DIR}/include/example-cpp.hpp
    ${GENERATED_DIR}/source/example-cpp.cpp
    ${GENERATED_DIR}/header.json
)

# The guest links as a WASI reactor: no `main`, and an `_initialize` that the
# engine calls once, when it loads the module, to run the static constructors.
add_executable(example-cpp
  src/module.cpp
  ${GENERATED_DIR}/source/example-cpp.cpp
)
add_dependencies(example-cpp generate-example-cpp-bindings)
target_include_directories(example-cpp PUBLIC ${ARORA_INCLUDE_DIR} ${GENERATED_DIR}/include)
target_link_libraries(example-cpp PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(example-cpp PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(example-cpp PRIVATE
  -mexec-model=reactor
  -Wl,--export=arora_buffer_alloc
  -Wl,--export=arora_buffer_free
  -Wl,--allow-multiple-definition
)

# The module directory a device loads the module from, written by the target
# example-cpp-module-directory: header.json, example-cpp.wasm and records/.
arora_module_directory(example-cpp ${ARORA_MODULE_DIR}
  HEADER ${GENERATED_DIR}/header.json
  RECORDS ${GENERATED_DIR}/records)
