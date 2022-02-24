add_custom_target(generate-test-cpp-2 ALL
  COMMAND ${ARORA_MODULE_CLI} --registry-uri ${REGISTRY_URI} generate -l cpp -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp-2.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/Status.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
  DEPENDS
    arora
)

add_executable(test-cpp-2
  src/main.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp-2.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
)
target_include_directories(test-cpp-2 PUBLIC ${RUST_TARGET_DIR}/include ${CMAKE_CURRENT_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp-2 PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp-2 PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(test-cpp-2 PRIVATE -Wl,--no-entry -Wl,--export=arora_buffer_alloc -Wl,--export=arora_buffer_free)

if(BUILD_TESTING)
  add_test(
    NAME test-call-cpp-wasm-from-engine-with-scalars
    COMMAND ${ARORA_CLI}
      --registry-uri ${REGISTRY_URI}
      --header ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
      --exe ${CMAKE_CURRENT_BINARY_DIR}/test-cpp-2
      --call "id: b213a552-77ad-465a-a26d-352e8eccfd63
args:
- id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
  value:
    u32: 42
- id: abf9ca4e-e03f-431a-a32b-4911f809c399
  value:
    u32: 64"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
endif()