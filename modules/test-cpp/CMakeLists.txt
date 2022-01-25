add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} generate -l cpp -c ${CMAKE_SOURCE_DIR}/module.yaml -o ${CMAKE_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp
)

add_executable(test-cpp src/main.cpp ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp)
target_include_directories(test-cpp PUBLIC ${TARGET}/include ${CMAKE_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp PRIVATE arora_buffers arora_util arora-cpp)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto)
target_link_options(test-cpp PRIVATE -L${WASM_LIB_DIR} -Wl,--no-entry -Wl,--export=malloc -Wl,--export=free)