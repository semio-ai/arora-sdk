project(arora-cpp)
cmake_minimum_required(VERSION 3.0)

set (TRIPLE wasm32-wasi)

add_compile_options(--sysroot=${WASI_SDK_PATH}/share/wasi-sysroot)

set (CMAKE_C_COMPILER ${WASI_SDK_PATH}/bin/clang)
set (CMAKE_CXX_COMPILER ${WASI_SDK_PATH}/bin/clang++)

set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_C_COMPILER_TARGET ${TRIPLE})
set(CMAKE_CXX_COMPILER_TARGET ${TRIPLE})



add_library(arora-cpp STATIC src/arora/buffer/deserialize.cpp)
target_include_directories(arora-cpp PUBLIC ${ARORA_BASE_INCLUDE_DIR} include ${CMAKE_BINARY_DIR}/arora/include)
target_link_directories(arora-cpp PRIVATE ${WASM_LIB_DIR})
target_link_libraries(arora-cpp PRIVATE arora_buffers)
target_compile_options(arora-cpp PRIVATE -fPIC -O3 -flto)