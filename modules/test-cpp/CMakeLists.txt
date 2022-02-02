add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI} --registry-uri ${REGISTRY_URI} generate -l cpp -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp-2.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/Status.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/TestStructure1.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/TestStructure1.cpp
  DEPENDS
    arora
)

add_executable(test-cpp
  src/main.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/TestStructure1.cpp
)



target_include_directories(test-cpp PUBLIC ${RUST_TARGET_DIR}/include ${CMAKE_CURRENT_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp PRIVATE -g -pg -fPIC -flto -fno-exceptions)
target_link_options(test-cpp PRIVATE -Wl,--no-entry -Wl,--export=malloc -Wl,--export=free -Wl,--export=arora_buffer_free)

if(BUILD_TESTING)
  add_test(
    NAME test-call-cpp-wasm-from-engine
    COMMAND ${ARORA_CLI}
      --header ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
      --exe ${CMAKE_CURRENT_BINARY_DIR}/test-cpp
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
endif()