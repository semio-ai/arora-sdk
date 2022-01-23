include(ExternalProject)

set (EMSCRIPTEN_TOOLCHAIN_FILE "${CMAKE_SOURCE_DIR}/deps/emsdk/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake")

ExternalProject_Add(test-cpp
  SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/test-cpp
  BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/test-cpp
  CMAKE_CACHE_ARGS
    -DCMAKE_TOOLCHAIN_FILE:FILEPATH=${EMSCRIPTEN_TOOLCHAIN_FILE}
    -DARORA_MODULE_CLI:FILEPATH=${ARORA_MODULE_CLI}
  INSTALL_COMMAND ""
  DEPENDS binaryen
)