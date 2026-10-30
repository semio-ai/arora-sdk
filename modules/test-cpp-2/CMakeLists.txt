cmake_minimum_required(VERSION 3.20)
project(test-cpp-2 CXX)

# Required cache vars (normally passed by the wrapping build.rs):
#   ARORA_MODULE_CLI            host bin (with arora-module-cpp sibling)
#   ARORA_BUFFERS_LIB           wasm32-wasip2 static lib (.a) of arora-buffers
#   ARORA_UTIL_LIB              wasm32-wasip2 static lib (.a) of arora-util
#   ARORA_CPP_SOURCE_DIR        source dir of libs/cpp
#   ARORA_BUFFERS_INCLUDE_DIR   directory holding arora/buffers.h
#   ARORA_UTIL_INCLUDE_DIR      directory holding arora/util.h
#   TEST_CPP_RECORDS            test-cpp's published records dir
#   LOCAL_RECORDS               this module's records/ dir
#   ARORA_MODULE_DIR            the module directory to write (see below)
# CMAKE_TOOLCHAIN_FILE must be the WASI SDK's wasm32-wasip2 toolchain file.
foreach(VAR ARORA_MODULE_CLI ARORA_BUFFERS_LIB ARORA_UTIL_LIB
            ARORA_BUFFERS_INCLUDE_DIR ARORA_UTIL_INCLUDE_DIR ARORA_CPP_SOURCE_DIR
            TEST_CPP_RECORDS LOCAL_RECORDS ARORA_MODULE_DIR)
  if(NOT DEFINED ${VAR})
    message(FATAL_ERROR "Required cache variable ${VAR} not set. "
            "Either invoke through modules/test-cpp-2/build.rs or pass -D${VAR}=... explicitly.")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_subdirectory(${ARORA_CPP_SOURCE_DIR} arora-cpp-build)

add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BUFFERS_LIB})
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_BUFFERS_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_UTIL_LIB})
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_UTIL_INCLUDE_DIR})

set(GENERATED_DIR ${CMAKE_CURRENT_BINARY_DIR}/arora)
# The records are written afresh: the generator adds files and removes none,
# so a version module.yaml no longer declares would stay among them.
add_custom_target(generate-test-cpp-2-wrapper ALL
  COMMAND ${CMAKE_COMMAND} -E rm -rf ${GENERATED_DIR}/records
  COMMAND ${ARORA_MODULE_CLI}
    --include ${TEST_CPP_RECORDS}
    --include ${LOCAL_RECORDS}
    generate
    -l cpp
    -m ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
    -o ${GENERATED_DIR}
  BYPRODUCTS
    ${GENERATED_DIR}/include/test-cpp-2.hpp
    ${GENERATED_DIR}/include/test-cpp.hpp
    ${GENERATED_DIR}/source/test-cpp-2.cpp
    ${GENERATED_DIR}/source/test-cpp.cpp
    ${GENERATED_DIR}/include/types/TestStructure1.hpp
    ${GENERATED_DIR}/source/types/TestStructure1.cpp
    ${GENERATED_DIR}/header.json
)

# A component, linked as a WASI reactor, as test-cpp is (see its CMakeLists.txt).
add_executable(test-cpp-2
  src/main.cpp
  ${GENERATED_DIR}/source/test-cpp-2.cpp
  ${GENERATED_DIR}/source/test-cpp.cpp
  ${GENERATED_DIR}/source/types/TestStructure1.cpp
)
add_dependencies(test-cpp-2 generate-test-cpp-2-wrapper)
target_include_directories(test-cpp-2 PUBLIC ${GENERATED_DIR}/include)
target_link_libraries(test-cpp-2 PRIVATE arora-buffers arora-util arora-cpp arora-cpp-component)
target_compile_options(test-cpp-2 PRIVATE -fPIC -O3 -flto -fno-exceptions)
target_link_options(test-cpp-2 PRIVATE
  -mexec-model=reactor
  -Wl,--allow-multiple-definition
)

# The module directory a device loads the module from, written by the target
# test-cpp-2-module-directory: its header, the guest,
# its record, and the records of the types its functions name.
arora_module_directory(test-cpp-2 ${ARORA_MODULE_DIR}
  HEADER ${GENERATED_DIR}/header.json
  RECORDS ${GENERATED_DIR}/records ${LOCAL_RECORDS})
