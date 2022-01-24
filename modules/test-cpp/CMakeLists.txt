project (test-cpp)

cmake_minimum_required(VERSION 3.0)

set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)

add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} generate -l cpp -c ${CMAKE_SOURCE_DIR}/module.yaml -o ${CMAKE_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp
)



add_executable(test-cpp src/main.cpp ${CMAKE_BINARY_DIR}/arora/source/test-cpp.cpp)
target_include_directories(test-cpp PUBLIC ${ARORA_BASE_INCLUDE_DIR} ${ARORA_CPP_INCLUDE_DIR} ${CMAKE_BINARY_DIR}/arora/include)
target_link_directories(test-cpp PRIVATE ${WASM_LIB_DIR})
target_link_libraries(test-cpp PRIVATE arora_buffers)
target_compile_options(test-cpp PRIVATE -fPIC -Os -flto)
target_link_options(test-cpp PRIVATE
  -sMAIN_MODULE=2 
  -sEXPORTED_FUNCTIONS=['_arora_function_07f5740c_ba4a_45af_8ec5_bedde5737e99']
)