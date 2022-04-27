add_custom_target(generate-test-cpp ALL
  COMMAND ${ARORA_MODULE_CLI}
  --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
  generate
  -l cpp
  -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
  -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/test-cpp.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/Status.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
    ${CMAKE_CURRENT_BINARY_DIR}/arora/records/module/23a75559-025c-4be8-8b48-0784e30dc9ec.yaml
  DEPENDS
    arora
)

# We need to make an executable to make a WASM module...
add_executable(test-cpp
  src/main.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/test-cpp.cpp
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
)
target_include_directories(test-cpp PUBLIC ${RUST_TARGET_DIR}/include ${CMAKE_CURRENT_BINARY_DIR}/arora/include)
target_link_libraries(test-cpp PRIVATE arora-buffers arora-util arora-cpp)
target_compile_options(test-cpp PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(test-cpp PRIVATE -Wl,--no-entry -Wl,--export=arora_buffer_alloc -Wl,--export=arora_buffer_free)
# ... but we don't want it to have a main entry point to use it as a library!

if(BUILD_TESTING)
  add_test(
    NAME test-call-cpp-wasm-from-engine-with-scalars
    COMMAND ${ARORA_CLI}
      --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
      --header ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
      --exe ${CMAKE_CURRENT_BINARY_DIR}/test-cpp
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