add_custom_target(generate-test-cpp-2 ALL
  COMMAND ${ARORA_MODULE_CLI} --registry-uri ${REGISTRY_URI} generate -l cpp -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp-2.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/Status.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
)

add_executable(test-cpp-2
  src/main.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
)
target_include_directories(test-cpp-2 PUBLIC ${RUST_TARGET_DIR}/include ${CMAKE_CURRENT_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp-2 PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp-2 PRIVATE -fPIC -O3 -flto)
target_link_options(test-cpp-2 PRIVATE -Wl,--no-entry -Wl,--export=malloc -Wl,--export=free -Wl,--export=arora_buffer_free)
