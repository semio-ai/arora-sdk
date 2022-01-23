project (test-cpp)

cmake_minimum_required(VERSION 3.0)

add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} generate -l cpp -c ${CMAKE_SOURCE_DIR}/module.yaml -o ${CMAKE_BINARY_DIR}/arora
)

add_executable(test-cpp src/main.cpp)