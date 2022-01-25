add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} generate -l cpp -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp
)

add_executable(test-cpp src/main.cpp ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp)
target_include_directories(test-cpp PUBLIC ${TARGET_DIR}/include ${CMAKE_CURRENT_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto)
target_link_options(test-cpp PRIVATE -Wl,--no-entry -Wl,--export=malloc -Wl,--export=free)