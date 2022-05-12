# This module is meant to be native, and supposes it is building for the host.
set(MODULE_NAME polly)
if(CMAKE_HOST_APPLE)
  message(STATUS Apple)
  set(DYLIB_EXTENSION .dylib)
elseif(CMAKE_HOST_UNIX)
  message(STATUS Unix)
  set(DYLIB_EXTENSION .so)
elseif(CMAKE_HOST_WINDOWS)
  message(STATUS Windows)
  set(DYLIB_EXTENSION .dll)
else()
  message(FATAL_ERROR "Unsupported host platform")
endif()
set(ARORA_GENERATED_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/src/arora_generated)
set(MODULE_EXECUTABLE ${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET_SUBDIR}/libpolly${DYLIB_EXTENSION})
add_custom_target(${MODULE_NAME} ALL
  COMMAND cargo build ${CARGO_BUILD_FLAGS}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  BYPRODUCTS ${MODULE_EXECUTABLE}
)

if(BUILD_TESTING)
  add_test(
    NAME test-call-${MODULE_NAME}-from-engine
    COMMAND ${ARORA_CLI}
      --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
      --header ${ARORA_GENERATED_SOURCE_DIR}/module.yaml
      --exe ${MODULE_EXECUTABLE}
      --call "id: e5a41333-4848-411f-878c-f1d662ebb4a0"
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
  )
  set_tests_properties(test-call-${MODULE_NAME}-from-engine PROPERTIES DEPENDS ${MODULE_NAME})
endif()