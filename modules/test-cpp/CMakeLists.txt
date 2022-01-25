project (test-cpp)

cmake_minimum_required(VERSION 3.0)


set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)

set (TRIPLE wasm32-wasi)

add_compile_options(--sysroot=${WASI_SDK_PATH}/share/wasi-sysroot)

set (CMAKE_C_COMPILER ${WASI_SDK_PATH}/bin/clang)
set (CMAKE_CXX_COMPILER ${WASI_SDK_PATH}/bin/clang++)

set(CMAKE_C_COMPILER_TARGET ${TRIPLE})
set(CMAKE_CXX_COMPILER_TARGET ${TRIPLE})

add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} generate -l cpp -c ${CMAKE_SOURCE_DIR}/module.yaml -o ${CMAKE_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp
)



add_executable(test-cpp src/main.cpp ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp)
target_include_directories(test-cpp PUBLIC ${ARORA_BASE_INCLUDE_DIR} ${ARORA_CPP_INCLUDE_DIR} ${CMAKE_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp PRIVATE arora_buffers arora_util arora-cpp)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto)
target_link_options(test-cpp PRIVATE -L${WASM_LIB_DIR} -L${ARORA_CPP_LIBRARY_DIR} -Wl,--no-entry -Wl,--export=malloc -Wl,--export=free)