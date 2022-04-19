add_custom_target(module-cli ALL
  COMMAND ${ARORA_MODULE_CLI}
  --config ${REGISTRY_CONFIGURATION_FILE}
  --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
  --include ${CMAKE_BINARY_DIR}/modules/test-cpp-2/arora/records
  --include ${CMAKE_CURRENT_SOURCE_DIR}/records
  generate
  -l cpp
  -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
  -o ${CMAKE_CURRENT_BINARY_DIR}/arora
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
    generate-test-cpp-2
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
target_link_options(test-cpp PRIVATE -Wl,--no-entry -Wl,--export=arora_buffer_alloc -Wl,--export=arora_buffer_free)

if(BUILD_TESTING)
  add_test(
    NAME test-call-cpp-wasm-from-engine-with-struct
    COMMAND ${ARORA_CLI}
      --config ${REGISTRY_CONFIGURATION_FILE}
      --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
      --include ${CMAKE_CURRENT_SOURCE_DIR}/records
      --include ${CMAKE_BINARY_DIR}/modules/test-cpp-2/arora/records
      --header ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
      --exe ${CMAKE_CURRENT_BINARY_DIR}/test-cpp
      --header ${CMAKE_BINARY_DIR}/modules/test-cpp-2/arora/module.yaml
      --exe ${CMAKE_BINARY_DIR}/modules/test-cpp-2/test-cpp-2
      --call "id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
args:
- id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
  value:
    enum:
      id: 325a5767-e344-4532-860e-0749bcf2e428
      variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169
      value: unit
- id: 63086e48-804f-403a-8862-3358ddedc08d
  value:
    struct:
      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
      fields:
      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e
        value:
          enum[]:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          i32: 113"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
endif()
